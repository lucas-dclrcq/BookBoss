use bb_core::{
    Error,
    book::BookId,
    library::{Library, LibraryId, LibraryRepository, LibraryToken, NewLibrary},
    repository::Transaction,
    shelf::ShelfId,
    user::UserId,
};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, ModelTrait, PaginatorTrait, QueryFilter, QueryOrder};

use crate::{
    entities::{libraries, library_books, prelude, shelves, user_libraries},
    error::handle_dberr,
    transaction::TransactionImpl,
};

// ── From impls
// ────────────────────────────────────────────────────────────────

impl From<libraries::Model> for Library {
    fn from(m: libraries::Model) -> Self {
        Self {
            id: m.id as u64,
            version: m.version as u64,
            token: m.token.parse().expect("valid library token in DB"),
            name: m.name,
            is_system: m.is_system,
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
        }
    }
}

// ── Adapter
// ───────────────────────────────────────────────────────────────────

pub(crate) struct LibraryRepositoryAdapter;

impl LibraryRepositoryAdapter {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl LibraryRepository for LibraryRepositoryAdapter {
    async fn create_library(&self, transaction: &dyn Transaction, library: NewLibrary) -> Result<Library, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let token = LibraryToken::generate();
        let now = Utc::now();

        let model = libraries::ActiveModel {
            id: Set(token.id() as i64),
            version: Set(0),
            token: Set(token.to_string()),
            name: Set(library.name),
            is_system: Set(false),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
        };

        let model = model.insert(transaction).await.map_err(handle_dberr)?;
        Ok(model.into())
    }

    async fn find_by_token(&self, transaction: &dyn Transaction, token: LibraryToken) -> Result<Option<Library>, Error> {
        // Tokens encode the row ID, so looking up by PK is more efficient than a token
        // string scan. The unique constraint on the token column means token↔id
        // is always consistent.
        self.find_by_id(transaction, token.id()).await
    }

    async fn find_by_id(&self, transaction: &dyn Transaction, id: LibraryId) -> Result<Option<Library>, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        Ok(prelude::Libraries::find_by_id(id as i64)
            .one(transaction)
            .await
            .map_err(handle_dberr)?
            .map(Into::into))
    }

    async fn list_libraries(&self, transaction: &dyn Transaction) -> Result<Vec<Library>, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let rows = prelude::Libraries::find()
            .order_by_asc(libraries::Column::Name)
            .all(transaction)
            .await
            .map_err(handle_dberr)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn delete_library(&self, transaction: &dyn Transaction, id: LibraryId) -> Result<(), Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        if let Some(model) = prelude::Libraries::find_by_id(id as i64).one(transaction).await.map_err(handle_dberr)? {
            model.delete(transaction).await.map_err(handle_dberr)?;
        }

        Ok(())
    }

    async fn add_book_to_library(&self, transaction: &dyn Transaction, library_id: LibraryId, book_id: BookId) -> Result<(), Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        // Idempotent: skip if the book is already in this library.
        if prelude::LibraryBooks::find_by_id((library_id as i64, book_id as i64))
            .one(transaction)
            .await
            .map_err(handle_dberr)?
            .is_some()
        {
            return Ok(());
        }

        let model = library_books::ActiveModel {
            library_id: Set(library_id as i64),
            book_id: Set(book_id as i64),
            added_at: Set(Utc::now().into()),
        };

        model.insert(transaction).await.map_err(handle_dberr)?;

        Ok(())
    }

    async fn remove_book_from_library(&self, transaction: &dyn Transaction, library_id: LibraryId, book_id: BookId) -> Result<(), Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        if let Some(model) = prelude::LibraryBooks::find_by_id((library_id as i64, book_id as i64))
            .one(transaction)
            .await
            .map_err(handle_dberr)?
        {
            model.delete(transaction).await.map_err(handle_dberr)?;
        }

        Ok(())
    }

    async fn library_ids_for_book(&self, transaction: &dyn Transaction, book_id: BookId) -> Result<Vec<LibraryId>, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let rows = prelude::LibraryBooks::find()
            .filter(library_books::Column::BookId.eq(book_id as i64))
            .all(transaction)
            .await
            .map_err(handle_dberr)?;

        Ok(rows.into_iter().map(|m| m.library_id as u64).collect())
    }

    async fn assign_user_to_library(&self, transaction: &dyn Transaction, user_id: UserId, library_id: LibraryId) -> Result<(), Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        // Idempotent: skip if the user is already assigned to this library.
        if prelude::UserLibraries::find_by_id((user_id as i64, library_id as i64))
            .one(transaction)
            .await
            .map_err(handle_dberr)?
            .is_some()
        {
            return Ok(());
        }

        let model = user_libraries::ActiveModel {
            user_id: Set(user_id as i64),
            library_id: Set(library_id as i64),
            added_at: Set(Utc::now().into()),
        };

        model.insert(transaction).await.map_err(handle_dberr)?;

        Ok(())
    }

    async fn unassign_user_from_library(&self, transaction: &dyn Transaction, user_id: UserId, library_id: LibraryId) -> Result<(), Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        if let Some(model) = prelude::UserLibraries::find_by_id((user_id as i64, library_id as i64))
            .one(transaction)
            .await
            .map_err(handle_dberr)?
        {
            model.delete(transaction).await.map_err(handle_dberr)?;
        }

        Ok(())
    }

    async fn libraries_for_user(&self, transaction: &dyn Transaction, user_id: UserId) -> Result<Vec<Library>, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let user_lib_rows = prelude::UserLibraries::find()
            .filter(user_libraries::Column::UserId.eq(user_id as i64))
            .all(transaction)
            .await
            .map_err(handle_dberr)?;

        let library_ids: Vec<i64> = user_lib_rows.into_iter().map(|m| m.library_id).collect();

        let rows = prelude::Libraries::find()
            .filter(libraries::Column::Id.is_in(library_ids))
            .all(transaction)
            .await
            .map_err(handle_dberr)?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn user_has_library(&self, transaction: &dyn Transaction, user_id: UserId, library_id: LibraryId) -> Result<bool, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let count = prelude::UserLibraries::find()
            .filter(user_libraries::Column::UserId.eq(user_id as i64))
            .filter(user_libraries::Column::LibraryId.eq(library_id as i64))
            .count(transaction)
            .await
            .map_err(handle_dberr)?;

        Ok(count > 0)
    }

    async fn user_count_for_library(&self, transaction: &dyn Transaction, library_id: LibraryId) -> Result<u64, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        Ok(prelude::UserLibraries::find()
            .filter(user_libraries::Column::LibraryId.eq(library_id as i64))
            .count(transaction)
            .await
            .map_err(handle_dberr)?)
    }

    async fn book_count_for_library(&self, transaction: &dyn Transaction, library_id: LibraryId) -> Result<u64, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        Ok(prelude::LibraryBooks::find()
            .filter(library_books::Column::LibraryId.eq(library_id as i64))
            .count(transaction)
            .await
            .map_err(handle_dberr)?)
    }

    async fn reparent_shelves(&self, transaction: &dyn Transaction, from_library_id: LibraryId, to_library_id: LibraryId) -> Result<(), Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let rows = prelude::Shelves::find()
            .filter(shelves::Column::LibraryId.eq(from_library_id as i64))
            .all(transaction)
            .await
            .map_err(handle_dberr)?;

        for row in rows {
            let mut model: shelves::ActiveModel = row.into();
            model.library_id = Set(to_library_id as i64);
            model.update(transaction).await.map_err(handle_dberr)?;
        }

        Ok(())
    }

    async fn copy_books_to_library(&self, transaction: &dyn Transaction, source_library_id: LibraryId, target_library_id: LibraryId) -> Result<(), Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let rows = prelude::LibraryBooks::find()
            .filter(library_books::Column::LibraryId.eq(source_library_id as i64))
            .all(transaction)
            .await
            .map_err(handle_dberr)?;

        let now = Utc::now();
        for row in rows {
            // Idempotent: skip if the book is already in the target library.
            if prelude::LibraryBooks::find_by_id((target_library_id as i64, row.book_id))
                .one(transaction)
                .await
                .map_err(handle_dberr)?
                .is_some()
            {
                continue;
            }

            let model = library_books::ActiveModel {
                library_id: Set(target_library_id as i64),
                book_id: Set(row.book_id),
                added_at: Set(now.into()),
            };

            model.insert(transaction).await.map_err(handle_dberr)?;
        }

        Ok(())
    }

    async fn library_id_for_shelf(&self, transaction: &dyn Transaction, shelf_id: ShelfId) -> Result<Option<LibraryId>, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        Ok(prelude::Shelves::find_by_id(shelf_id as i64)
            .one(transaction)
            .await
            .map_err(handle_dberr)?
            .map(|m| m.library_id as u64))
    }
}
