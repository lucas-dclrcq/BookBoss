use bb_core::{
    Error,
    book::{BookId, FileFormat},
    device::{Device, DeviceBook, DeviceId, DeviceRepository, DeviceSyncLog, DeviceToken, NewDevice, NewDeviceSyncLog, OnRemovalAction, SyncStatus},
    repository::Transaction,
    user::UserId,
};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, ModelTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::{
    entities::{device_books, device_sync_log, devices, prelude},
    error::handle_dberr,
    transaction::TransactionImpl,
};

// ── String conversions
// ────────────────────────────────────────────────────────

fn file_format_to_str(f: &FileFormat) -> &'static str {
    match f {
        FileFormat::Epub => "epub",
        FileFormat::Kepub => "kepub",
        FileFormat::Mobi => "mobi",
        FileFormat::Azw3 => "azw3",
        FileFormat::Pdf => "pdf",
        FileFormat::Cbz => "cbz",
    }
}

fn str_to_file_format(s: &str) -> FileFormat {
    match s {
        "mobi" => FileFormat::Mobi,
        "azw3" => FileFormat::Azw3,
        "pdf" => FileFormat::Pdf,
        "cbz" => FileFormat::Cbz,
        _ => FileFormat::Epub,
    }
}

fn removal_action_to_str(a: &OnRemovalAction) -> &'static str {
    match a {
        OnRemovalAction::MarkRead => "mark_read",
        OnRemovalAction::MarkDnf => "mark_dnf",
        OnRemovalAction::Nothing => "nothing",
    }
}

fn str_to_removal_action(s: &str) -> OnRemovalAction {
    match s {
        "mark_read" => OnRemovalAction::MarkRead,
        "mark_dnf" => OnRemovalAction::MarkDnf,
        _ => OnRemovalAction::Nothing,
    }
}

fn sync_status_to_str(s: &SyncStatus) -> &'static str {
    match s {
        SyncStatus::Running => "running",
        SyncStatus::Completed => "completed",
        SyncStatus::Failed => "failed",
    }
}

fn str_to_sync_status(s: &str) -> SyncStatus {
    match s {
        "completed" => SyncStatus::Completed,
        "failed" => SyncStatus::Failed,
        _ => SyncStatus::Running,
    }
}

// ── From impls
// ────────────────────────────────────────────────────────────────

impl From<devices::Model> for Device {
    fn from(m: devices::Model) -> Self {
        let token = DeviceToken::new(m.id as u64);
        Self {
            id: m.id as u64,
            version: m.version as u64,
            token,
            owner_id: m.owner_id as u64,
            name: m.name,
            device_type: m.device_type,
            preferred_format: m.preferred_format.as_deref().map(str_to_file_format),
            on_removal_action: str_to_removal_action(&m.on_removal_action),
            last_synced_at: m.last_synced_at.map(|t| t.with_timezone(&Utc)),
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
        }
    }
}

impl From<device_books::Model> for DeviceBook {
    fn from(m: device_books::Model) -> Self {
        Self {
            device_id: m.device_id as u64,
            book_id: m.book_id as u64,
            format: str_to_file_format(&m.format),
            synced_at: m.synced_at.with_timezone(&Utc),
            removed_at: m.removed_at.map(|t| t.with_timezone(&Utc)),
        }
    }
}

impl From<device_sync_log::Model> for DeviceSyncLog {
    fn from(m: device_sync_log::Model) -> Self {
        Self {
            id: m.id,
            device_id: m.device_id as u64,
            status: str_to_sync_status(&m.status),
            books_added: m.books_added,
            books_removed: m.books_removed,
            started_at: m.started_at.with_timezone(&Utc),
            completed_at: m.completed_at.map(|t| t.with_timezone(&Utc)),
        }
    }
}

// ── Adapter
// ───────────────────────────────────────────────────────────────────

pub(crate) struct DeviceRepositoryAdapter;

impl DeviceRepositoryAdapter {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl DeviceRepository for DeviceRepositoryAdapter {
    async fn add_device(&self, transaction: &dyn Transaction, device: NewDevice) -> Result<Device, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let token = DeviceToken::generate();
        let now = Utc::now();

        let model = devices::ActiveModel {
            id: Set(token.id() as i64),
            token: Set(token.to_string()),
            owner_id: Set(device.owner_id as i64),
            name: Set(device.name),
            device_type: Set(device.device_type),
            preferred_format: Set(device.preferred_format.as_ref().map(|f| file_format_to_str(f).to_owned())),
            on_removal_action: Set(removal_action_to_str(&device.on_removal_action).to_owned()),
            last_synced_at: Set(None),
            version: Set(0),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
        }
        .insert(transaction)
        .await
        .map_err(handle_dberr)?;

        Ok(model.into())
    }

    async fn update_device(&self, transaction: &dyn Transaction, device: Device) -> Result<Device, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let existing = prelude::Devices::find_by_id(device.id as i64)
            .one(transaction)
            .await
            .map_err(handle_dberr)?
            .ok_or(bb_core::RepositoryError::NotFound)?;

        let mut updater: devices::ActiveModel = existing.into();
        updater.name = Set(device.name);
        updater.device_type = Set(device.device_type);
        updater.preferred_format = Set(device.preferred_format.as_ref().map(|f| file_format_to_str(f).to_owned()));
        updater.on_removal_action = Set(removal_action_to_str(&device.on_removal_action).to_owned());
        updater.last_synced_at = Set(device.last_synced_at.map(Into::into));

        let result = updater.update(transaction).await.map_err(handle_dberr)?;
        Ok(result.into())
    }

    async fn delete_device(&self, transaction: &dyn Transaction, device: Device) -> Result<(), Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        if let Some(existing) = prelude::Devices::find_by_id(device.id as i64).one(transaction).await.map_err(handle_dberr)? {
            existing.delete(transaction).await.map_err(handle_dberr)?;
        }

        Ok(())
    }

    async fn find_by_id(&self, transaction: &dyn Transaction, id: DeviceId) -> Result<Option<Device>, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        Ok(prelude::Devices::find_by_id(id as i64)
            .one(transaction)
            .await
            .map_err(handle_dberr)?
            .map(Into::into))
    }

    async fn find_by_token(&self, transaction: &dyn Transaction, token: &DeviceToken) -> Result<Option<Device>, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        Ok(prelude::Devices::find()
            .filter(devices::Column::Token.eq(token.to_string()))
            .one(transaction)
            .await
            .map_err(handle_dberr)?
            .map(Into::into))
    }

    async fn list_for_user(&self, transaction: &dyn Transaction, owner_id: UserId) -> Result<Vec<Device>, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        Ok(prelude::Devices::find()
            .filter(devices::Column::OwnerId.eq(owner_id as i64))
            .order_by_asc(devices::Column::CreatedAt)
            .all(transaction)
            .await
            .map_err(handle_dberr)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn count_with_name_prefix(&self, transaction: &dyn Transaction, owner_id: UserId, prefix: &str) -> Result<u64, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        Ok(prelude::Devices::find()
            .filter(devices::Column::OwnerId.eq(owner_id as i64))
            .filter(devices::Column::Name.like(format!("{prefix}%")))
            .count(transaction)
            .await
            .map_err(handle_dberr)?)
    }

    async fn add_device_book(&self, transaction: &dyn Transaction, book: DeviceBook) -> Result<DeviceBook, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let now = Utc::now();
        let model = device_books::ActiveModel {
            device_id: Set(book.device_id as i64),
            book_id: Set(book.book_id as i64),
            format: Set(file_format_to_str(&book.format).to_owned()),
            synced_at: Set(now.into()),
            removed_at: Set(None),
        }
        .insert(transaction)
        .await
        .map_err(handle_dberr)?;

        Ok(model.into())
    }

    async fn remove_device_book(&self, transaction: &dyn Transaction, device_id: DeviceId, book_id: BookId) -> Result<(), Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        prelude::DeviceBooks::delete_many()
            .filter(device_books::Column::DeviceId.eq(device_id as i64))
            .filter(device_books::Column::BookId.eq(book_id as i64))
            .exec(transaction)
            .await
            .map_err(handle_dberr)?;

        Ok(())
    }

    async fn books_for_device(&self, transaction: &dyn Transaction, device_id: DeviceId) -> Result<Vec<DeviceBook>, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        Ok(prelude::DeviceBooks::find()
            .filter(device_books::Column::DeviceId.eq(device_id as i64))
            .all(transaction)
            .await
            .map_err(handle_dberr)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    async fn add_sync_log(&self, transaction: &dyn Transaction, log: NewDeviceSyncLog) -> Result<DeviceSyncLog, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let model = device_sync_log::ActiveModel {
            device_id: Set(log.device_id as i64),
            status: Set(sync_status_to_str(&log.status).to_owned()),
            books_added: Set(log.books_added),
            books_removed: Set(log.books_removed),
            started_at: Set(log.started_at.into()),
            completed_at: Set(log.completed_at.map(Into::into)),
            ..Default::default()
        }
        .insert(transaction)
        .await
        .map_err(handle_dberr)?;

        Ok(model.into())
    }

    async fn list_sync_logs_for_device(&self, transaction: &dyn Transaction, device_id: DeviceId, page_size: Option<u64>) -> Result<Vec<DeviceSyncLog>, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let mut query = prelude::DeviceSyncLogs::find()
            .filter(device_sync_log::Column::DeviceId.eq(device_id as i64))
            .order_by_desc(device_sync_log::Column::StartedAt);

        if let Some(limit) = page_size {
            query = query.limit(limit);
        }

        Ok(query.all(transaction).await.map_err(handle_dberr)?.into_iter().map(Into::into).collect())
    }
}
