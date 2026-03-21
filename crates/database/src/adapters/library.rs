use bb_core::{
    Error,
    book::{Book, BookSortOrder},
    filter::BookFilter,
    library::LibraryRepository,
    repository::Transaction,
    user::UserId,
};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect};

use crate::{
    entities::{books, prelude},
    error::handle_dberr,
    filter::build_condition,
    transaction::TransactionImpl,
};

pub struct LibraryRepositoryAdapter;

impl LibraryRepositoryAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl LibraryRepository for LibraryRepositoryAdapter {
    async fn count_available_books(&self, transaction: &dyn Transaction) -> Result<u64, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        Ok(prelude::Books::find()
            .filter(books::Column::Status.eq("available"))
            .count(transaction)
            .await
            .map_err(handle_dberr)?)
    }

    async fn count_authors(&self, transaction: &dyn Transaction) -> Result<u64, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        Ok(prelude::Authors::find().count(transaction).await.map_err(handle_dberr)?)
    }

    async fn books_for_filter(
        &self,
        transaction: &dyn Transaction,
        filter: &BookFilter,
        user_id: UserId,
        offset: Option<u64>,
        page_size: Option<u64>,
        sort: Option<BookSortOrder>,
    ) -> Result<Vec<Book>, Error> {
        const DEFAULT_PAGE_SIZE: u64 = 50;
        const MAX_PAGE_SIZE: u64 = 50;

        if let Some(page_size) = page_size {
            if page_size < 1 {
                return Err(Error::InvalidPageSize(page_size));
            }
        }

        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let query = prelude::Books::find()
            .filter(books::Column::Status.eq("available"))
            .filter(build_condition(filter, user_id).map_err(bb_core::Error::RepositoryError)?);

        let query = crate::sort::apply_book_sort(query, sort);

        let mut query = if let Some(offset) = offset { query.offset(offset) } else { query };

        let page_size = page_size.unwrap_or(DEFAULT_PAGE_SIZE).min(MAX_PAGE_SIZE);
        query = query.limit(page_size);

        let rows = query.all(transaction).await.map_err(handle_dberr)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn count_for_filter(&self, transaction: &dyn Transaction, filter: &BookFilter, user_id: UserId) -> Result<u64, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let query = prelude::Books::find()
            .filter(books::Column::Status.eq("available"))
            .filter(build_condition(filter, user_id).map_err(bb_core::Error::RepositoryError)?);

        Ok(query.count(transaction).await.map_err(handle_dberr)?)
    }
}
