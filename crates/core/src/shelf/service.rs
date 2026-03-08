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
}
