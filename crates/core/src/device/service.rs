use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use chrono::{DateTime, Utc};

use crate::{
    Error, RepositoryError,
    book::{BookFile, BookId, FileFormat, FileRole},
    device::{BookSyncEntry, Device, DeviceBook, DeviceId, DeviceToken, NewDevice, NewDeviceSyncLog, OnRemovalAction, SyncDiff, SyncStatus},
    filter::{BookFilter, FilterReadStatus, FilterRule, SetOp},
    reading::{
        ReadStatus,
        service::{apply_transition, default_state},
    },
    repository::RepositoryService,
    shelf::{NewShelf, Shelf, ShelfType, ShelfVisibility},
    user::UserId,
    with_read_only_transaction, with_transaction,
};

// ── Trait ──────────────────────────────────────────────────────────────────────

#[async_trait::async_trait]
pub trait DeviceService: Send + Sync {
    /// Returns all devices belonging to the given user.
    async fn list_devices_for_user(&self, user_id: UserId) -> Result<Vec<Device>, Error>;

    /// Returns a single device identified by token, verifying ownership.
    ///
    /// Returns `NotFound` if the token does not exist or belongs to another
    /// user.
    async fn get_device(&self, token: &DeviceToken, user_id: UserId) -> Result<Device, Error>;

    /// Creates a new device and a companion private smart shelf (atomically).
    ///
    /// The companion shelf is named after the device and pre-configured with a
    /// `ReadStatus IncludesAny [Active]` filter — the standard "sync to device"
    /// filter. Returns the new device's token.
    async fn create_device(&self, owner_id: UserId, name: String, device_type: String, on_removal_action: OnRemovalAction) -> Result<DeviceToken, Error>;

    /// Updates a device's name and on-removal action.
    ///
    /// If the name changes the companion shelf (if any) is renamed to match
    /// within the same transaction.
    async fn update_device(&self, token: &DeviceToken, name: String, on_removal_action: OnRemovalAction, user_id: UserId) -> Result<(), Error>;

    /// Deletes a device.
    ///
    /// When `delete_companion_shelf` is `true` the linked shelf is deleted
    /// first; otherwise it survives as a regular unlinked smart shelf (the FK
    /// is cleared automatically by `ON DELETE SET NULL`).
    async fn delete_device(&self, token: &DeviceToken, delete_companion_shelf: bool, user_id: UserId) -> Result<(), Error>;

    /// Returns the companion shelf linked to the given device, if one exists.
    async fn get_companion_shelf(&self, device_id: DeviceId) -> Result<Option<Shelf>, Error>;

    /// Suggests a default name for a new device owned by the given user.
    ///
    /// Uses the first word of the user's `full_name` (falling back to `"My"`)
    /// to build `"{first}'s Device"`. Appends `" (N)"` when that name is
    /// already taken.
    async fn default_device_name(&self, owner_id: UserId) -> Result<String, Error>;

    /// Computes which books need to be added, upgraded, refreshed, or removed
    /// for a device sync page.
    ///
    /// Loads all books from the device's companion shelf and all current
    /// `DeviceBook` records, classifies each book, sorts by `book_id`, then
    /// applies the keyset cursor (`after_book_id`) to return at most
    /// `page_size` entries. Removals are included only on the first page
    /// (`after_book_id` is `None`).
    ///
    /// `since` should be `device.last_synced_at` for incremental syncs, or
    /// `None` for a full initial sync.
    async fn compute_sync_diff(
        &self,
        device_id: DeviceId,
        owner_id: UserId,
        since: Option<DateTime<Utc>>,
        after_book_id: Option<BookId>,
        page_size: u64,
    ) -> Result<SyncDiff, Error>;

    /// Applies a sync diff page: upserts `DeviceBook` records for new and
    /// upgraded books, deletes records for removed books, and writes a
    /// `DeviceSyncLog` entry. Updates `Device.last_synced_at` only on the
    /// final page (`!diff.has_more`).
    async fn apply_sync(&self, device_id: DeviceId, diff: &SyncDiff) -> Result<(), Error>;

    /// Looks up a device by its token without verifying ownership.
    ///
    /// Used by the Kobo sync extractor where the token itself is the
    /// authentication credential. Returns `None` if no device with that token
    /// exists.
    async fn find_device_by_token(&self, token: &DeviceToken) -> Result<Option<Device>, Error>;

    /// Clears all `DeviceBook` records for a device, forcing a full resync on
    /// the next Kobo library sync. Also resets `last_synced_at` to `None`.
    ///
    /// Returns `NotFound` if the token does not exist or belongs to another
    /// user.
    async fn reset_device_sync(&self, token: &DeviceToken, user_id: UserId) -> Result<(), Error>;

    /// Removes a single `DeviceBook` record for the given device and book.
    ///
    /// Called when the Kobo sends `DELETE /v1/library/{uuid}` to signal that
    /// the user removed the book from the device. Removing the record ensures
    /// the book is re-delivered as `New` on the next sync. Idempotent — returns
    /// `Ok(())` if the record did not exist.
    async fn remove_book_from_device(&self, device_id: DeviceId, book_id: BookId) -> Result<(), Error>;
}

// ── Implementation
// ─────────────────────────────────────────────────────────────

/// Classification of a shelf book during sync diff computation.
enum EntryKind {
    New,
    Upgraded,
    Refreshed,
}

/// Selects the best file to send to a Kobo device from a book's available
/// files. Priority: Enriched Kepub → Enriched Epub → Original Kepub →
/// Original Epub. Returns `None` if no suitable file is available.
fn select_best_file(files: &[BookFile]) -> Option<BookFile> {
    let candidates = [
        (FileFormat::Kepub, FileRole::Enriched),
        (FileFormat::Epub, FileRole::Enriched),
        (FileFormat::Kepub, FileRole::Original),
        (FileFormat::Epub, FileRole::Original),
    ];
    for (format, role) in &candidates {
        if let Some(f) = files.iter().find(|f| &f.format == format && &f.file_role == role) {
            return Some(f.clone());
        }
    }
    None
}

pub(crate) struct DeviceServiceImpl {
    repository_service: Arc<RepositoryService>,
}

impl DeviceServiceImpl {
    pub(crate) fn new(repository_service: Arc<RepositoryService>) -> Self {
        Self { repository_service }
    }
}

/// Derives the preferred file format for a device type.
///
/// Currently only `"kobo"` maps to EPUB; all other types return `None`.
fn derive_preferred_format(device_type: &str) -> Option<FileFormat> {
    match device_type.to_lowercase().as_str() {
        "kobo" => Some(FileFormat::Epub),
        _ => None,
    }
}

/// Builds the default companion-shelf filter: `ReadStatus IncludesAny
/// [Active]`.
fn device_shelf_filter() -> BookFilter {
    BookFilter::Rule(FilterRule::ReadStatus {
        op: SetOp::IncludesAny,
        values: vec![FilterReadStatus::Active],
    })
}

#[async_trait::async_trait]
impl DeviceService for DeviceServiceImpl {
    // #[tracing::instrument(level = "trace", skip(self))]
    async fn list_devices_for_user(&self, user_id: UserId) -> Result<Vec<Device>, Error> {
        with_read_only_transaction!(self, device_repository, |tx| device_repository.list_for_user(tx, user_id).await)
    }

    // #[tracing::instrument(level = "trace", skip(self))]
    async fn get_device(&self, token: &DeviceToken, user_id: UserId) -> Result<Device, Error> {
        let token = *token;

        with_read_only_transaction!(self, device_repository, |tx| {
            let device = device_repository
                .find_by_token(tx, &token)
                .await?
                .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

            if device.owner_id != user_id {
                return Err(Error::RepositoryError(RepositoryError::NotFound));
            }

            Ok(device)
        })
    }

    // #[tracing::instrument(level = "trace", skip(self))]
    async fn create_device(&self, owner_id: UserId, name: String, device_type: String, on_removal_action: OnRemovalAction) -> Result<DeviceToken, Error> {
        if name.trim().is_empty() {
            return Err(Error::Validation("device name must not be empty".to_string()));
        }

        let preferred_format = derive_preferred_format(&device_type);

        with_transaction!(self, device_repository, shelf_repository, |tx| {
            let device = device_repository
                .add_device(
                    tx,
                    NewDevice {
                        owner_id,
                        name: name.clone(),
                        device_type,
                        preferred_format,
                        on_removal_action,
                    },
                )
                .await?;

            shelf_repository
                .add_shelf(
                    tx,
                    NewShelf {
                        owner_id,
                        name,
                        shelf_type: ShelfType::Smart,
                        visibility: ShelfVisibility::Private,
                        device_id: Some(device.id),
                        filter_criteria: Some(device_shelf_filter()),
                    },
                )
                .await?;

            Ok(device.token)
        })
    }

    // #[tracing::instrument(level = "trace", skip(self))]
    async fn update_device(&self, token: &DeviceToken, name: String, on_removal_action: OnRemovalAction, user_id: UserId) -> Result<(), Error> {
        if name.trim().is_empty() {
            return Err(Error::Validation("device name must not be empty".to_string()));
        }

        let token = *token;

        with_transaction!(self, device_repository, shelf_repository, |tx| {
            let device = device_repository
                .find_by_token(tx, &token)
                .await?
                .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

            if device.owner_id != user_id {
                return Err(Error::RepositoryError(RepositoryError::NotFound));
            }

            let name_changed = device.name != name;

            let updated = Device {
                name: name.clone(),
                on_removal_action,
                ..device.clone()
            };
            device_repository.update_device(tx, updated).await?;

            if name_changed {
                if let Some(shelf) = shelf_repository.find_by_device_id(tx, device.id).await? {
                    let renamed = Shelf { name, ..shelf };
                    shelf_repository.update_shelf(tx, renamed).await?;
                }
            }

            Ok(())
        })
    }

    // #[tracing::instrument(level = "trace", skip(self))]
    async fn delete_device(&self, token: &DeviceToken, delete_companion_shelf: bool, user_id: UserId) -> Result<(), Error> {
        let token = *token;

        with_transaction!(self, device_repository, shelf_repository, |tx| {
            let device = device_repository
                .find_by_token(tx, &token)
                .await?
                .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

            if device.owner_id != user_id {
                return Err(Error::RepositoryError(RepositoryError::NotFound));
            }

            if delete_companion_shelf {
                if let Some(shelf) = shelf_repository.find_by_device_id(tx, device.id).await? {
                    shelf_repository.delete_shelf(tx, shelf).await?;
                }
            }

            device_repository.delete_device(tx, device).await
        })
    }

    // #[tracing::instrument(level = "trace", skip(self))]
    async fn get_companion_shelf(&self, device_id: DeviceId) -> Result<Option<Shelf>, Error> {
        with_read_only_transaction!(self, shelf_repository, |tx| shelf_repository.find_by_device_id(tx, device_id).await)
    }

    // #[tracing::instrument(level = "trace", skip(self))]
    async fn default_device_name(&self, owner_id: UserId) -> Result<String, Error> {
        with_read_only_transaction!(self, user_repository, device_repository, |tx| {
            let user = user_repository
                .find_by_id(tx, owner_id)
                .await?
                .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

            let first_word = user.full_name.split_whitespace().next().unwrap_or("My").to_string();

            let base = format!("{first_word}'s Device");

            let count = device_repository.count_with_name_prefix(tx, owner_id, &base).await?;

            if count == 0 { Ok(base) } else { Ok(format!("{base} ({})", count + 1)) }
        })
    }

    // #[tracing::instrument(level = "debug", skip(self))]
    async fn compute_sync_diff(
        &self,
        device_id: DeviceId,
        owner_id: UserId,
        since: Option<DateTime<Utc>>,
        after_book_id: Option<BookId>,
        page_size: u64,
    ) -> Result<SyncDiff, Error> {
        with_read_only_transaction!(self, shelf_repository, book_repository, device_repository, |tx| {
            // 1. Find companion shelf — no shelf means nothing to sync
            let Some(shelf) = shelf_repository.find_by_device_id(tx, device_id).await? else {
                tracing::debug!(device_id, "no companion shelf found, returning empty diff");
                return Ok(SyncDiff::empty());
            };
            let Some(ref filter) = shelf.filter_criteria else {
                tracing::warn!(device_id, shelf_id = shelf.id, "companion shelf has no filter criteria");
                return Ok(SyncDiff::empty());
            };

            // 2. Load all shelf books with no page limit, then sort by book_id for
            //    deterministic keyset pagination
            let mut shelf_books = shelf_repository.books_for_filter(tx, filter, owner_id, None, None).await?;
            shelf_books.sort_by_key(|b| b.id);

            // 3. Load all DeviceBook records for quick lookup
            let device_books = device_repository.books_for_device(tx, device_id).await?;
            let device_book_map: HashMap<BookId, DeviceBook> = device_books.iter().map(|db| (db.book_id, db.clone())).collect();

            // 4. Detect removals: books in DeviceBook that are no longer on the shelf. Only
            //    included on the first page to avoid duplicating them across pages.
            let shelf_id_set: HashSet<BookId> = shelf_books.iter().map(|b| b.id).collect();
            let removed_book_ids = if after_book_id.is_none() {
                device_books
                    .iter()
                    .filter(|db| !shelf_id_set.contains(&db.book_id))
                    .map(|db| db.book_id)
                    .collect()
            } else {
                vec![]
            };

            // 5. Classify each shelf book — N+1 files query accepted; fast indexed lookups
            //    on a personal library are negligible
            let mut classified: Vec<(EntryKind, BookSyncEntry)> = Vec::new();
            for book in &shelf_books {
                let files = book_repository.files_for_book(tx, book.id).await?;
                let Some(best) = select_best_file(&files) else {
                    tracing::debug!(
                        book_id = book.id,
                        title = %book.title,
                        "no suitable file available, skipping"
                    );
                    continue;
                };

                let kind = match device_book_map.get(&book.id) {
                    None => EntryKind::New,
                    Some(db) if db.format != best.format || db.file_role != best.file_role => EntryKind::Upgraded,
                    // No cursor (since = None) means full sync from scratch — re-deliver all
                    // existing books as Refreshed so nothing is skipped. Also re-deliver any
                    // book whose metadata has changed since the last sync.
                    Some(_) if since.is_none() || since.is_some_and(|s| book.updated_at > s) => EntryKind::Refreshed,
                    Some(_) => continue, // unchanged — skip
                };

                tracing::info!(
                    book_id = book.id,
                    title = %book.title,
                    format = ?best.format,
                    file_role = ?best.file_role,
                    kind = match &kind {
                        EntryKind::New => "new",
                        EntryKind::Upgraded => "upgraded",
                        EntryKind::Refreshed => "refreshed",
                    },
                    "preparing book for kobo sync"
                );
                classified.push((
                    kind,
                    BookSyncEntry {
                        book: book.clone(),
                        file: best,
                    },
                ));
            }

            // 6. Apply keyset cursor and extract page
            let start = match after_book_id {
                None => 0,
                Some(id) => classified.partition_point(|(_, e)| e.book.id <= id),
            };
            let end = (start + page_size as usize).min(classified.len());
            let has_more = end < classified.len();

            // 7. Distribute page entries into their categories
            let mut new_books = Vec::new();
            let mut upgraded_books = Vec::new();
            let mut refreshed_books = Vec::new();
            for (kind, entry) in classified.drain(start..end) {
                match kind {
                    EntryKind::New => new_books.push(entry),
                    EntryKind::Upgraded => upgraded_books.push(entry),
                    EntryKind::Refreshed => refreshed_books.push(entry),
                }
            }

            Ok(SyncDiff {
                new_books,
                upgraded_books,
                refreshed_books,
                removed_book_ids,
                has_more,
            })
        })
    }

    // #[tracing::instrument(level = "debug", skip(self, diff))]
    async fn apply_sync(&self, device_id: DeviceId, diff: &SyncDiff) -> Result<(), Error> {
        let now = Utc::now();

        let new_count = diff.new_books.len();
        let upgraded_count = diff.upgraded_books.len();
        let refreshed_count = diff.refreshed_books.len();
        let removed_count = diff.removed_book_ids.len();
        let books_added = (new_count + upgraded_count + refreshed_count) as i32;
        let books_removed = removed_count as i32;
        let has_more = diff.has_more;

        // Clone entries for the async move closure (diff is a borrowed reference)
        let new_books = diff.new_books.clone();
        let upgraded_books = diff.upgraded_books.clone();
        let removed_book_ids = diff.removed_book_ids.clone();

        with_transaction!(self, device_repository, |tx| {
            // Add new DeviceBook records
            for entry in &new_books {
                device_repository
                    .add_device_book(
                        tx,
                        DeviceBook {
                            device_id,
                            book_id: entry.book.id,
                            format: entry.file.format.clone(),
                            file_role: entry.file.file_role.clone(),
                            synced_at: now,
                        },
                    )
                    .await?;
            }

            // Update DeviceBook records for upgraded books (different file)
            for entry in &upgraded_books {
                device_repository
                    .update_device_book(
                        tx,
                        DeviceBook {
                            device_id,
                            book_id: entry.book.id,
                            format: entry.file.format.clone(),
                            file_role: entry.file.file_role.clone(),
                            synced_at: now,
                        },
                    )
                    .await?;
            }

            // Remove DeviceBook records for books no longer on the companion shelf
            for book_id in &removed_book_ids {
                device_repository.remove_device_book(tx, device_id, *book_id).await?;
            }

            // Write a sync log entry for this page
            device_repository
                .add_sync_log(
                    tx,
                    NewDeviceSyncLog {
                        device_id,
                        status: SyncStatus::Completed,
                        books_added,
                        books_removed,
                        started_at: now,
                        completed_at: Some(now),
                    },
                )
                .await?;

            // On the final page, update the device's last_synced_at
            if !has_more {
                let device = device_repository
                    .find_by_id(tx, device_id)
                    .await?
                    .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

                device_repository
                    .update_device(
                        tx,
                        Device {
                            last_synced_at: Some(now),
                            ..device
                        },
                    )
                    .await?;
            }

            tracing::info!(
                device_id,
                new = new_count,
                upgraded = upgraded_count,
                refreshed = refreshed_count,
                removed = removed_count,
                has_more,
                "kobo sync page applied"
            );

            Ok(())
        })
    }

    // #[tracing::instrument(level = "trace", skip(self))]
    async fn find_device_by_token(&self, token: &DeviceToken) -> Result<Option<Device>, Error> {
        let token = *token;
        with_read_only_transaction!(self, device_repository, |tx| device_repository.find_by_token(tx, &token).await)
    }

    async fn reset_device_sync(&self, token: &DeviceToken, user_id: UserId) -> Result<(), Error> {
        let token = *token;
        with_transaction!(self, device_repository, |tx| {
            let device = device_repository
                .find_by_token(tx, &token)
                .await?
                .filter(|d| d.owner_id == user_id)
                .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

            // Resetting last_synced_at to None is enough: the library sync
            // handler treats None as a server-side override that ignores the
            // Kobo's cursor and forces a full re-classification on the next sync.
            // DeviceBook records are preserved so re-sent books are classified
            // as Refreshed (existing) or New (not yet sent) rather than all New.
            let updated = Device {
                last_synced_at: None,
                ..device
            };
            device_repository.update_device(tx, updated).await?;

            Ok(())
        })
    }

    async fn remove_book_from_device(&self, device_id: DeviceId, book_id: BookId) -> Result<(), Error> {
        with_transaction!(self, device_repository, user_book_metadata_repository, |tx| {
            let device = device_repository
                .find_by_id(tx, device_id)
                .await?
                .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

            device_repository.remove_device_book(tx, device_id, book_id).await?;

            let target_status = match device.on_removal_action {
                OnRemovalAction::Nothing => return Ok(()),
                OnRemovalAction::MarkRead => ReadStatus::Read,
                OnRemovalAction::MarkDnf => ReadStatus::Abandoned,
            };

            let current = user_book_metadata_repository
                .find_by_user_and_book(tx, device.owner_id, book_id)
                .await?
                .unwrap_or_else(|| default_state(device.owner_id, book_id));
            let next = apply_transition(current, target_status);
            user_book_metadata_repository.upsert(tx, next).await?;

            Ok(())
        })
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::{
        auth::{NewSession, Session, SessionRepository},
        book::{
            Author, AuthorId, AuthorRepository, AuthorRole, AuthorToken, Book, BookAuthor, BookIdentifier, BookQuery, BookRepository, BookStatus, BookToken,
            Genre, GenreId, GenreRepository, GenreToken, IdentifierType, NewAuthor, NewBook, NewGenre, NewPublisher, NewSeries, NewTag, Publisher, PublisherId,
            PublisherRepository, PublisherToken, Series, SeriesId, SeriesRepository, SeriesToken, Tag, TagId, TagRepository, TagToken,
        },
        device::{DeviceRepository, DeviceSyncLog},
        import::{ImportJob, ImportJobId, ImportJobRepository, ImportJobToken, ImportStatus, NewImportJob},
        jobs::{Job, JobRepository},
        reading::{ReadStatus, UserBookMetadata, UserBookMetadataRepository},
        repository::{Repository, RepositoryServiceBuilder, Transaction},
        shelf::{BookShelf, ShelfId, ShelfRepository, ShelfToken},
        user::{NewUser, NewUserSetting, User, UserRepository, UserSetting, UserSettingRepository, UserToken},
    };

    // ─── Mock Repository (provides transactions) ──────────────────────────────

    struct MockRepository;

    #[async_trait::async_trait]
    impl Repository for MockRepository {
        async fn begin(&self) -> Result<Box<dyn Transaction>, Error> {
            Ok(Box::new(MockTransaction))
        }
        async fn begin_read_only(&self) -> Result<Box<dyn Transaction>, Error> {
            Ok(Box::new(MockTransaction))
        }
        async fn close(&self) -> Result<(), Error> {
            Ok(())
        }
    }

    struct MockTransaction;

    #[async_trait::async_trait]
    impl Transaction for MockTransaction {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        async fn commit(self: Box<Self>) -> Result<(), Error> {
            Ok(())
        }
        async fn rollback(self: Box<Self>) -> Result<(), Error> {
            Ok(())
        }
    }

    // ─── Mock DeviceRepository ────────────────────────────────────────────────

    #[derive(Default)]
    struct MockDeviceRepository {
        add_device_result: Mutex<Option<Result<Device, Error>>>,
        update_device_result: Mutex<Option<Result<Device, Error>>>,
        delete_device_called: Mutex<bool>,
        find_by_id_result: Mutex<Option<Result<Option<Device>, Error>>>,
        find_by_token_result: Mutex<Option<Result<Option<Device>, Error>>>,
        list_for_user_result: Mutex<Option<Result<Vec<Device>, Error>>>,
        count_with_name_prefix_result: Mutex<Option<Result<u64, Error>>>,
        books_for_device_result: Mutex<Option<Result<Vec<DeviceBook>, Error>>>,
        added_device_books: Mutex<Vec<DeviceBook>>,
        updated_device_books: Mutex<Vec<DeviceBook>>,
        removed_device_books: Mutex<Vec<(DeviceId, BookId)>>,
        sync_logs: Mutex<Vec<NewDeviceSyncLog>>,
    }

    impl MockDeviceRepository {
        fn with_add_device(self, result: Result<Device, Error>) -> Self {
            *self.add_device_result.lock().unwrap() = Some(result);
            self
        }
        fn with_update_device(self, result: Result<Device, Error>) -> Self {
            *self.update_device_result.lock().unwrap() = Some(result);
            self
        }
        fn with_find_by_id(self, result: Result<Option<Device>, Error>) -> Self {
            *self.find_by_id_result.lock().unwrap() = Some(result);
            self
        }
        fn with_find_by_token(self, result: Result<Option<Device>, Error>) -> Self {
            *self.find_by_token_result.lock().unwrap() = Some(result);
            self
        }
        fn with_list_for_user(self, result: Result<Vec<Device>, Error>) -> Self {
            *self.list_for_user_result.lock().unwrap() = Some(result);
            self
        }
        fn with_count_with_name_prefix(self, result: Result<u64, Error>) -> Self {
            *self.count_with_name_prefix_result.lock().unwrap() = Some(result);
            self
        }
        fn with_books_for_device(self, result: Result<Vec<DeviceBook>, Error>) -> Self {
            *self.books_for_device_result.lock().unwrap() = Some(result);
            self
        }
    }

    #[async_trait::async_trait]
    impl DeviceRepository for MockDeviceRepository {
        async fn add_device(&self, _: &dyn Transaction, _: NewDevice) -> Result<Device, Error> {
            self.add_device_result
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| Err(Error::MockNotConfigured("add_device")))
        }
        async fn update_device(&self, _: &dyn Transaction, _: Device) -> Result<Device, Error> {
            self.update_device_result
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| Err(Error::MockNotConfigured("update_device")))
        }
        async fn delete_device(&self, _: &dyn Transaction, _: Device) -> Result<(), Error> {
            *self.delete_device_called.lock().unwrap() = true;
            Ok(())
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: DeviceId) -> Result<Option<Device>, Error> {
            self.find_by_id_result
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| Err(Error::MockNotConfigured("find_by_id")))
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &DeviceToken) -> Result<Option<Device>, Error> {
            self.find_by_token_result
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| Err(Error::MockNotConfigured("find_by_token")))
        }
        async fn list_for_user(&self, _: &dyn Transaction, _: UserId) -> Result<Vec<Device>, Error> {
            self.list_for_user_result.lock().unwrap().clone().unwrap_or(Ok(vec![]))
        }
        async fn count_with_name_prefix(&self, _: &dyn Transaction, _: UserId, _: &str) -> Result<u64, Error> {
            self.count_with_name_prefix_result
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| Err(Error::MockNotConfigured("count_with_name_prefix")))
        }
        async fn add_device_book(&self, _: &dyn Transaction, book: DeviceBook) -> Result<DeviceBook, Error> {
            let clone = book.clone();
            self.added_device_books.lock().unwrap().push(book);
            Ok(clone)
        }
        async fn remove_device_book(&self, _: &dyn Transaction, device_id: DeviceId, book_id: BookId) -> Result<(), Error> {
            self.removed_device_books.lock().unwrap().push((device_id, book_id));
            Ok(())
        }
        async fn clear_device_books(&self, _: &dyn Transaction, _: DeviceId) -> Result<(), Error> {
            Ok(())
        }
        async fn update_device_book(&self, _: &dyn Transaction, book: DeviceBook) -> Result<DeviceBook, Error> {
            let clone = book.clone();
            self.updated_device_books.lock().unwrap().push(book);
            Ok(clone)
        }
        async fn books_for_device(&self, _: &dyn Transaction, _: DeviceId) -> Result<Vec<DeviceBook>, Error> {
            self.books_for_device_result.lock().unwrap().clone().unwrap_or(Ok(vec![]))
        }
        async fn add_sync_log(&self, _: &dyn Transaction, log: NewDeviceSyncLog) -> Result<DeviceSyncLog, Error> {
            let result = DeviceSyncLog {
                id: 1,
                device_id: log.device_id,
                status: log.status.clone(),
                books_added: log.books_added,
                books_removed: log.books_removed,
                started_at: log.started_at,
                completed_at: log.completed_at,
            };
            self.sync_logs.lock().unwrap().push(log);
            Ok(result)
        }
        async fn list_sync_logs_for_device(&self, _: &dyn Transaction, _: DeviceId, _: Option<u64>) -> Result<Vec<DeviceSyncLog>, Error> {
            unimplemented!()
        }
    }

    // ─── Mock ShelfRepository ─────────────────────────────────────────────────

    #[derive(Default)]
    struct MockShelfRepository {
        add_shelf_result: Mutex<Option<Result<Shelf, Error>>>,
        update_shelf_result: Mutex<Option<Result<Shelf, Error>>>,
        delete_shelf_called: Mutex<bool>,
        find_by_device_id_result: Mutex<Option<Result<Option<Shelf>, Error>>>,
        books_for_filter_result: Mutex<Option<Result<Vec<Book>, Error>>>,
    }

    impl MockShelfRepository {
        fn with_add_shelf(self, result: Result<Shelf, Error>) -> Self {
            *self.add_shelf_result.lock().unwrap() = Some(result);
            self
        }
        fn with_update_shelf(self, result: Result<Shelf, Error>) -> Self {
            *self.update_shelf_result.lock().unwrap() = Some(result);
            self
        }
        fn with_find_by_device_id(self, result: Result<Option<Shelf>, Error>) -> Self {
            *self.find_by_device_id_result.lock().unwrap() = Some(result);
            self
        }
        fn with_books_for_filter(self, result: Result<Vec<Book>, Error>) -> Self {
            *self.books_for_filter_result.lock().unwrap() = Some(result);
            self
        }
    }

    #[async_trait::async_trait]
    impl ShelfRepository for MockShelfRepository {
        async fn add_shelf(&self, _: &dyn Transaction, _: NewShelf) -> Result<Shelf, Error> {
            self.add_shelf_result
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| Err(Error::MockNotConfigured("add_shelf")))
        }
        async fn update_shelf(&self, _: &dyn Transaction, _: Shelf) -> Result<Shelf, Error> {
            self.update_shelf_result
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| Err(Error::MockNotConfigured("update_shelf")))
        }
        async fn delete_shelf(&self, _: &dyn Transaction, _: Shelf) -> Result<(), Error> {
            *self.delete_shelf_called.lock().unwrap() = true;
            Ok(())
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: ShelfId) -> Result<Option<Shelf>, Error> {
            unimplemented!()
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &ShelfToken) -> Result<Option<Shelf>, Error> {
            unimplemented!()
        }
        async fn list_for_user(&self, _: &dyn Transaction, _: UserId) -> Result<Vec<Shelf>, Error> {
            Ok(vec![])
        }
        async fn list_public_shelves(&self, _: &dyn Transaction, _: UserId) -> Result<Vec<Shelf>, Error> {
            Ok(vec![])
        }
        async fn add_book_to_shelf(&self, _: &dyn Transaction, _: BookShelf) -> Result<BookShelf, Error> {
            unimplemented!()
        }
        async fn remove_book_from_shelf(&self, _: &dyn Transaction, _: ShelfId, _: BookId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn books_for_shelf(&self, _: &dyn Transaction, _: ShelfId, _: Option<BookId>, _: Option<u64>) -> Result<Vec<BookShelf>, Error> {
            unimplemented!()
        }
        async fn books_for_filter(&self, _: &dyn Transaction, _: &BookFilter, _: UserId, _: Option<BookId>, _: Option<u64>) -> Result<Vec<Book>, Error> {
            self.books_for_filter_result.lock().unwrap().clone().unwrap_or(Ok(vec![]))
        }
        async fn count_for_filter(&self, _: &dyn Transaction, _: &BookFilter, _: UserId) -> Result<u64, Error> {
            unimplemented!()
        }
        async fn find_by_device_id(&self, _: &dyn Transaction, _: DeviceId) -> Result<Option<Shelf>, Error> {
            self.find_by_device_id_result.lock().unwrap().clone().unwrap_or(Ok(None))
        }
    }

    // ─── Stub repositories ────────────────────────────────────────────────────

    struct MockSessionRepository;
    #[async_trait::async_trait]
    impl SessionRepository for MockSessionRepository {
        async fn count(&self, _: &dyn Transaction) -> Result<i64, Error> {
            unimplemented!()
        }
        async fn store(&self, _: &dyn Transaction, _: NewSession) -> Result<Session, Error> {
            unimplemented!()
        }
        async fn load(&self, _: &dyn Transaction, _: &str) -> Result<Option<Session>, Error> {
            unimplemented!()
        }
        async fn delete_by_id(&self, _: &dyn Transaction, _: &str) -> Result<(), Error> {
            unimplemented!()
        }
        async fn exists(&self, _: &dyn Transaction, _: &str) -> Result<bool, Error> {
            unimplemented!()
        }
        async fn delete_by_expiry(&self, _: &dyn Transaction) -> Result<Vec<String>, Error> {
            unimplemented!()
        }
        async fn delete_all(&self, _: &dyn Transaction) -> Result<(), Error> {
            unimplemented!()
        }
        async fn get_ids(&self, _: &dyn Transaction) -> Result<Vec<String>, Error> {
            unimplemented!()
        }
    }

    #[derive(Default)]
    struct MockUserRepository {
        find_by_id_result: Mutex<Option<Result<Option<User>, Error>>>,
    }

    impl MockUserRepository {
        fn with_find_by_id(self, result: Result<Option<User>, Error>) -> Self {
            *self.find_by_id_result.lock().unwrap() = Some(result);
            self
        }
    }

    #[async_trait::async_trait]
    impl UserRepository for MockUserRepository {
        async fn add_user(&self, _: &dyn Transaction, _: NewUser) -> Result<User, Error> {
            unimplemented!()
        }
        async fn update_user(&self, _: &dyn Transaction, _: User) -> Result<User, Error> {
            unimplemented!()
        }
        async fn delete_user(&self, _: &dyn Transaction, _: User) -> Result<User, Error> {
            unimplemented!()
        }
        async fn list_users(&self, _: &dyn Transaction, _: Option<UserId>, _: Option<u64>) -> Result<Vec<User>, Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: UserId) -> Result<Option<User>, Error> {
            self.find_by_id_result
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| Err(Error::MockNotConfigured("find_by_id")))
        }
        async fn find_by_username(&self, _: &dyn Transaction, _: &str) -> Result<Option<User>, Error> {
            unimplemented!()
        }
    }

    struct MockUserSettingRepository;
    #[async_trait::async_trait]
    impl UserSettingRepository for MockUserSettingRepository {
        async fn get(&self, _: &dyn Transaction, _: UserId, _: &str) -> Result<Option<UserSetting>, Error> {
            unimplemented!()
        }
        async fn set(&self, _: &dyn Transaction, _: NewUserSetting) -> Result<UserSetting, Error> {
            unimplemented!()
        }
        async fn delete(&self, _: &dyn Transaction, _: UserId, _: &str) -> Result<(), Error> {
            unimplemented!()
        }
        async fn list_by_user(&self, _: &dyn Transaction, _: UserId) -> Result<Vec<UserSetting>, Error> {
            unimplemented!()
        }
    }

    struct MockAuthorRepository;
    #[async_trait::async_trait]
    impl AuthorRepository for MockAuthorRepository {
        async fn add_author(&self, _: &dyn Transaction, _: NewAuthor) -> Result<Author, Error> {
            unimplemented!()
        }
        async fn update_author(&self, _: &dyn Transaction, _: Author) -> Result<Author, Error> {
            unimplemented!()
        }
        async fn delete_author(&self, _: &dyn Transaction, _: AuthorId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: AuthorId) -> Result<Option<Author>, Error> {
            unimplemented!()
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &AuthorToken) -> Result<Option<Author>, Error> {
            unimplemented!()
        }
        async fn find_by_name(&self, _: &dyn Transaction, _: &str) -> Result<Option<Author>, Error> {
            unimplemented!()
        }
        async fn list_authors(&self, _: &dyn Transaction, _: Option<AuthorId>, _: Option<u64>) -> Result<Vec<Author>, Error> {
            unimplemented!()
        }
        async fn count_authors(&self, _: &dyn Transaction) -> Result<u64, Error> {
            unimplemented!()
        }
        async fn list_all_authors(&self, _: &dyn Transaction) -> Result<Vec<Author>, Error> {
            unimplemented!()
        }
    }

    struct MockSeriesRepository;
    #[async_trait::async_trait]
    impl SeriesRepository for MockSeriesRepository {
        async fn add_series(&self, _: &dyn Transaction, _: NewSeries) -> Result<Series, Error> {
            unimplemented!()
        }
        async fn update_series(&self, _: &dyn Transaction, _: Series) -> Result<Series, Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: SeriesId) -> Result<Option<Series>, Error> {
            unimplemented!()
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &SeriesToken) -> Result<Option<Series>, Error> {
            unimplemented!()
        }
        async fn find_by_name(&self, _: &dyn Transaction, _: &str) -> Result<Option<Series>, Error> {
            unimplemented!()
        }
        async fn list_series(&self, _: &dyn Transaction, _: Option<SeriesId>, _: Option<u64>) -> Result<Vec<Series>, Error> {
            unimplemented!()
        }
        async fn list_all_series(&self, _: &dyn Transaction) -> Result<Vec<Series>, Error> {
            unimplemented!()
        }
        async fn max_series_number_for_series(&self, _: &dyn Transaction, _: SeriesId) -> Result<Option<rust_decimal::Decimal>, Error> {
            unimplemented!()
        }
    }

    struct MockPublisherRepository;
    #[async_trait::async_trait]
    impl PublisherRepository for MockPublisherRepository {
        async fn add_publisher(&self, _: &dyn Transaction, _: NewPublisher) -> Result<Publisher, Error> {
            unimplemented!()
        }
        async fn update_publisher(&self, _: &dyn Transaction, _: Publisher) -> Result<Publisher, Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: PublisherId) -> Result<Option<Publisher>, Error> {
            unimplemented!()
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &PublisherToken) -> Result<Option<Publisher>, Error> {
            unimplemented!()
        }
        async fn find_by_name(&self, _: &dyn Transaction, _: &str) -> Result<Option<Publisher>, Error> {
            unimplemented!()
        }
        async fn list_publishers(&self, _: &dyn Transaction, _: Option<PublisherId>, _: Option<u64>) -> Result<Vec<Publisher>, Error> {
            unimplemented!()
        }
        async fn list_all_publishers(&self, _: &dyn Transaction) -> Result<Vec<Publisher>, Error> {
            unimplemented!()
        }
    }

    struct MockGenreRepository;
    #[async_trait::async_trait]
    impl GenreRepository for MockGenreRepository {
        async fn add_genre(&self, _: &dyn Transaction, _: NewGenre) -> Result<Genre, Error> {
            unimplemented!()
        }
        async fn update_genre(&self, _: &dyn Transaction, _: Genre) -> Result<Genre, Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: GenreId) -> Result<Option<Genre>, Error> {
            unimplemented!()
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &GenreToken) -> Result<Option<Genre>, Error> {
            unimplemented!()
        }
        async fn find_by_name(&self, _: &dyn Transaction, _: &str) -> Result<Option<Genre>, Error> {
            unimplemented!()
        }
        async fn list_genres(&self, _: &dyn Transaction, _: Option<GenreId>, _: Option<u64>) -> Result<Vec<Genre>, Error> {
            unimplemented!()
        }
        async fn list_all_genres(&self, _: &dyn Transaction) -> Result<Vec<Genre>, Error> {
            unimplemented!()
        }
    }

    struct MockTagRepository;
    #[async_trait::async_trait]
    impl TagRepository for MockTagRepository {
        async fn add_tag(&self, _: &dyn Transaction, _: NewTag) -> Result<Tag, Error> {
            unimplemented!()
        }
        async fn update_tag(&self, _: &dyn Transaction, _: Tag) -> Result<Tag, Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: TagId) -> Result<Option<Tag>, Error> {
            unimplemented!()
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &TagToken) -> Result<Option<Tag>, Error> {
            unimplemented!()
        }
        async fn find_by_name(&self, _: &dyn Transaction, _: &str) -> Result<Option<Tag>, Error> {
            unimplemented!()
        }
        async fn list_tags(&self, _: &dyn Transaction, _: Option<TagId>, _: Option<u64>) -> Result<Vec<Tag>, Error> {
            unimplemented!()
        }
        async fn list_all_tags(&self, _: &dyn Transaction) -> Result<Vec<Tag>, Error> {
            unimplemented!()
        }
    }

    struct MockImportJobRepository;
    #[async_trait::async_trait]
    impl ImportJobRepository for MockImportJobRepository {
        async fn add_job(&self, _: &dyn Transaction, _: NewImportJob) -> Result<ImportJob, Error> {
            unimplemented!()
        }
        async fn update_job(&self, _: &dyn Transaction, _: ImportJob) -> Result<ImportJob, Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: ImportJobId) -> Result<Option<ImportJob>, Error> {
            unimplemented!()
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &ImportJobToken) -> Result<Option<ImportJob>, Error> {
            unimplemented!()
        }
        async fn find_by_hash(&self, _: &dyn Transaction, _: &str) -> Result<Option<ImportJob>, Error> {
            unimplemented!()
        }
        async fn list_by_status(&self, _: &dyn Transaction, _: ImportStatus, _: Option<ImportJobId>, _: Option<u64>) -> Result<Vec<ImportJob>, Error> {
            unimplemented!()
        }
        async fn reset_in_progress_to_pending(&self, _: &dyn Transaction) -> Result<u64, Error> {
            unimplemented!()
        }
        async fn find_by_candidate_book_id(&self, _: &dyn Transaction, _: BookId) -> Result<Option<ImportJob>, Error> {
            unimplemented!()
        }
        async fn delete_job(&self, _: &dyn Transaction, _: ImportJobId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn approve_job(&self, _: &dyn Transaction, _: ImportJobId) -> Result<(), Error> {
            unimplemented!()
        }
    }

    struct MockJobRepository;
    #[async_trait::async_trait]
    impl JobRepository for MockJobRepository {
        async fn enqueue_raw(&self, _: &dyn Transaction, _: &str, _: serde_json::Value, _: i16) -> Result<Job, Error> {
            unimplemented!()
        }
        async fn claim_next(&self, _: &dyn Transaction) -> Result<Option<Job>, Error> {
            unimplemented!()
        }
        async fn complete(&self, _: &dyn Transaction, _: Job) -> Result<Job, Error> {
            unimplemented!()
        }
        async fn fail(&self, _: &dyn Transaction, _: Job, _: String) -> Result<Job, Error> {
            unimplemented!()
        }
        async fn reset_running_to_pending(&self, _: &dyn Transaction) -> Result<u64, Error> {
            unimplemented!()
        }
        async fn count_pending_by_type(&self, _: &dyn Transaction, _: &str) -> Result<u64, Error> {
            unimplemented!()
        }
    }

    struct MockUserBookMetadataRepository;
    #[async_trait::async_trait]
    impl UserBookMetadataRepository for MockUserBookMetadataRepository {
        async fn upsert(&self, _: &dyn Transaction, _: UserBookMetadata) -> Result<UserBookMetadata, Error> {
            unimplemented!()
        }
        async fn find_by_user_and_book(&self, _: &dyn Transaction, _: UserId, _: BookId) -> Result<Option<UserBookMetadata>, Error> {
            unimplemented!()
        }
        async fn list_for_user(
            &self,
            _: &dyn Transaction,
            _: UserId,
            _: Option<ReadStatus>,
            _: Option<BookId>,
            _: Option<u64>,
        ) -> Result<Vec<UserBookMetadata>, Error> {
            unimplemented!()
        }
        async fn list_for_user_and_books(&self, _: &dyn Transaction, _: UserId, _: &[BookId]) -> Result<Vec<UserBookMetadata>, Error> {
            unimplemented!()
        }
    }

    // ─── Helpers ──────────────────────────────────────────────────────────────

    fn create_service(device_repo: MockDeviceRepository, shelf_repo: MockShelfRepository, user_repo: MockUserRepository) -> DeviceServiceImpl {
        let repository_service = Arc::new(
            RepositoryServiceBuilder::default()
                .repository(Arc::new(MockRepository) as Arc<dyn Repository>)
                .session_repository(Arc::new(MockSessionRepository) as Arc<dyn SessionRepository>)
                .user_repository(Arc::new(user_repo) as Arc<dyn UserRepository>)
                .user_setting_repository(Arc::new(MockUserSettingRepository) as Arc<dyn UserSettingRepository>)
                .author_repository(Arc::new(MockAuthorRepository) as Arc<dyn AuthorRepository>)
                .series_repository(Arc::new(MockSeriesRepository) as Arc<dyn SeriesRepository>)
                .publisher_repository(Arc::new(MockPublisherRepository) as Arc<dyn PublisherRepository>)
                .genre_repository(Arc::new(MockGenreRepository) as Arc<dyn GenreRepository>)
                .tag_repository(Arc::new(MockTagRepository) as Arc<dyn TagRepository>)
                .book_repository(Arc::new(MockBookRepository::default()) as Arc<dyn BookRepository>)
                .import_job_repository(Arc::new(MockImportJobRepository) as Arc<dyn ImportJobRepository>)
                .job_repository(Arc::new(MockJobRepository) as Arc<dyn JobRepository>)
                .shelf_repository(Arc::new(shelf_repo) as Arc<dyn ShelfRepository>)
                .user_book_metadata_repository(Arc::new(MockUserBookMetadataRepository) as Arc<dyn UserBookMetadataRepository>)
                .device_repository(Arc::new(device_repo) as Arc<dyn DeviceRepository>)
                .build()
                .expect("all fields provided"),
        );
        DeviceServiceImpl::new(repository_service)
    }

    #[derive(Default)]
    struct MockBookRepository {
        files_for_book_results: Mutex<HashMap<BookId, Vec<BookFile>>>,
    }

    impl MockBookRepository {
        fn with_files(self, book_id: BookId, files: Vec<BookFile>) -> Self {
            self.files_for_book_results.lock().unwrap().insert(book_id, files);
            self
        }
    }

    #[async_trait::async_trait]
    impl BookRepository for MockBookRepository {
        async fn add_book(&self, _: &dyn Transaction, _: NewBook) -> Result<Book, Error> {
            unimplemented!()
        }
        async fn update_book(&self, _: &dyn Transaction, _: Book) -> Result<Book, Error> {
            unimplemented!()
        }
        async fn delete_book(&self, _: &dyn Transaction, _: BookId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn find_by_id(&self, _: &dyn Transaction, _: BookId) -> Result<Option<Book>, Error> {
            unimplemented!()
        }
        async fn find_by_token(&self, _: &dyn Transaction, _: &BookToken) -> Result<Option<Book>, Error> {
            unimplemented!()
        }
        async fn list_books(&self, _: &dyn Transaction, _: &BookQuery, _: Option<BookId>, _: Option<u64>) -> Result<Vec<Book>, Error> {
            unimplemented!()
        }
        async fn count_available_books(&self, _: &dyn Transaction) -> Result<u64, Error> {
            unimplemented!()
        }
        async fn count_books_for_author(&self, _: &dyn Transaction, _: AuthorId) -> Result<u64, Error> {
            unimplemented!()
        }
        async fn add_book_author(&self, _: &dyn Transaction, _: BookId, _: AuthorId, _: AuthorRole, _: i32) -> Result<(), Error> {
            unimplemented!()
        }
        async fn delete_book_authors(&self, _: &dyn Transaction, _: BookId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn authors_for_book(&self, _: &dyn Transaction, _: BookId) -> Result<Vec<BookAuthor>, Error> {
            unimplemented!()
        }
        async fn add_book_identifier(&self, _: &dyn Transaction, _: BookId, _: IdentifierType, _: String) -> Result<(), Error> {
            unimplemented!()
        }
        async fn delete_book_identifiers(&self, _: &dyn Transaction, _: BookId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn identifiers_for_book(&self, _: &dyn Transaction, _: BookId) -> Result<Vec<BookIdentifier>, Error> {
            unimplemented!()
        }
        async fn add_book_file(&self, _: &dyn Transaction, _: BookId, _: FileFormat, _: FileRole, _: String, _: i64, _: String) -> Result<BookFile, Error> {
            unimplemented!()
        }
        async fn files_for_book(&self, _: &dyn Transaction, book_id: BookId) -> Result<Vec<BookFile>, Error> {
            Ok(self.files_for_book_results.lock().unwrap().get(&book_id).cloned().unwrap_or_default())
        }
        async fn find_file_by_hash(&self, _: &dyn Transaction, _: &str) -> Result<Option<BookFile>, Error> {
            unimplemented!()
        }
        async fn genres_for_book(&self, _: &dyn Transaction, _: BookId) -> Result<Vec<Genre>, Error> {
            unimplemented!()
        }
        async fn tags_for_book(&self, _: &dyn Transaction, _: BookId) -> Result<Vec<Tag>, Error> {
            unimplemented!()
        }
        async fn add_book_genre(&self, _: &dyn Transaction, _: BookId, _: GenreId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn add_book_tag(&self, _: &dyn Transaction, _: BookId, _: TagId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn delete_book_genres(&self, _: &dyn Transaction, _: BookId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn delete_book_tags(&self, _: &dyn Transaction, _: BookId) -> Result<(), Error> {
            unimplemented!()
        }
        async fn delete_book_file_by_role(&self, _: &dyn Transaction, _: BookId, _: FileFormat, _: FileRole) -> Result<(), Error> {
            unimplemented!()
        }
        async fn find_book_ids_needing_enrichment(&self, _: &dyn Transaction) -> Result<Vec<BookId>, Error> {
            unimplemented!()
        }

        async fn find_book_ids_needing_kepub_conversion(&self, _: &dyn Transaction) -> Result<Vec<BookId>, Error> {
            unimplemented!()
        }

        async fn update_enriched_paths(&self, _: &dyn Transaction, _: BookId, _: &str, _: &str) -> Result<(), Error> {
            unimplemented!()
        }
    }

    fn fake_device(owner_id: UserId) -> Device {
        Device {
            id: 1,
            version: 1,
            token: DeviceToken::new(1),
            owner_id,
            name: "My Device".to_string(),
            device_type: "kobo".to_string(),
            preferred_format: Some(FileFormat::Epub),
            on_removal_action: OnRemovalAction::Nothing,
            last_synced_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn fake_shelf(owner_id: UserId, device_id: Option<DeviceId>) -> Shelf {
        Shelf {
            id: 10,
            version: 1,
            token: ShelfToken::new(10),
            owner_id,
            name: "My Device".to_string(),
            shelf_type: ShelfType::Smart,
            visibility: ShelfVisibility::Private,
            device_id,
            filter_criteria: Some(device_shelf_filter()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn fake_user(id: UserId, full_name: &str) -> User {
        User {
            id,
            version: 1,
            token: UserToken::new(id),
            username: "test".to_string(),
            full_name: full_name.to_string(),
            password_hash: String::new(),
            email_address: crate::types::EmailAddress::new("test@example.com").unwrap(),
            capabilities: Default::default(),
            change_password_on_login: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn create_sync_service(device_repo: MockDeviceRepository, shelf_repo: MockShelfRepository, book_repo: MockBookRepository) -> DeviceServiceImpl {
        let repository_service = Arc::new(
            RepositoryServiceBuilder::default()
                .repository(Arc::new(MockRepository) as Arc<dyn Repository>)
                .session_repository(Arc::new(MockSessionRepository) as Arc<dyn SessionRepository>)
                .user_repository(Arc::new(MockUserRepository::default()) as Arc<dyn UserRepository>)
                .user_setting_repository(Arc::new(MockUserSettingRepository) as Arc<dyn UserSettingRepository>)
                .author_repository(Arc::new(MockAuthorRepository) as Arc<dyn AuthorRepository>)
                .series_repository(Arc::new(MockSeriesRepository) as Arc<dyn SeriesRepository>)
                .publisher_repository(Arc::new(MockPublisherRepository) as Arc<dyn PublisherRepository>)
                .genre_repository(Arc::new(MockGenreRepository) as Arc<dyn GenreRepository>)
                .tag_repository(Arc::new(MockTagRepository) as Arc<dyn TagRepository>)
                .book_repository(Arc::new(book_repo) as Arc<dyn BookRepository>)
                .import_job_repository(Arc::new(MockImportJobRepository) as Arc<dyn ImportJobRepository>)
                .job_repository(Arc::new(MockJobRepository) as Arc<dyn JobRepository>)
                .shelf_repository(Arc::new(shelf_repo) as Arc<dyn ShelfRepository>)
                .user_book_metadata_repository(Arc::new(MockUserBookMetadataRepository) as Arc<dyn UserBookMetadataRepository>)
                .device_repository(Arc::new(device_repo) as Arc<dyn DeviceRepository>)
                .build()
                .expect("all fields provided"),
        );
        DeviceServiceImpl::new(repository_service)
    }

    fn fake_book_file(book_id: BookId, format: FileFormat, file_role: FileRole) -> BookFile {
        BookFile {
            book_id,
            format,
            file_role,
            path: String::new(),
            file_size: 0,
            file_hash: String::new(),
        }
    }

    fn fake_device_book(device_id: DeviceId, book_id: BookId, format: FileFormat, file_role: FileRole) -> DeviceBook {
        DeviceBook {
            device_id,
            book_id,
            format,
            file_role,
            synced_at: Utc::now(),
        }
    }

    // ─── derive_preferred_format ──────────────────────────────────────────────

    #[test]
    fn test_derive_preferred_format_kobo() {
        assert_eq!(derive_preferred_format("kobo"), Some(FileFormat::Epub));
        assert_eq!(derive_preferred_format("Kobo"), Some(FileFormat::Epub));
    }

    #[test]
    fn test_derive_preferred_format_unknown() {
        assert_eq!(derive_preferred_format("kindle"), None);
        assert_eq!(derive_preferred_format(""), None);
    }

    // ─── list_devices_for_user ────────────────────────────────────────────────

    #[tokio::test]
    async fn test_list_devices_returns_devices() {
        let device = fake_device(1);
        let svc = create_service(
            MockDeviceRepository::default().with_list_for_user(Ok(vec![device.clone()])),
            MockShelfRepository::default(),
            MockUserRepository::default(),
        );

        let result = svc.list_devices_for_user(1).await.unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, device.id);
    }

    // ─── get_device ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_get_device_success() {
        let device = fake_device(1);
        let token = device.token;
        let svc = create_service(
            MockDeviceRepository::default().with_find_by_token(Ok(Some(device.clone()))),
            MockShelfRepository::default(),
            MockUserRepository::default(),
        );

        let result = svc.get_device(&token, 1).await.unwrap();

        assert_eq!(result.id, device.id);
    }

    #[tokio::test]
    async fn test_get_device_not_found() {
        let token = DeviceToken::new(99);
        let svc = create_service(
            MockDeviceRepository::default().with_find_by_token(Ok(None)),
            MockShelfRepository::default(),
            MockUserRepository::default(),
        );

        let result = svc.get_device(&token, 1).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::NotFound))));
    }

    #[tokio::test]
    async fn test_get_device_wrong_owner_returns_not_found() {
        let device = fake_device(1); // owned by user 1
        let token = device.token;
        let svc = create_service(
            MockDeviceRepository::default().with_find_by_token(Ok(Some(device))),
            MockShelfRepository::default(),
            MockUserRepository::default(),
        );

        let result = svc.get_device(&token, 2).await; // user 2 requests

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::NotFound))));
    }

    // ─── create_device ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_create_device_returns_token() {
        let device = fake_device(1);
        let expected_token = device.token;
        let shelf = fake_shelf(1, Some(device.id));
        let svc = create_service(
            MockDeviceRepository::default().with_add_device(Ok(device)),
            MockShelfRepository::default().with_add_shelf(Ok(shelf)),
            MockUserRepository::default(),
        );

        let result = svc
            .create_device(1, "My Device".to_string(), "kobo".to_string(), OnRemovalAction::Nothing)
            .await;

        assert_eq!(result.unwrap(), expected_token);
    }

    #[tokio::test]
    async fn test_create_device_empty_name_returns_validation_error() {
        let svc = create_service(MockDeviceRepository::default(), MockShelfRepository::default(), MockUserRepository::default());

        let result = svc.create_device(1, "  ".to_string(), "kobo".to_string(), OnRemovalAction::Nothing).await;

        assert!(matches!(result, Err(Error::Validation(_))));
    }

    // ─── update_device ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_update_device_renames_companion_shelf() {
        let device = fake_device(1);
        let token = device.token;
        let shelf = fake_shelf(1, Some(device.id));
        let updated_device = Device {
            name: "New Name".to_string(),
            ..device.clone()
        };
        let updated_shelf = Shelf {
            name: "New Name".to_string(),
            ..shelf.clone()
        };

        let svc = create_service(
            MockDeviceRepository::default()
                .with_find_by_token(Ok(Some(device)))
                .with_update_device(Ok(updated_device)),
            MockShelfRepository::default()
                .with_find_by_device_id(Ok(Some(shelf)))
                .with_update_shelf(Ok(updated_shelf)),
            MockUserRepository::default(),
        );

        let result = svc.update_device(&token, "New Name".to_string(), OnRemovalAction::Nothing, 1).await;

        result.unwrap();
    }

    #[tokio::test]
    async fn test_update_device_same_name_skips_shelf_rename() {
        let device = fake_device(1);
        let token = device.token;
        let updated_device = device.clone();

        let shelf_repo = MockShelfRepository::default(); // no find_by_device_id set → would panic if called

        let svc = create_service(
            MockDeviceRepository::default()
                .with_find_by_token(Ok(Some(device)))
                .with_update_device(Ok(updated_device)),
            shelf_repo,
            MockUserRepository::default(),
        );

        // Same name as current — shelf rename should not be attempted
        let result = svc.update_device(&token, "My Device".to_string(), OnRemovalAction::MarkRead, 1).await;

        result.unwrap();
    }

    #[tokio::test]
    async fn test_update_device_wrong_owner_returns_not_found() {
        let device = fake_device(1);
        let token = device.token;
        let svc = create_service(
            MockDeviceRepository::default().with_find_by_token(Ok(Some(device))),
            MockShelfRepository::default(),
            MockUserRepository::default(),
        );

        let result = svc.update_device(&token, "New Name".to_string(), OnRemovalAction::Nothing, 2).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::NotFound))));
    }

    // ─── delete_device ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_delete_device_with_companion_shelf_deletion() {
        let device = fake_device(1);
        let token = device.token;
        let shelf = fake_shelf(1, Some(device.id));

        let shelf_repo = MockShelfRepository::default().with_find_by_device_id(Ok(Some(shelf)));
        let device_repo = MockDeviceRepository::default().with_find_by_token(Ok(Some(device)));

        let svc = create_service(device_repo, shelf_repo, MockUserRepository::default());

        let result = svc.delete_device(&token, true, 1).await;

        result.unwrap();
    }

    #[tokio::test]
    async fn test_delete_device_without_shelf_deletion() {
        let device = fake_device(1);
        let token = device.token;
        let svc = create_service(
            MockDeviceRepository::default().with_find_by_token(Ok(Some(device))),
            MockShelfRepository::default(),
            MockUserRepository::default(),
        );

        let result = svc.delete_device(&token, false, 1).await;

        result.unwrap();
    }

    #[tokio::test]
    async fn test_delete_device_wrong_owner_returns_not_found() {
        let device = fake_device(1);
        let token = device.token;
        let svc = create_service(
            MockDeviceRepository::default().with_find_by_token(Ok(Some(device))),
            MockShelfRepository::default(),
            MockUserRepository::default(),
        );

        let result = svc.delete_device(&token, false, 2).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::NotFound))));
    }

    // ─── default_device_name ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_default_device_name_no_collision() {
        let user = fake_user(1, "Alice Smith");
        let svc = create_service(
            MockDeviceRepository::default().with_count_with_name_prefix(Ok(0)),
            MockShelfRepository::default(),
            MockUserRepository::default().with_find_by_id(Ok(Some(user))),
        );

        let name = svc.default_device_name(1).await.unwrap();

        assert_eq!(name, "Alice's Device");
    }

    #[tokio::test]
    async fn test_default_device_name_with_collision() {
        let user = fake_user(1, "Alice Smith");
        let svc = create_service(
            MockDeviceRepository::default().with_count_with_name_prefix(Ok(1)),
            MockShelfRepository::default(),
            MockUserRepository::default().with_find_by_id(Ok(Some(user))),
        );

        let name = svc.default_device_name(1).await.unwrap();

        assert_eq!(name, "Alice's Device (2)");
    }

    #[tokio::test]
    async fn test_default_device_name_empty_full_name_falls_back_to_my() {
        let user = fake_user(1, "");
        let svc = create_service(
            MockDeviceRepository::default().with_count_with_name_prefix(Ok(0)),
            MockShelfRepository::default(),
            MockUserRepository::default().with_find_by_id(Ok(Some(user))),
        );

        let name = svc.default_device_name(1).await.unwrap();

        assert_eq!(name, "My's Device");
    }

    // ─── compute_sync_diff ────────────────────────────────────────────────────

    fn sync_shelf() -> Shelf {
        fake_shelf(1, Some(1))
    }

    #[tokio::test]
    async fn test_sync_diff_empty_shelf_returns_empty_diff() {
        let svc = create_sync_service(
            MockDeviceRepository::default().with_books_for_device(Ok(vec![])),
            MockShelfRepository::default()
                .with_find_by_device_id(Ok(Some(sync_shelf())))
                .with_books_for_filter(Ok(vec![])),
            MockBookRepository::default(),
        );

        let diff = svc.compute_sync_diff(1, 1, None, None, 100).await.unwrap();

        assert!(diff.is_empty());
        assert!(!diff.has_more);
    }

    #[tokio::test]
    async fn test_sync_diff_no_companion_shelf_returns_empty() {
        let svc = create_sync_service(
            MockDeviceRepository::default(),
            MockShelfRepository::default().with_find_by_device_id(Ok(None)),
            MockBookRepository::default(),
        );

        let diff = svc.compute_sync_diff(1, 1, None, None, 100).await.unwrap();

        assert!(diff.is_empty());
    }

    #[tokio::test]
    async fn test_sync_diff_book_without_files_is_skipped() {
        let book = Book::fake(1, "No Files", BookStatus::Available);
        let svc = create_sync_service(
            MockDeviceRepository::default().with_books_for_device(Ok(vec![])),
            MockShelfRepository::default()
                .with_find_by_device_id(Ok(Some(sync_shelf())))
                .with_books_for_filter(Ok(vec![book])),
            MockBookRepository::default(), // no files configured → returns empty
        );

        let diff = svc.compute_sync_diff(1, 1, None, None, 100).await.unwrap();

        assert!(diff.is_empty());
    }

    #[tokio::test]
    async fn test_sync_diff_new_books_sorted_by_id() {
        // Shelf returns books out of order; diff must be sorted by book_id
        let books = vec![
            Book::fake(3, "C", BookStatus::Available),
            Book::fake(1, "A", BookStatus::Available),
            Book::fake(2, "B", BookStatus::Available),
        ];
        let svc = create_sync_service(
            MockDeviceRepository::default().with_books_for_device(Ok(vec![])),
            MockShelfRepository::default()
                .with_find_by_device_id(Ok(Some(sync_shelf())))
                .with_books_for_filter(Ok(books)),
            MockBookRepository::default()
                .with_files(1, vec![fake_book_file(1, FileFormat::Epub, FileRole::Original)])
                .with_files(2, vec![fake_book_file(2, FileFormat::Epub, FileRole::Original)])
                .with_files(3, vec![fake_book_file(3, FileFormat::Epub, FileRole::Original)]),
        );

        let diff = svc.compute_sync_diff(1, 1, None, None, 100).await.unwrap();

        assert_eq!(diff.new_books.len(), 3);
        assert_eq!(diff.new_books[0].book.id, 1);
        assert_eq!(diff.new_books[1].book.id, 2);
        assert_eq!(diff.new_books[2].book.id, 3);
        assert!(diff.upgraded_books.is_empty());
        assert!(diff.refreshed_books.is_empty());
    }

    #[tokio::test]
    async fn test_sync_diff_best_file_selection_prefers_enriched_kepub() {
        let book = Book::fake(1, "Book", BookStatus::Available);
        let svc = create_sync_service(
            MockDeviceRepository::default().with_books_for_device(Ok(vec![])),
            MockShelfRepository::default()
                .with_find_by_device_id(Ok(Some(sync_shelf())))
                .with_books_for_filter(Ok(vec![book])),
            MockBookRepository::default().with_files(
                1,
                vec![
                    fake_book_file(1, FileFormat::Epub, FileRole::Original),
                    fake_book_file(1, FileFormat::Epub, FileRole::Enriched),
                    fake_book_file(1, FileFormat::Kepub, FileRole::Enriched),
                ],
            ),
        );

        let diff = svc.compute_sync_diff(1, 1, None, None, 100).await.unwrap();

        assert_eq!(diff.new_books.len(), 1);
        assert_eq!(diff.new_books[0].file.format, FileFormat::Kepub);
        assert_eq!(diff.new_books[0].file.file_role, FileRole::Enriched);
    }

    #[tokio::test]
    async fn test_sync_diff_upgraded_book_when_better_file_available() {
        let book = Book::fake(1, "Book", BookStatus::Available);
        // Device has Original Epub; now Enriched Epub is available → upgraded
        let device_book = fake_device_book(1, 1, FileFormat::Epub, FileRole::Original);
        let svc = create_sync_service(
            MockDeviceRepository::default().with_books_for_device(Ok(vec![device_book])),
            MockShelfRepository::default()
                .with_find_by_device_id(Ok(Some(sync_shelf())))
                .with_books_for_filter(Ok(vec![book])),
            MockBookRepository::default().with_files(
                1,
                vec![
                    fake_book_file(1, FileFormat::Epub, FileRole::Original),
                    fake_book_file(1, FileFormat::Epub, FileRole::Enriched),
                ],
            ),
        );

        let diff = svc.compute_sync_diff(1, 1, None, None, 100).await.unwrap();

        assert!(diff.new_books.is_empty());
        assert_eq!(diff.upgraded_books.len(), 1);
        assert_eq!(diff.upgraded_books[0].file.file_role, FileRole::Enriched);
    }

    #[tokio::test]
    async fn test_sync_diff_refreshed_book_when_metadata_updated() {
        let updated_at = DateTime::from_timestamp(1000, 0).unwrap();
        let since = DateTime::from_timestamp(500, 0).unwrap();
        let book = Book {
            updated_at,
            ..Book::fake(1, "Book", BookStatus::Available)
        };
        // Device has same file, but book.updated_at > since → refreshed
        let device_book = fake_device_book(1, 1, FileFormat::Epub, FileRole::Original);
        let svc = create_sync_service(
            MockDeviceRepository::default().with_books_for_device(Ok(vec![device_book])),
            MockShelfRepository::default()
                .with_find_by_device_id(Ok(Some(sync_shelf())))
                .with_books_for_filter(Ok(vec![book])),
            MockBookRepository::default().with_files(1, vec![fake_book_file(1, FileFormat::Epub, FileRole::Original)]),
        );

        let diff = svc.compute_sync_diff(1, 1, Some(since), None, 100).await.unwrap();

        assert!(diff.new_books.is_empty());
        assert!(diff.upgraded_books.is_empty());
        assert_eq!(diff.refreshed_books.len(), 1);
    }

    #[tokio::test]
    async fn test_sync_diff_unchanged_book_is_skipped() {
        let updated_at = DateTime::from_timestamp(500, 0).unwrap();
        let since = DateTime::from_timestamp(1000, 0).unwrap();
        let book = Book {
            updated_at,
            ..Book::fake(1, "Book", BookStatus::Available)
        };
        let device_book = fake_device_book(1, 1, FileFormat::Epub, FileRole::Original);
        let svc = create_sync_service(
            MockDeviceRepository::default().with_books_for_device(Ok(vec![device_book])),
            MockShelfRepository::default()
                .with_find_by_device_id(Ok(Some(sync_shelf())))
                .with_books_for_filter(Ok(vec![book])),
            MockBookRepository::default().with_files(1, vec![fake_book_file(1, FileFormat::Epub, FileRole::Original)]),
        );

        let diff = svc.compute_sync_diff(1, 1, Some(since), None, 100).await.unwrap();

        assert!(diff.is_empty());
    }

    #[tokio::test]
    async fn test_sync_diff_removals_only_on_first_page() {
        let book = Book::fake(1, "On Shelf", BookStatus::Available);
        // DeviceBook for book_id=2 is no longer on the shelf → should appear as removal
        let stale_book = fake_device_book(1, 2, FileFormat::Epub, FileRole::Original);
        let svc = create_sync_service(
            MockDeviceRepository::default().with_books_for_device(Ok(vec![stale_book.clone()])),
            MockShelfRepository::default()
                .with_find_by_device_id(Ok(Some(sync_shelf())))
                .with_books_for_filter(Ok(vec![book])),
            MockBookRepository::default().with_files(1, vec![fake_book_file(1, FileFormat::Epub, FileRole::Original)]),
        );

        // First page (after_book_id = None): removals included
        let diff_first = svc.compute_sync_diff(1, 1, None, None, 100).await.unwrap();
        assert_eq!(diff_first.removed_book_ids, vec![2]);

        // Subsequent page (after_book_id = Some(...)): no removals
        let diff_second = svc.compute_sync_diff(1, 1, None, Some(0), 100).await.unwrap();
        assert!(diff_second.removed_book_ids.is_empty());
    }

    #[tokio::test]
    async fn test_sync_diff_paging_150_books_has_more_on_first_page() {
        // 150 books (ids 1-150), all new, page_size=100
        let books: Vec<Book> = (1u64..=150).map(|i| Book::fake(i, format!("Book {i}"), BookStatus::Available)).collect();
        let mut book_repo = MockBookRepository::default();
        for i in 1u64..=150 {
            book_repo = book_repo.with_files(i, vec![fake_book_file(i, FileFormat::Epub, FileRole::Original)]);
        }
        let svc = create_sync_service(
            MockDeviceRepository::default().with_books_for_device(Ok(vec![])),
            MockShelfRepository::default()
                .with_find_by_device_id(Ok(Some(sync_shelf())))
                .with_books_for_filter(Ok(books)),
            book_repo,
        );

        // First page
        let diff = svc.compute_sync_diff(1, 1, None, None, 100).await.unwrap();
        assert_eq!(diff.new_books.len(), 100);
        assert!(diff.has_more);
        assert_eq!(diff.new_books.first().unwrap().book.id, 1);
        assert_eq!(diff.new_books.last().unwrap().book.id, 100);

        // Second page
        let diff2 = svc.compute_sync_diff(1, 1, None, Some(100), 100).await.unwrap();
        assert_eq!(diff2.new_books.len(), 50);
        assert!(!diff2.has_more);
        assert_eq!(diff2.new_books.first().unwrap().book.id, 101);
        assert_eq!(diff2.new_books.last().unwrap().book.id, 150);
    }
}
