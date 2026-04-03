use crate::{
    Error,
    book::BookId,
    library::model::{Library, LibraryId, LibraryToken, NewLibrary},
    repository::Transaction,
    shelf::ShelfId,
    user::UserId,
};

#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait LibraryRepository: Send + Sync {
    async fn create_library(&self, transaction: &dyn Transaction, library: NewLibrary) -> Result<Library, Error>;
    async fn find_by_token(&self, transaction: &dyn Transaction, token: LibraryToken) -> Result<Option<Library>, Error>;
    async fn find_by_id(&self, transaction: &dyn Transaction, id: LibraryId) -> Result<Option<Library>, Error>;
    async fn list_libraries(&self, transaction: &dyn Transaction) -> Result<Vec<Library>, Error>;
    async fn delete_library(&self, transaction: &dyn Transaction, id: LibraryId) -> Result<(), Error>;
    async fn add_book_to_library(&self, transaction: &dyn Transaction, library_id: LibraryId, book_id: BookId) -> Result<(), Error>;
    async fn remove_book_from_library(&self, transaction: &dyn Transaction, library_id: LibraryId, book_id: BookId) -> Result<(), Error>;
    async fn library_ids_for_book(&self, transaction: &dyn Transaction, book_id: BookId) -> Result<Vec<LibraryId>, Error>;
    async fn assign_user_to_library(&self, transaction: &dyn Transaction, user_id: UserId, library_id: LibraryId) -> Result<(), Error>;
    async fn unassign_user_from_library(&self, transaction: &dyn Transaction, user_id: UserId, library_id: LibraryId) -> Result<(), Error>;
    async fn libraries_for_user(&self, transaction: &dyn Transaction, user_id: UserId) -> Result<Vec<Library>, Error>;
    async fn user_has_library(&self, transaction: &dyn Transaction, user_id: UserId, library_id: LibraryId) -> Result<bool, Error>;
    async fn user_count_for_library(&self, transaction: &dyn Transaction, library_id: LibraryId) -> Result<u64, Error>;
    async fn book_count_for_library(&self, transaction: &dyn Transaction, library_id: LibraryId) -> Result<u64, Error>;
    async fn reparent_shelves(&self, transaction: &dyn Transaction, from_library_id: LibraryId, to_library_id: LibraryId) -> Result<(), Error>;
    async fn copy_books_to_library(&self, transaction: &dyn Transaction, source_library_id: LibraryId, target_library_id: LibraryId) -> Result<(), Error>;
    async fn library_id_for_shelf(&self, transaction: &dyn Transaction, shelf_id: ShelfId) -> Result<Option<LibraryId>, Error>;
}
