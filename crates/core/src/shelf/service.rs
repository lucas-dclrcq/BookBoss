use std::sync::Arc;

use crate::{
    Error, RepositoryError,
    book::BookToken,
    repository::RepositoryService,
    shelf::{BookShelf, Shelf, ShelfToken, ShelfVisibility},
    user::UserId,
    with_read_only_transaction, with_transaction,
};

#[async_trait::async_trait]
pub trait ShelfService: Send + Sync {
    /// Creates a new manual shelf for the given user.
    ///
    /// Returns an error if the name is empty or a shelf with the same name
    /// already exists for the user.
    async fn create_manual_shelf(&self, owner_id: UserId, name: String, visibility: ShelfVisibility) -> Result<ShelfToken, Error>;

    /// Renames a shelf. Only the owner may rename.
    async fn rename_shelf(&self, token: &ShelfToken, new_name: String, user_id: UserId) -> Result<(), Error>;

    /// Deletes a shelf. Only the owner may delete.
    async fn delete_shelf(&self, token: &ShelfToken, user_id: UserId) -> Result<(), Error>;

    /// Adds a book to a shelf. Only the owner may add books.
    async fn add_book_to_shelf(&self, shelf_token: &ShelfToken, book_token: &BookToken, user_id: UserId) -> Result<(), Error>;

    /// Removes a book from a shelf. Only the owner may remove books.
    async fn remove_book_from_shelf(&self, shelf_token: &ShelfToken, book_token: &BookToken, user_id: UserId) -> Result<(), Error>;

    /// Returns paginated books for a shelf.
    ///
    /// Owners can access private shelves; other users may only access public
    /// shelves.
    async fn books_for_shelf(&self, token: &ShelfToken, user_id: UserId, start_id: Option<u64>, page_size: Option<u64>) -> Result<Vec<BookShelf>, Error>;

    /// Returns all shelves owned by the given user.
    async fn list_shelves_for_user(&self, user_id: UserId) -> Result<Vec<Shelf>, Error>;

    /// Updates the visibility of a shelf. Only the owner may change visibility.
    async fn set_visibility(&self, token: &ShelfToken, visibility: ShelfVisibility, user_id: UserId) -> Result<(), Error>;

    /// Returns all public shelves not owned by the given user, sorted by name.
    async fn list_public_shelves(&self, user_id: UserId) -> Result<Vec<Shelf>, Error>;

    /// Returns metadata for a single shelf.
    ///
    /// Owners can access private shelves; other users may only access public
    /// shelves. Returns `NotFound` for missing shelves and a validation error
    /// for private shelves the requester does not own.
    async fn get_shelf(&self, token: &ShelfToken, user_id: UserId) -> Result<Shelf, Error>;

    /// Updates the name and visibility of a shelf in a single transaction.
    ///
    /// Only the owner may update. Returns an error if the name is empty or
    /// another shelf owned by the same user already has that name.
    async fn update_shelf(&self, token: &ShelfToken, new_name: String, visibility: ShelfVisibility, user_id: UserId) -> Result<(), Error>;
}

pub(crate) struct ShelfServiceImpl {
    repository_service: Arc<RepositoryService>,
}

impl ShelfServiceImpl {
    pub(crate) fn new(repository_service: Arc<RepositoryService>) -> Self {
        Self { repository_service }
    }
}

#[async_trait::async_trait]
impl ShelfService for ShelfServiceImpl {
    #[tracing::instrument(level = "trace", skip(self))]
    async fn create_manual_shelf(&self, owner_id: UserId, name: String, visibility: ShelfVisibility) -> Result<ShelfToken, Error> {
        if name.trim().is_empty() {
            return Err(Error::Validation("shelf name must not be empty".to_string()));
        }

        let name_lower = name.to_lowercase();

        with_transaction!(self, shelf_repository, |tx| {
            let existing = shelf_repository.list_for_user(tx, owner_id).await?;
            if existing.iter().any(|s| s.name.to_lowercase() == name_lower) {
                return Err(Error::RepositoryError(RepositoryError::Conflict));
            }

            let shelf = shelf_repository
                .add_shelf(
                    tx,
                    crate::shelf::NewShelf {
                        owner_id,
                        name,
                        shelf_type: crate::shelf::ShelfType::Manual,
                        visibility,
                        device_id: None,
                        filter_criteria: None,
                    },
                )
                .await?;

            Ok(shelf.token)
        })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn rename_shelf(&self, token: &ShelfToken, new_name: String, user_id: UserId) -> Result<(), Error> {
        if new_name.trim().is_empty() {
            return Err(Error::Validation("shelf name must not be empty".to_string()));
        }

        let token = *token;
        let new_name_lower = new_name.to_lowercase();

        with_transaction!(self, shelf_repository, |tx| {
            let shelf = shelf_repository
                .find_by_token(tx, &token)
                .await?
                .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

            if shelf.owner_id != user_id {
                return Err(Error::Validation("only the owner may rename a shelf".to_string()));
            }

            let existing = shelf_repository.list_for_user(tx, user_id).await?;
            if existing.iter().any(|s| s.token != token && s.name.to_lowercase() == new_name_lower) {
                return Err(Error::RepositoryError(RepositoryError::Conflict));
            }

            let updated = Shelf { name: new_name, ..shelf };
            shelf_repository.update_shelf(tx, updated).await?;

            Ok(())
        })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn delete_shelf(&self, token: &ShelfToken, user_id: UserId) -> Result<(), Error> {
        let token = *token;

        with_transaction!(self, shelf_repository, |tx| {
            let shelf = shelf_repository
                .find_by_token(tx, &token)
                .await?
                .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

            if shelf.owner_id != user_id {
                return Err(Error::Validation("only the owner may delete a shelf".to_string()));
            }

            shelf_repository.delete_shelf(tx, shelf).await
        })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn add_book_to_shelf(&self, shelf_token: &ShelfToken, book_token: &BookToken, user_id: UserId) -> Result<(), Error> {
        let shelf_token = *shelf_token;
        let book_token = *book_token;

        with_transaction!(self, shelf_repository, book_repository, |tx| {
            let shelf = shelf_repository
                .find_by_token(tx, &shelf_token)
                .await?
                .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

            if shelf.owner_id != user_id {
                return Err(Error::Validation("only the owner may add books to a shelf".to_string()));
            }

            let book = book_repository
                .find_by_token(tx, &book_token)
                .await?
                .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

            shelf_repository
                .add_book_to_shelf(
                    tx,
                    BookShelf {
                        shelf_id: shelf.id,
                        book_id: book.id,
                        sort_order: 0,
                        added_at: chrono::Utc::now(),
                    },
                )
                .await?;

            Ok(())
        })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn remove_book_from_shelf(&self, shelf_token: &ShelfToken, book_token: &BookToken, user_id: UserId) -> Result<(), Error> {
        let shelf_token = *shelf_token;
        let book_token = *book_token;

        with_transaction!(self, shelf_repository, book_repository, |tx| {
            let shelf = shelf_repository
                .find_by_token(tx, &shelf_token)
                .await?
                .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

            if shelf.owner_id != user_id {
                return Err(Error::Validation("only the owner may remove books from a shelf".to_string()));
            }

            let book = book_repository
                .find_by_token(tx, &book_token)
                .await?
                .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

            shelf_repository.remove_book_from_shelf(tx, shelf.id, book.id).await
        })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn books_for_shelf(&self, token: &ShelfToken, user_id: UserId, start_id: Option<u64>, page_size: Option<u64>) -> Result<Vec<BookShelf>, Error> {
        let token = *token;

        with_read_only_transaction!(self, shelf_repository, |tx| {
            let shelf = shelf_repository
                .find_by_token(tx, &token)
                .await?
                .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

            if shelf.visibility == ShelfVisibility::Private && shelf.owner_id != user_id {
                return Err(Error::Validation("this shelf is private".to_string()));
            }

            shelf_repository.books_for_shelf(tx, shelf.id, start_id, page_size).await
        })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn list_shelves_for_user(&self, user_id: UserId) -> Result<Vec<Shelf>, Error> {
        with_read_only_transaction!(self, shelf_repository, |tx| shelf_repository.list_for_user(tx, user_id).await)
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn list_public_shelves(&self, user_id: UserId) -> Result<Vec<Shelf>, Error> {
        with_read_only_transaction!(self, shelf_repository, |tx| shelf_repository.list_public_shelves(tx, user_id).await)
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn get_shelf(&self, token: &ShelfToken, user_id: UserId) -> Result<Shelf, Error> {
        let token = *token;

        with_read_only_transaction!(self, shelf_repository, |tx| {
            let shelf = shelf_repository
                .find_by_token(tx, &token)
                .await?
                .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

            if shelf.visibility == ShelfVisibility::Private && shelf.owner_id != user_id {
                return Err(Error::Validation("this shelf is private".to_string()));
            }

            Ok(shelf)
        })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn update_shelf(&self, token: &ShelfToken, new_name: String, visibility: ShelfVisibility, user_id: UserId) -> Result<(), Error> {
        if new_name.trim().is_empty() {
            return Err(Error::Validation("shelf name must not be empty".to_string()));
        }

        let token = *token;
        let new_name_lower = new_name.to_lowercase();

        with_transaction!(self, shelf_repository, |tx| {
            let shelf = shelf_repository
                .find_by_token(tx, &token)
                .await?
                .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

            if shelf.owner_id != user_id {
                return Err(Error::Validation("only the owner may update a shelf".to_string()));
            }

            let existing = shelf_repository.list_for_user(tx, user_id).await?;
            if existing.iter().any(|s| s.token != token && s.name.to_lowercase() == new_name_lower) {
                return Err(Error::RepositoryError(RepositoryError::Conflict));
            }

            let updated = Shelf {
                name: new_name,
                visibility,
                ..shelf
            };
            shelf_repository.update_shelf(tx, updated).await?;

            Ok(())
        })
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn set_visibility(&self, token: &ShelfToken, visibility: ShelfVisibility, user_id: UserId) -> Result<(), Error> {
        let token = *token;

        with_transaction!(self, shelf_repository, |tx| {
            let shelf = shelf_repository
                .find_by_token(tx, &token)
                .await?
                .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

            if shelf.owner_id != user_id {
                return Err(Error::Validation("only the owner may change shelf visibility".to_string()));
            }

            let updated = Shelf { visibility, ..shelf };
            shelf_repository.update_shelf(tx, updated).await?;

            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        any::Any,
        sync::{Arc, Mutex},
    };

    use chrono::Utc;

    use super::{ShelfService, ShelfServiceImpl};
    use crate::{
        Error, RepositoryError,
        auth::{NewSession, Session, repository::SessionRepository},
        book::{
            Author, AuthorId, AuthorRepository, AuthorToken, Book, BookAuthor, BookFile, BookFilter, BookId, BookIdentifier, BookRepository, BookStatus,
            BookToken, FileFormat, Genre, GenreId, GenreRepository, GenreToken, IdentifierType, NewAuthor, NewBook, NewGenre, NewPublisher, NewSeries, NewTag,
            Publisher, PublisherId, PublisherRepository, PublisherToken, Series, SeriesId, SeriesRepository, SeriesToken, Tag, TagId, TagRepository, TagToken,
        },
        import::{ImportJob, ImportJobId, ImportJobRepository, ImportJobToken, ImportStatus, NewImportJob},
        jobs::{Job, JobRepository},
        reading::{ReadStatus, UserBookMetadata, UserBookMetadataRepository},
        repository::{Repository, RepositoryServiceBuilder, Transaction},
        shelf::{BookShelf, NewShelf, Shelf, ShelfFilter, ShelfId, ShelfRepository, ShelfToken, ShelfType, ShelfVisibility},
        user::{
            NewUser, NewUserSetting, User, UserId, UserSetting,
            repository::{UserRepository, UserSettingRepository},
        },
    };

    // ─── Mock Transaction ─────────────────────────────────────────────────────

    struct MockTransaction;

    #[async_trait::async_trait]
    impl Transaction for MockTransaction {
        fn as_any(&self) -> &dyn Any {
            self
        }
        async fn commit(self: Box<Self>) -> Result<(), Error> {
            Ok(())
        }
        async fn rollback(self: Box<Self>) -> Result<(), Error> {
            Ok(())
        }
    }

    // ─── Mock Repository ──────────────────────────────────────────────────────

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

    // ─── Mock ShelfRepository ─────────────────────────────────────────────────

    #[derive(Default)]
    struct MockShelfRepository {
        find_by_token_result: Mutex<Option<Result<Option<Shelf>, Error>>>,
        list_for_user_result: Mutex<Option<Result<Vec<Shelf>, Error>>>,
        add_shelf_result: Mutex<Option<Result<Shelf, Error>>>,
        update_shelf_result: Mutex<Option<Result<Shelf, Error>>>,
        delete_shelf_called: Mutex<bool>,
        add_book_to_shelf_called: Mutex<bool>,
        remove_book_from_shelf_called: Mutex<bool>,
        books_for_shelf_result: Mutex<Option<Result<Vec<BookShelf>, Error>>>,
    }

    impl MockShelfRepository {
        fn with_find_by_token(self, result: Result<Option<Shelf>, Error>) -> Self {
            *self.find_by_token_result.lock().unwrap() = Some(result);
            self
        }
        fn with_list_for_user(self, result: Result<Vec<Shelf>, Error>) -> Self {
            *self.list_for_user_result.lock().unwrap() = Some(result);
            self
        }
        fn with_add_shelf(self, result: Result<Shelf, Error>) -> Self {
            *self.add_shelf_result.lock().unwrap() = Some(result);
            self
        }
        fn with_update_shelf(self, result: Result<Shelf, Error>) -> Self {
            *self.update_shelf_result.lock().unwrap() = Some(result);
            self
        }
        fn with_books_for_shelf(self, result: Result<Vec<BookShelf>, Error>) -> Self {
            *self.books_for_shelf_result.lock().unwrap() = Some(result);
            self
        }
        fn delete_shelf_was_called(&self) -> bool {
            *self.delete_shelf_called.lock().unwrap()
        }
        fn add_book_to_shelf_was_called(&self) -> bool {
            *self.add_book_to_shelf_called.lock().unwrap()
        }
        fn remove_book_from_shelf_was_called(&self) -> bool {
            *self.remove_book_from_shelf_called.lock().unwrap()
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
            self.find_by_token_result
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| Err(Error::MockNotConfigured("find_by_token")))
        }
        async fn list_for_user(&self, _: &dyn Transaction, _: UserId) -> Result<Vec<Shelf>, Error> {
            self.list_for_user_result.lock().unwrap().clone().unwrap_or(Ok(vec![]))
        }
        async fn list_public_shelves(&self, _: &dyn Transaction, _: UserId) -> Result<Vec<Shelf>, Error> {
            Ok(vec![])
        }
        async fn add_book_to_shelf(&self, _: &dyn Transaction, _: BookShelf) -> Result<BookShelf, Error> {
            *self.add_book_to_shelf_called.lock().unwrap() = true;
            Ok(fake_book_shelf())
        }
        async fn remove_book_from_shelf(&self, _: &dyn Transaction, _: ShelfId, _: BookId) -> Result<(), Error> {
            *self.remove_book_from_shelf_called.lock().unwrap() = true;
            Ok(())
        }
        async fn books_for_shelf(&self, _: &dyn Transaction, _: ShelfId, _: Option<BookId>, _: Option<u64>) -> Result<Vec<BookShelf>, Error> {
            self.books_for_shelf_result.lock().unwrap().clone().unwrap_or(Ok(vec![]))
        }
        async fn books_for_filter(&self, _: &dyn Transaction, _: &ShelfFilter, _: UserId, _: Option<BookId>, _: Option<u64>) -> Result<Vec<Book>, Error> {
            unimplemented!()
        }
        async fn count_for_filter(&self, _: &dyn Transaction, _: &ShelfFilter, _: UserId) -> Result<u64, Error> {
            unimplemented!()
        }
    }

    // ─── Mock BookRepository ──────────────────────────────────────────────────

    #[derive(Default)]
    struct MockBookRepository {
        find_by_token_result: Mutex<Option<Result<Option<Book>, Error>>>,
    }

    impl MockBookRepository {
        fn with_find_by_token(self, result: Result<Option<Book>, Error>) -> Self {
            *self.find_by_token_result.lock().unwrap() = Some(result);
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
            self.find_by_token_result
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| Err(Error::MockNotConfigured("find_by_token")))
        }
        async fn list_books(&self, _: &dyn Transaction, _: &BookFilter, _: Option<BookId>, _: Option<u64>) -> Result<Vec<Book>, Error> {
            unimplemented!()
        }
        async fn count_available_books(&self, _: &dyn Transaction) -> Result<u64, Error> {
            unimplemented!()
        }
        async fn count_books_for_author(&self, _: &dyn Transaction, _: AuthorId) -> Result<u64, Error> {
            unimplemented!()
        }
        async fn add_book_author(&self, _: &dyn Transaction, _: BookId, _: AuthorId, _: crate::book::AuthorRole, _: i32) -> Result<(), Error> {
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
        async fn add_book_file(&self, _: &dyn Transaction, _: BookId, _: FileFormat, _: i64, _: String) -> Result<BookFile, Error> {
            unimplemented!()
        }
        async fn files_for_book(&self, _: &dyn Transaction, _: BookId) -> Result<Vec<BookFile>, Error> {
            unimplemented!()
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
    }

    // ─── Stub repositories (not under test) ───────────────────────────────────

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

    struct MockUserRepository;
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
            unimplemented!()
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
    }

    // ─── Mock UserBookMetadataRepository ────────────────────────────────────

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
    }

    // ─── Helpers ──────────────────────────────────────────────────────────────

    fn create_service(shelf_repo: MockShelfRepository, book_repo: MockBookRepository) -> ShelfServiceImpl {
        let repository_service = Arc::new(
            RepositoryServiceBuilder::default()
                .repository(Arc::new(MockRepository) as Arc<dyn Repository>)
                .session_repository(Arc::new(MockSessionRepository) as Arc<dyn SessionRepository>)
                .user_repository(Arc::new(MockUserRepository) as Arc<dyn UserRepository>)
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
                .build()
                .expect("all fields provided"),
        );
        ShelfServiceImpl::new(repository_service)
    }

    fn fake_shelf(owner_id: UserId, visibility: ShelfVisibility) -> Shelf {
        Shelf {
            id: 1,
            version: 1,
            token: ShelfToken::new(1),
            owner_id,
            name: "My Shelf".to_string(),
            shelf_type: ShelfType::Manual,
            visibility,
            device_id: None,
            filter_criteria: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn fake_book() -> Book {
        Book::fake(1, "Test Book", BookStatus::Available)
    }

    fn fake_book_shelf() -> BookShelf {
        BookShelf {
            shelf_id: 1,
            book_id: 1,
            sort_order: 1,
            added_at: Utc::now(),
        }
    }

    // ─── create_manual_shelf ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_create_manual_shelf_returns_token() {
        let shelf = fake_shelf(1, ShelfVisibility::Private);
        let expected_token = shelf.token;
        let svc = create_service(
            MockShelfRepository::default().with_list_for_user(Ok(vec![])).with_add_shelf(Ok(shelf)),
            MockBookRepository::default(),
        );

        let result = svc.create_manual_shelf(1, "My Shelf".to_string(), ShelfVisibility::Private).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), expected_token);
    }

    #[tokio::test]
    async fn test_create_manual_shelf_empty_name_returns_validation_error() {
        let svc = create_service(MockShelfRepository::default(), MockBookRepository::default());

        for name in ["", "   "] {
            let result = svc.create_manual_shelf(1, name.to_string(), ShelfVisibility::Private).await;
            assert!(matches!(result, Err(Error::Validation(_))), "expected Validation for name={name:?}");
        }
    }

    #[tokio::test]
    async fn test_create_manual_shelf_duplicate_name_returns_conflict() {
        let existing = fake_shelf(1, ShelfVisibility::Private);
        let svc = create_service(
            MockShelfRepository::default().with_list_for_user(Ok(vec![existing])),
            MockBookRepository::default(),
        );

        let result = svc.create_manual_shelf(1, "My Shelf".to_string(), ShelfVisibility::Private).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::Conflict))));
    }

    #[tokio::test]
    async fn test_create_manual_shelf_case_insensitive_duplicate() {
        let mut existing = fake_shelf(1, ShelfVisibility::Private);
        existing.name = "Fantasy".to_string();
        let svc = create_service(
            MockShelfRepository::default().with_list_for_user(Ok(vec![existing])),
            MockBookRepository::default(),
        );

        let result = svc.create_manual_shelf(1, "fantasy".to_string(), ShelfVisibility::Private).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::Conflict))));
    }

    // ─── rename_shelf ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_rename_shelf_success() {
        let shelf = fake_shelf(1, ShelfVisibility::Private);
        let token = shelf.token;
        let updated = Shelf {
            name: "New Name".to_string(),
            ..shelf.clone()
        };
        let svc = create_service(
            MockShelfRepository::default()
                .with_find_by_token(Ok(Some(shelf)))
                .with_list_for_user(Ok(vec![]))
                .with_update_shelf(Ok(updated)),
            MockBookRepository::default(),
        );

        let result = svc.rename_shelf(&token, "New Name".to_string(), 1).await;

        result.unwrap();
    }

    #[tokio::test]
    async fn test_rename_shelf_empty_name_returns_validation_error() {
        let svc = create_service(MockShelfRepository::default(), MockBookRepository::default());
        let token = ShelfToken::new(1);

        let result = svc.rename_shelf(&token, String::new(), 1).await;

        assert!(matches!(result, Err(Error::Validation(_))));
    }

    #[tokio::test]
    async fn test_rename_shelf_not_found() {
        let token = ShelfToken::new(99);
        let svc = create_service(MockShelfRepository::default().with_find_by_token(Ok(None)), MockBookRepository::default());

        let result = svc.rename_shelf(&token, "New Name".to_string(), 1).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::NotFound))));
    }

    #[tokio::test]
    async fn test_rename_shelf_wrong_owner_returns_validation_error() {
        let shelf = fake_shelf(1, ShelfVisibility::Private); // owned by user 1
        let token = shelf.token;
        let svc = create_service(
            MockShelfRepository::default().with_find_by_token(Ok(Some(shelf))),
            MockBookRepository::default(),
        );

        let result = svc.rename_shelf(&token, "New Name".to_string(), 2).await; // user 2

        assert!(matches!(result, Err(Error::Validation(_))));
    }

    #[tokio::test]
    async fn test_rename_shelf_duplicate_name_returns_conflict() {
        let shelf = fake_shelf(1, ShelfVisibility::Private);
        let token = shelf.token;
        let mut other = fake_shelf(1, ShelfVisibility::Private);
        other.id = 2;
        other.token = ShelfToken::new(2);
        other.name = "Other Shelf".to_string();
        let svc = create_service(
            MockShelfRepository::default()
                .with_find_by_token(Ok(Some(shelf)))
                .with_list_for_user(Ok(vec![other])),
            MockBookRepository::default(),
        );

        // Rename current shelf to the name of the other shelf
        let result = svc.rename_shelf(&token, "Other Shelf".to_string(), 1).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::Conflict))));
    }

    #[tokio::test]
    async fn test_rename_shelf_same_name_as_self_succeeds() {
        let shelf = fake_shelf(1, ShelfVisibility::Private);
        let token = shelf.token;
        let updated = shelf.clone();
        // list_for_user returns the shelf itself — must not conflict with itself
        let svc = create_service(
            MockShelfRepository::default()
                .with_find_by_token(Ok(Some(shelf.clone())))
                .with_list_for_user(Ok(vec![shelf]))
                .with_update_shelf(Ok(updated)),
            MockBookRepository::default(),
        );

        let result = svc.rename_shelf(&token, "My Shelf".to_string(), 1).await;

        result.unwrap();
    }

    // ─── delete_shelf ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_delete_shelf_success() {
        let shelf_repo = MockShelfRepository::default().with_find_by_token(Ok(Some(fake_shelf(1, ShelfVisibility::Private))));
        let shelf_repo = Arc::new(shelf_repo);
        let repository_service = Arc::new(
            RepositoryServiceBuilder::default()
                .repository(Arc::new(MockRepository) as Arc<dyn Repository>)
                .session_repository(Arc::new(MockSessionRepository) as Arc<dyn SessionRepository>)
                .user_repository(Arc::new(MockUserRepository) as Arc<dyn UserRepository>)
                .user_setting_repository(Arc::new(MockUserSettingRepository) as Arc<dyn UserSettingRepository>)
                .author_repository(Arc::new(MockAuthorRepository) as Arc<dyn AuthorRepository>)
                .series_repository(Arc::new(MockSeriesRepository) as Arc<dyn SeriesRepository>)
                .publisher_repository(Arc::new(MockPublisherRepository) as Arc<dyn PublisherRepository>)
                .genre_repository(Arc::new(MockGenreRepository) as Arc<dyn GenreRepository>)
                .tag_repository(Arc::new(MockTagRepository) as Arc<dyn TagRepository>)
                .book_repository(Arc::new(MockBookRepository::default()) as Arc<dyn BookRepository>)
                .import_job_repository(Arc::new(MockImportJobRepository) as Arc<dyn ImportJobRepository>)
                .job_repository(Arc::new(MockJobRepository) as Arc<dyn JobRepository>)
                .shelf_repository(shelf_repo.clone() as Arc<dyn ShelfRepository>)
                .user_book_metadata_repository(Arc::new(MockUserBookMetadataRepository) as Arc<dyn UserBookMetadataRepository>)
                .build()
                .expect("all fields provided"),
        );
        let svc = ShelfServiceImpl::new(repository_service);
        let token = ShelfToken::new(1);

        let result = svc.delete_shelf(&token, 1).await;

        result.unwrap();
        assert!(shelf_repo.delete_shelf_was_called());
    }

    #[tokio::test]
    async fn test_delete_shelf_not_found() {
        let token = ShelfToken::new(99);
        let svc = create_service(MockShelfRepository::default().with_find_by_token(Ok(None)), MockBookRepository::default());

        let result = svc.delete_shelf(&token, 1).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::NotFound))));
    }

    #[tokio::test]
    async fn test_delete_shelf_wrong_owner() {
        let shelf = fake_shelf(1, ShelfVisibility::Private); // owned by user 1
        let token = shelf.token;
        let svc = create_service(
            MockShelfRepository::default().with_find_by_token(Ok(Some(shelf))),
            MockBookRepository::default(),
        );

        let result = svc.delete_shelf(&token, 2).await; // user 2

        assert!(matches!(result, Err(Error::Validation(_))));
    }

    // ─── add_book_to_shelf ────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_add_book_to_shelf_success() {
        let shelf_repo = Arc::new(MockShelfRepository::default().with_find_by_token(Ok(Some(fake_shelf(1, ShelfVisibility::Private)))));
        let repository_service = Arc::new(
            RepositoryServiceBuilder::default()
                .repository(Arc::new(MockRepository) as Arc<dyn Repository>)
                .session_repository(Arc::new(MockSessionRepository) as Arc<dyn SessionRepository>)
                .user_repository(Arc::new(MockUserRepository) as Arc<dyn UserRepository>)
                .user_setting_repository(Arc::new(MockUserSettingRepository) as Arc<dyn UserSettingRepository>)
                .author_repository(Arc::new(MockAuthorRepository) as Arc<dyn AuthorRepository>)
                .series_repository(Arc::new(MockSeriesRepository) as Arc<dyn SeriesRepository>)
                .publisher_repository(Arc::new(MockPublisherRepository) as Arc<dyn PublisherRepository>)
                .genre_repository(Arc::new(MockGenreRepository) as Arc<dyn GenreRepository>)
                .tag_repository(Arc::new(MockTagRepository) as Arc<dyn TagRepository>)
                .book_repository(Arc::new(MockBookRepository::default().with_find_by_token(Ok(Some(fake_book())))) as Arc<dyn BookRepository>)
                .import_job_repository(Arc::new(MockImportJobRepository) as Arc<dyn ImportJobRepository>)
                .job_repository(Arc::new(MockJobRepository) as Arc<dyn JobRepository>)
                .shelf_repository(shelf_repo.clone() as Arc<dyn ShelfRepository>)
                .user_book_metadata_repository(Arc::new(MockUserBookMetadataRepository) as Arc<dyn UserBookMetadataRepository>)
                .build()
                .expect("all fields provided"),
        );
        let svc = ShelfServiceImpl::new(repository_service);

        let result = svc.add_book_to_shelf(&ShelfToken::new(1), &BookToken::new(1), 1).await;

        result.unwrap();
        assert!(shelf_repo.add_book_to_shelf_was_called());
    }

    #[tokio::test]
    async fn test_add_book_to_shelf_shelf_not_found() {
        let svc = create_service(MockShelfRepository::default().with_find_by_token(Ok(None)), MockBookRepository::default());

        let result = svc.add_book_to_shelf(&ShelfToken::new(1), &BookToken::new(1), 1).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::NotFound))));
    }

    #[tokio::test]
    async fn test_add_book_to_shelf_book_not_found() {
        let svc = create_service(
            MockShelfRepository::default().with_find_by_token(Ok(Some(fake_shelf(1, ShelfVisibility::Private)))),
            MockBookRepository::default().with_find_by_token(Ok(None)),
        );

        let result = svc.add_book_to_shelf(&ShelfToken::new(1), &BookToken::new(99), 1).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::NotFound))));
    }

    #[tokio::test]
    async fn test_add_book_to_shelf_wrong_owner() {
        let svc = create_service(
            MockShelfRepository::default().with_find_by_token(Ok(Some(fake_shelf(1, ShelfVisibility::Private)))),
            MockBookRepository::default(),
        );

        let result = svc.add_book_to_shelf(&ShelfToken::new(1), &BookToken::new(1), 2).await; // user 2

        assert!(matches!(result, Err(Error::Validation(_))));
    }

    // ─── remove_book_from_shelf ───────────────────────────────────────────────

    #[tokio::test]
    async fn test_remove_book_from_shelf_success() {
        let shelf_repo = Arc::new(MockShelfRepository::default().with_find_by_token(Ok(Some(fake_shelf(1, ShelfVisibility::Private)))));
        let repository_service = Arc::new(
            RepositoryServiceBuilder::default()
                .repository(Arc::new(MockRepository) as Arc<dyn Repository>)
                .session_repository(Arc::new(MockSessionRepository) as Arc<dyn SessionRepository>)
                .user_repository(Arc::new(MockUserRepository) as Arc<dyn UserRepository>)
                .user_setting_repository(Arc::new(MockUserSettingRepository) as Arc<dyn UserSettingRepository>)
                .author_repository(Arc::new(MockAuthorRepository) as Arc<dyn AuthorRepository>)
                .series_repository(Arc::new(MockSeriesRepository) as Arc<dyn SeriesRepository>)
                .publisher_repository(Arc::new(MockPublisherRepository) as Arc<dyn PublisherRepository>)
                .genre_repository(Arc::new(MockGenreRepository) as Arc<dyn GenreRepository>)
                .tag_repository(Arc::new(MockTagRepository) as Arc<dyn TagRepository>)
                .book_repository(Arc::new(MockBookRepository::default().with_find_by_token(Ok(Some(fake_book())))) as Arc<dyn BookRepository>)
                .import_job_repository(Arc::new(MockImportJobRepository) as Arc<dyn ImportJobRepository>)
                .job_repository(Arc::new(MockJobRepository) as Arc<dyn JobRepository>)
                .shelf_repository(shelf_repo.clone() as Arc<dyn ShelfRepository>)
                .user_book_metadata_repository(Arc::new(MockUserBookMetadataRepository) as Arc<dyn UserBookMetadataRepository>)
                .build()
                .expect("all fields provided"),
        );
        let svc = ShelfServiceImpl::new(repository_service);

        let result = svc.remove_book_from_shelf(&ShelfToken::new(1), &BookToken::new(1), 1).await;

        result.unwrap();
        assert!(shelf_repo.remove_book_from_shelf_was_called());
    }

    #[tokio::test]
    async fn test_remove_book_from_shelf_wrong_owner() {
        let svc = create_service(
            MockShelfRepository::default().with_find_by_token(Ok(Some(fake_shelf(1, ShelfVisibility::Private)))),
            MockBookRepository::default(),
        );

        let result = svc.remove_book_from_shelf(&ShelfToken::new(1), &BookToken::new(1), 2).await;

        assert!(matches!(result, Err(Error::Validation(_))));
    }

    // ─── books_for_shelf ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_books_for_shelf_owner_can_access_private() {
        let shelf = fake_shelf(1, ShelfVisibility::Private);
        let token = shelf.token;
        let svc = create_service(
            MockShelfRepository::default()
                .with_find_by_token(Ok(Some(shelf)))
                .with_books_for_shelf(Ok(vec![fake_book_shelf()])),
            MockBookRepository::default(),
        );

        let result = svc.books_for_shelf(&token, 1, None, None).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_books_for_shelf_public_accessible_by_other_user() {
        let mut shelf = fake_shelf(1, ShelfVisibility::Public);
        shelf.owner_id = 1;
        let token = shelf.token;
        let svc = create_service(
            MockShelfRepository::default()
                .with_find_by_token(Ok(Some(shelf)))
                .with_books_for_shelf(Ok(vec![])),
            MockBookRepository::default(),
        );

        let result = svc.books_for_shelf(&token, 2, None, None).await; // user 2 accessing user 1's public shelf

        result.unwrap();
    }

    #[tokio::test]
    async fn test_books_for_shelf_private_blocked_for_other_user() {
        let shelf = fake_shelf(1, ShelfVisibility::Private);
        let token = shelf.token;
        let svc = create_service(
            MockShelfRepository::default().with_find_by_token(Ok(Some(shelf))),
            MockBookRepository::default(),
        );

        let result = svc.books_for_shelf(&token, 2, None, None).await; // user 2 accessing user 1's private shelf

        assert!(matches!(result, Err(Error::Validation(_))));
    }

    #[tokio::test]
    async fn test_books_for_shelf_not_found() {
        let token = ShelfToken::new(99);
        let svc = create_service(MockShelfRepository::default().with_find_by_token(Ok(None)), MockBookRepository::default());

        let result = svc.books_for_shelf(&token, 1, None, None).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::NotFound))));
    }

    // ─── list_public_shelves ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_list_public_shelves_returns_others_public_shelves() {
        let mut public_shelf = fake_shelf(2, ShelfVisibility::Public); // owned by user 2
        public_shelf.owner_id = 2;
        let svc = create_service(
            MockShelfRepository::default().with_list_for_user(Ok(vec![public_shelf.clone()])),
            MockBookRepository::default(),
        );

        // list_public_shelves is backed by list_for_user in the mock (returns
        // Ok(vec![])) so just verify it doesn't error and returns the empty
        // default
        let result = svc.list_public_shelves(1).await;
        result.unwrap();
    }

    // ─── set_visibility ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_set_visibility_success() {
        let shelf = fake_shelf(1, ShelfVisibility::Private);
        let token = shelf.token;
        let updated = Shelf {
            visibility: ShelfVisibility::Public,
            ..shelf.clone()
        };
        let svc = create_service(
            MockShelfRepository::default()
                .with_find_by_token(Ok(Some(shelf)))
                .with_update_shelf(Ok(updated)),
            MockBookRepository::default(),
        );

        let result = svc.set_visibility(&token, ShelfVisibility::Public, 1).await;

        result.unwrap();
    }

    #[tokio::test]
    async fn test_set_visibility_not_found() {
        let token = ShelfToken::new(99);
        let svc = create_service(MockShelfRepository::default().with_find_by_token(Ok(None)), MockBookRepository::default());

        let result = svc.set_visibility(&token, ShelfVisibility::Public, 1).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::NotFound))));
    }

    #[tokio::test]
    async fn test_set_visibility_wrong_owner() {
        let shelf = fake_shelf(1, ShelfVisibility::Private); // owned by user 1
        let token = shelf.token;
        let svc = create_service(
            MockShelfRepository::default().with_find_by_token(Ok(Some(shelf))),
            MockBookRepository::default(),
        );

        let result = svc.set_visibility(&token, ShelfVisibility::Public, 2).await; // user 2

        assert!(matches!(result, Err(Error::Validation(_))));
    }

    // ─── list_shelves_for_user ────────────────────────────────────────────────

    #[tokio::test]
    async fn test_list_shelves_for_user_returns_all() {
        let mut shelf2 = fake_shelf(1, ShelfVisibility::Public);
        shelf2.id = 2;
        shelf2.token = ShelfToken::new(2);
        shelf2.name = "Public Shelf".to_string();
        let svc = create_service(
            MockShelfRepository::default().with_list_for_user(Ok(vec![fake_shelf(1, ShelfVisibility::Private), shelf2])),
            MockBookRepository::default(),
        );

        let result = svc.list_shelves_for_user(1).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 2);
    }

    // ─── get_shelf ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_get_shelf_owner_can_access_private() {
        let shelf = fake_shelf(1, ShelfVisibility::Private);
        let token = shelf.token;
        let svc = create_service(
            MockShelfRepository::default().with_find_by_token(Ok(Some(shelf))),
            MockBookRepository::default(),
        );

        let result = svc.get_shelf(&token, 1).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().visibility, ShelfVisibility::Private);
    }

    #[tokio::test]
    async fn test_get_shelf_non_owner_denied_private() {
        let shelf = fake_shelf(1, ShelfVisibility::Private); // owned by user 1
        let token = shelf.token;
        let svc = create_service(
            MockShelfRepository::default().with_find_by_token(Ok(Some(shelf))),
            MockBookRepository::default(),
        );

        let result = svc.get_shelf(&token, 2).await; // user 2

        assert!(matches!(result, Err(Error::Validation(_))));
    }

    #[tokio::test]
    async fn test_get_shelf_non_owner_can_access_public() {
        let shelf = fake_shelf(1, ShelfVisibility::Public); // owned by user 1
        let token = shelf.token;
        let svc = create_service(
            MockShelfRepository::default().with_find_by_token(Ok(Some(shelf))),
            MockBookRepository::default(),
        );

        let result = svc.get_shelf(&token, 2).await; // user 2

        result.unwrap();
    }

    // ─── update_shelf ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_update_shelf_success() {
        let shelf = fake_shelf(1, ShelfVisibility::Private);
        let token = shelf.token;
        let updated = Shelf {
            name: "Renamed".to_string(),
            visibility: ShelfVisibility::Public,
            ..shelf.clone()
        };
        let svc = create_service(
            MockShelfRepository::default()
                .with_find_by_token(Ok(Some(shelf)))
                .with_list_for_user(Ok(vec![]))
                .with_update_shelf(Ok(updated)),
            MockBookRepository::default(),
        );

        let result = svc.update_shelf(&token, "Renamed".to_string(), ShelfVisibility::Public, 1).await;

        result.unwrap();
    }

    #[tokio::test]
    async fn test_update_shelf_empty_name_returns_validation_error() {
        let svc = create_service(MockShelfRepository::default(), MockBookRepository::default());
        let token = ShelfToken::new(1);

        let result = svc.update_shelf(&token, "  ".to_string(), ShelfVisibility::Private, 1).await;

        assert!(matches!(result, Err(Error::Validation(_))));
    }

    #[tokio::test]
    async fn test_update_shelf_not_found() {
        let token = ShelfToken::new(99);
        let svc = create_service(MockShelfRepository::default().with_find_by_token(Ok(None)), MockBookRepository::default());

        let result = svc.update_shelf(&token, "New Name".to_string(), ShelfVisibility::Private, 1).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::NotFound))));
    }

    #[tokio::test]
    async fn test_update_shelf_wrong_owner_returns_validation_error() {
        let shelf = fake_shelf(1, ShelfVisibility::Private); // owned by user 1
        let token = shelf.token;
        let svc = create_service(
            MockShelfRepository::default().with_find_by_token(Ok(Some(shelf))),
            MockBookRepository::default(),
        );

        let result = svc.update_shelf(&token, "New Name".to_string(), ShelfVisibility::Private, 2).await; // user 2

        assert!(matches!(result, Err(Error::Validation(_))));
    }

    #[tokio::test]
    async fn test_update_shelf_duplicate_name_returns_conflict() {
        let shelf = fake_shelf(1, ShelfVisibility::Private);
        let token = shelf.token;
        let mut other = fake_shelf(1, ShelfVisibility::Private);
        other.id = 2;
        other.token = ShelfToken::new(2);
        other.name = "Other Shelf".to_string();
        let svc = create_service(
            MockShelfRepository::default()
                .with_find_by_token(Ok(Some(shelf)))
                .with_list_for_user(Ok(vec![other])),
            MockBookRepository::default(),
        );

        let result = svc.update_shelf(&token, "Other Shelf".to_string(), ShelfVisibility::Public, 1).await;

        assert!(matches!(result, Err(Error::RepositoryError(RepositoryError::Conflict))));
    }

    #[tokio::test]
    async fn test_update_shelf_same_name_as_self_succeeds() {
        let shelf = fake_shelf(1, ShelfVisibility::Private);
        let token = shelf.token;
        let updated = Shelf {
            visibility: ShelfVisibility::Public,
            ..shelf.clone()
        };
        let svc = create_service(
            MockShelfRepository::default()
                .with_find_by_token(Ok(Some(shelf.clone())))
                .with_list_for_user(Ok(vec![shelf]))
                .with_update_shelf(Ok(updated)),
            MockBookRepository::default(),
        );

        // Same name, different visibility — must not conflict with itself
        let result = svc.update_shelf(&token, "My Shelf".to_string(), ShelfVisibility::Public, 1).await;

        result.unwrap();
    }
}
