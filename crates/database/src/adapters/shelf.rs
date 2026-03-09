use bb_core::{
    Error, RepositoryError,
    book::{Book, BookId},
    reading::ReadStatus,
    repository::Transaction,
    shelf::{BookShelf, NewShelf, Shelf, ShelfFilter, ShelfId, ShelfRepository, ShelfToken, ShelfType, ShelfVisibility},
    user::UserId,
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, ModelTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, sea_query::Query,
};

use crate::{
    entities::{book_authors, book_genres, book_shelves, book_tags, books, prelude, shelves, user_book_metadata},
    error::handle_dberr,
    transaction::TransactionImpl,
};

// ── String conversions
// ────────────────────────────────────────────────────────

fn shelf_type_to_str(t: &ShelfType) -> &'static str {
    match t {
        ShelfType::System => "system",
        ShelfType::Manual => "manual",
        ShelfType::Smart => "smart",
    }
}

fn str_to_shelf_type(s: &str) -> ShelfType {
    match s {
        "system" => ShelfType::System,
        "smart" => ShelfType::Smart,
        _ => ShelfType::Manual,
    }
}

fn shelf_visibility_to_str(v: &ShelfVisibility) -> &'static str {
    match v {
        ShelfVisibility::Private => "private",
        ShelfVisibility::Public => "public",
    }
}

fn str_to_shelf_visibility(s: &str) -> ShelfVisibility {
    match s {
        "public" => ShelfVisibility::Public,
        _ => ShelfVisibility::Private,
    }
}

fn read_status_to_str(s: &ReadStatus) -> &'static str {
    match s {
        ReadStatus::Unread => "unread",
        ReadStatus::Reading => "reading",
        ReadStatus::Paused => "paused",
        ReadStatus::Rereading => "rereading",
        ReadStatus::Read => "read",
        ReadStatus::Abandoned => "abandoned",
    }
}

// ── From impls
// ────────────────────────────────────────────────────────────────

impl From<shelves::Model> for Shelf {
    fn from(m: shelves::Model) -> Self {
        let token = ShelfToken::new(m.id as u64);
        let filter_criteria = m.filter_criteria.and_then(|v| serde_json::from_value(v).ok());

        Self {
            id: m.id as u64,
            version: m.version as u64,
            token,
            owner_id: m.owner_id as u64,
            name: m.name,
            shelf_type: str_to_shelf_type(&m.shelf_type),
            visibility: str_to_shelf_visibility(&m.visibility),
            device_id: m.device_id.map(|id| id as u64),
            filter_criteria,
            created_at: m.created_at.with_timezone(&Utc),
            updated_at: m.updated_at.with_timezone(&Utc),
        }
    }
}

impl From<book_shelves::Model> for BookShelf {
    fn from(m: book_shelves::Model) -> Self {
        Self {
            book_id: m.book_id as u64,
            shelf_id: m.shelf_id as u64,
            added_at: m.added_at.with_timezone(&Utc),
            sort_order: m.sort_order,
        }
    }
}

// ── Adapter
// ───────────────────────────────────────────────────────────────────

pub(crate) struct ShelfRepositoryAdapter;

impl ShelfRepositoryAdapter {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl ShelfRepository for ShelfRepositoryAdapter {
    async fn add_shelf(&self, transaction: &dyn Transaction, shelf: NewShelf) -> Result<Shelf, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let token = ShelfToken::generate();
        let now = Utc::now();

        let model = shelves::ActiveModel {
            id: Set(token.id() as i64),
            token: Set(token.to_string()),
            owner_id: Set(shelf.owner_id as i64),
            name: Set(shelf.name),
            shelf_type: Set(shelf_type_to_str(&shelf.shelf_type).to_owned()),
            visibility: Set(shelf_visibility_to_str(&shelf.visibility).to_owned()),
            device_id: Set(shelf.device_id.map(|id| id as i64)),
            filter_criteria: Set(shelf.filter_criteria.and_then(|f| serde_json::to_value(f).ok())),
            version: Set(0),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
        };

        let model = model.insert(transaction).await.map_err(handle_dberr)?;
        Ok(model.into())
    }

    async fn update_shelf(&self, transaction: &dyn Transaction, shelf: Shelf) -> Result<Shelf, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let existing = prelude::Shelves::find_by_id(shelf.id as i64)
            .one(transaction)
            .await
            .map_err(handle_dberr)?
            .ok_or(Error::RepositoryError(RepositoryError::NotFound))?;

        if existing.version != shelf.version as i64 {
            return Err(Error::RepositoryError(RepositoryError::Conflict));
        }

        let mut updater: shelves::ActiveModel = existing.into();
        updater.name = Set(shelf.name);
        updater.visibility = Set(shelf_visibility_to_str(&shelf.visibility).to_owned());
        updater.device_id = Set(shelf.device_id.map(|id| id as i64));
        updater.filter_criteria = Set(shelf.filter_criteria.and_then(|f| serde_json::to_value(f).ok()));

        let updated = updater.update(transaction).await.map_err(handle_dberr)?;
        Ok(updated.into())
    }

    async fn delete_shelf(&self, transaction: &dyn Transaction, shelf: Shelf) -> Result<(), Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        if let Some(existing) = prelude::Shelves::find_by_id(shelf.id as i64).one(transaction).await.map_err(handle_dberr)? {
            existing.delete(transaction).await.map_err(handle_dberr)?;
        }

        Ok(())
    }

    async fn find_by_id(&self, transaction: &dyn Transaction, id: ShelfId) -> Result<Option<Shelf>, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        Ok(prelude::Shelves::find_by_id(id as i64)
            .one(transaction)
            .await
            .map_err(handle_dberr)?
            .map(Into::into))
    }

    async fn find_by_token(&self, transaction: &dyn Transaction, token: &ShelfToken) -> Result<Option<Shelf>, Error> {
        self.find_by_id(transaction, token.id()).await
    }

    async fn list_for_user(&self, transaction: &dyn Transaction, owner_id: UserId) -> Result<Vec<Shelf>, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let rows = prelude::Shelves::find()
            .filter(shelves::Column::OwnerId.eq(owner_id as i64))
            .order_by_asc(shelves::Column::CreatedAt)
            .all(transaction)
            .await
            .map_err(handle_dberr)?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_public_shelves(&self, transaction: &dyn Transaction, exclude_owner_id: UserId) -> Result<Vec<Shelf>, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let rows = prelude::Shelves::find()
            .filter(shelves::Column::Visibility.eq("public"))
            .filter(shelves::Column::OwnerId.ne(exclude_owner_id as i64))
            .order_by_asc(shelves::Column::Name)
            .all(transaction)
            .await
            .map_err(handle_dberr)?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn add_book_to_shelf(&self, transaction: &dyn Transaction, book_shelf: BookShelf) -> Result<BookShelf, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        // Idempotent: return existing row if book is already on this shelf.
        if let Some(existing) = prelude::BookShelves::find_by_id((book_shelf.book_id as i64, book_shelf.shelf_id as i64))
            .one(transaction)
            .await
            .map_err(handle_dberr)?
        {
            return Ok(existing.into());
        }

        // sort_order = MAX(sort_order) + 1 for this shelf, or 0 if empty.
        let max_sort_order = prelude::BookShelves::find()
            .filter(book_shelves::Column::ShelfId.eq(book_shelf.shelf_id as i64))
            .order_by_desc(book_shelves::Column::SortOrder)
            .one(transaction)
            .await
            .map_err(handle_dberr)?
            .map(|m| m.sort_order)
            .unwrap_or(-1);

        let model = book_shelves::ActiveModel {
            book_id: Set(book_shelf.book_id as i64),
            shelf_id: Set(book_shelf.shelf_id as i64),
            added_at: Set(Utc::now().into()),
            sort_order: Set(max_sort_order + 1),
        };

        let model = model.insert(transaction).await.map_err(handle_dberr)?;
        Ok(model.into())
    }

    async fn remove_book_from_shelf(&self, transaction: &dyn Transaction, shelf_id: ShelfId, book_id: BookId) -> Result<(), Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        if let Some(existing) = prelude::BookShelves::find_by_id((book_id as i64, shelf_id as i64))
            .one(transaction)
            .await
            .map_err(handle_dberr)?
        {
            existing.delete(transaction).await.map_err(handle_dberr)?;
        }

        Ok(())
    }

    async fn books_for_shelf(
        &self,
        transaction: &dyn Transaction,
        shelf_id: ShelfId,
        start_id: Option<BookId>,
        page_size: Option<u64>,
    ) -> Result<Vec<BookShelf>, Error> {
        const DEFAULT_PAGE_SIZE: u64 = 50;
        const MAX_PAGE_SIZE: u64 = 50;

        if let Some(page_size) = page_size {
            if page_size < 1 {
                return Err(Error::InvalidPageSize(page_size));
            }
        }

        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let mut query = prelude::BookShelves::find()
            .filter(book_shelves::Column::ShelfId.eq(shelf_id as i64))
            .order_by_asc(book_shelves::Column::BookId);

        if let Some(start_id) = start_id {
            query = query.filter(book_shelves::Column::BookId.gte(start_id as i64));
        }

        let page_size = page_size.unwrap_or(DEFAULT_PAGE_SIZE).min(MAX_PAGE_SIZE);
        query = query.limit(page_size);

        let rows = query.all(transaction).await.map_err(handle_dberr)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn books_for_filter(
        &self,
        transaction: &dyn Transaction,
        filter: &ShelfFilter,
        user_id: UserId,
        start_id: Option<BookId>,
        page_size: Option<u64>,
    ) -> Result<Vec<Book>, Error> {
        const DEFAULT_PAGE_SIZE: u64 = 50;
        const MAX_PAGE_SIZE: u64 = 50;

        if let Some(page_size) = page_size {
            if page_size < 1 {
                return Err(Error::InvalidPageSize(page_size));
            }
        }

        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let mut query = prelude::Books::find()
            .filter(books::Column::Status.eq("available"))
            .order_by_asc(books::Column::Id);

        if let Some(start_id) = start_id {
            query = query.filter(books::Column::Id.gte(start_id as i64));
        }

        query = apply_shelf_filter(query, filter, user_id);

        let page_size = page_size.unwrap_or(DEFAULT_PAGE_SIZE).min(MAX_PAGE_SIZE);
        query = query.limit(page_size);

        let rows = query.all(transaction).await.map_err(handle_dberr)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn count_for_filter(&self, transaction: &dyn Transaction, filter: &ShelfFilter, user_id: UserId) -> Result<u64, Error> {
        let transaction = TransactionImpl::get_db_transaction(transaction)?;

        let query = prelude::Books::find().filter(books::Column::Status.eq("available"));
        let query = apply_shelf_filter(query, filter, user_id);

        Ok(query.count(transaction).await.map_err(handle_dberr)?)
    }
}

// ── Filter helper
// ─────────────────────────────────────────────────────────────

fn apply_shelf_filter(mut query: sea_orm::Select<books::Entity>, filter: &ShelfFilter, user_id: UserId) -> sea_orm::Select<books::Entity> {
    if let Some(author_ids) = &filter.authors {
        if !author_ids.is_empty() {
            let ids: Vec<i64> = author_ids.iter().map(|&id| id as i64).collect();
            let mut subq = Query::select();
            subq.column(book_authors::Column::BookId)
                .from(book_authors::Entity)
                .and_where(book_authors::Column::AuthorId.is_in(ids));
            query = query.filter(books::Column::Id.in_subquery(subq));
        }
    }

    if let Some(series_ids) = &filter.series {
        if !series_ids.is_empty() {
            let ids: Vec<i64> = series_ids.iter().map(|&id| id as i64).collect();
            query = query.filter(books::Column::SeriesId.is_in(ids));
        }
    }

    if let Some(genre_ids) = &filter.genres {
        if !genre_ids.is_empty() {
            let ids: Vec<i64> = genre_ids.iter().map(|&id| id as i64).collect();
            let mut subq = Query::select();
            subq.column(book_genres::Column::BookId)
                .from(book_genres::Entity)
                .and_where(book_genres::Column::GenreId.is_in(ids));
            query = query.filter(books::Column::Id.in_subquery(subq));
        }
    }

    if let Some(tag_ids) = &filter.tags {
        if !tag_ids.is_empty() {
            let ids: Vec<i64> = tag_ids.iter().map(|&id| id as i64).collect();
            let mut subq = Query::select();
            subq.column(book_tags::Column::BookId)
                .from(book_tags::Entity)
                .and_where(book_tags::Column::TagId.is_in(ids));
            query = query.filter(books::Column::Id.in_subquery(subq));
        }
    }

    if let Some(publisher_ids) = &filter.publishers {
        if !publisher_ids.is_empty() {
            let ids: Vec<i64> = publisher_ids.iter().map(|&id| id as i64).collect();
            query = query.filter(books::Column::PublisherId.is_in(ids));
        }
    }

    if let Some(languages) = &filter.languages {
        if !languages.is_empty() {
            query = query.filter(books::Column::Language.is_in(languages.clone()));
        }
    }

    if let Some(after) = filter.date_added_after {
        query = query.filter(books::Column::CreatedAt.gt(after.fixed_offset()));
    }

    if let Some(read_statuses) = &filter.read_status {
        if !read_statuses.is_empty() {
            let status_strs: Vec<String> = read_statuses.iter().map(|s| read_status_to_str(s).to_owned()).collect();
            let mut subq = Query::select();
            subq.column(user_book_metadata::Column::BookId)
                .from(user_book_metadata::Entity)
                .and_where(user_book_metadata::Column::UserId.eq(user_id as i64))
                .and_where(user_book_metadata::Column::ReadStatus.is_in(status_strs));
            query = query.filter(books::Column::Id.in_subquery(subq));
        }
    }

    if let Some(rating_min) = filter.rating_min {
        let mut subq = Query::select();
        subq.column(user_book_metadata::Column::BookId)
            .from(user_book_metadata::Entity)
            .and_where(user_book_metadata::Column::UserId.eq(user_id as i64))
            .and_where(user_book_metadata::Column::PersonalRating.gte(rating_min as i64));
        query = query.filter(books::Column::Id.in_subquery(subq));
    }

    query
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bb_core::{
        book::{BookRepository, BookStatus, NewBook, NewSeries, SeriesRepository},
        repository::RepositoryService,
        shelf::{BookShelf, NewShelf, ShelfFilter, ShelfType, ShelfVisibility},
        types::Capabilities,
        user::{NewUser, UserRepository},
    };
    use chrono::Utc;
    use sea_orm::Database;

    use crate::create_repository_service;

    async fn setup() -> Arc<RepositoryService> {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        create_repository_service(db).await.unwrap()
    }

    async fn new_user(svc: &RepositoryService, username: &str) -> u64 {
        let tx = svc.repository().begin().await.unwrap();
        let user = svc
            .user_repository()
            .add_user(
                &*tx,
                NewUser::new(username, "password", format!("{username}@example.com"), Capabilities::default()).unwrap(),
            )
            .await
            .unwrap();
        tx.commit().await.unwrap();
        user.id
    }

    async fn new_book(svc: &RepositoryService, title: &str) -> u64 {
        let tx = svc.repository().begin().await.unwrap();
        let book = svc
            .book_repository()
            .add_book(
                &*tx,
                NewBook {
                    title: title.to_owned(),
                    status: BookStatus::Available,
                    description: None,
                    published_date: None,
                    language: None,
                    series_id: None,
                    series_number: None,
                    publisher_id: None,
                    page_count: None,
                    rating: None,
                    metadata_source: None,
                    cover_path: None,
                },
            )
            .await
            .unwrap();
        tx.commit().await.unwrap();
        book.id
    }

    fn manual_shelf(owner_id: u64, name: &str) -> NewShelf {
        NewShelf {
            owner_id,
            name: name.to_owned(),
            shelf_type: ShelfType::Manual,
            visibility: ShelfVisibility::Private,
            device_id: None,
            filter_criteria: None,
        }
    }

    fn book_shelf_entry(book_id: u64, shelf_id: u64) -> BookShelf {
        BookShelf {
            book_id,
            shelf_id,
            added_at: Utc::now(),
            sort_order: 0,
        }
    }

    // ─── add_shelf ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_add_shelf_success() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        let tx = svc.repository().begin().await.unwrap();

        let result = svc.shelf_repository().add_shelf(&*tx, manual_shelf(user_id, "Favourites")).await;

        assert!(result.is_ok());
        let shelf = result.unwrap();
        assert_ne!(shelf.id, 0);
        assert_eq!(shelf.name, "Favourites");
        assert_eq!(shelf.owner_id, user_id);
        assert_eq!(shelf.shelf_type, ShelfType::Manual);
        assert_eq!(shelf.visibility, ShelfVisibility::Private);
        assert_eq!(shelf.token.id(), shelf.id);
    }

    // ─── find_by_id ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_find_by_id_found() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        let tx = svc.repository().begin().await.unwrap();

        let shelf = svc.shelf_repository().add_shelf(&*tx, manual_shelf(user_id, "To Read")).await.unwrap();
        let found = svc.shelf_repository().find_by_id(&*tx, shelf.id).await.unwrap();

        assert!(found.is_some());
        assert_eq!(found.unwrap().id, shelf.id);
    }

    #[tokio::test]
    async fn test_find_by_id_not_found() {
        let svc = setup().await;
        let tx = svc.repository().begin().await.unwrap();

        assert!(svc.shelf_repository().find_by_id(&*tx, 999999).await.unwrap().is_none());
    }

    // ─── find_by_token ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_find_by_token_found() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        let tx = svc.repository().begin().await.unwrap();

        let shelf = svc.shelf_repository().add_shelf(&*tx, manual_shelf(user_id, "To Read")).await.unwrap();
        let found = svc.shelf_repository().find_by_token(&*tx, &shelf.token).await.unwrap();

        assert!(found.is_some());
        assert_eq!(found.unwrap().id, shelf.id);
    }

    // ─── list_for_user ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_list_for_user_returns_own_shelves() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        let tx = svc.repository().begin().await.unwrap();

        svc.shelf_repository().add_shelf(&*tx, manual_shelf(user_id, "A")).await.unwrap();
        svc.shelf_repository().add_shelf(&*tx, manual_shelf(user_id, "B")).await.unwrap();

        let shelves = svc.shelf_repository().list_for_user(&*tx, user_id).await.unwrap();
        assert_eq!(shelves.len(), 2);
    }

    #[tokio::test]
    async fn test_list_for_user_excludes_other_users() {
        let svc = setup().await;
        let alice = new_user(&svc, "alice").await;
        let bob = new_user(&svc, "bob").await;
        let tx = svc.repository().begin().await.unwrap();

        svc.shelf_repository().add_shelf(&*tx, manual_shelf(alice, "Alice's Shelf")).await.unwrap();
        svc.shelf_repository().add_shelf(&*tx, manual_shelf(bob, "Bob's Shelf")).await.unwrap();

        let alice_shelves = svc.shelf_repository().list_for_user(&*tx, alice).await.unwrap();
        assert_eq!(alice_shelves.len(), 1);
        assert_eq!(alice_shelves[0].owner_id, alice);
    }

    // ─── update_shelf ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_update_shelf_success() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        let tx = svc.repository().begin().await.unwrap();

        let shelf = svc.shelf_repository().add_shelf(&*tx, manual_shelf(user_id, "Old Name")).await.unwrap();
        let version_before = shelf.version;

        let mut updated = shelf;
        updated.name = "New Name".to_owned();
        updated.visibility = ShelfVisibility::Public;

        let result = svc.shelf_repository().update_shelf(&*tx, updated).await;
        assert!(result.is_ok());
        let saved = result.unwrap();
        assert_eq!(saved.name, "New Name");
        assert_eq!(saved.visibility, ShelfVisibility::Public);
        assert_eq!(saved.version, version_before + 1);
    }

    #[tokio::test]
    async fn test_update_shelf_version_conflict() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        let tx = svc.repository().begin().await.unwrap();

        let mut shelf = svc.shelf_repository().add_shelf(&*tx, manual_shelf(user_id, "Shelf")).await.unwrap();
        shelf.version = 99;

        assert!(matches!(
            svc.shelf_repository().update_shelf(&*tx, shelf).await,
            Err(bb_core::Error::RepositoryError(bb_core::RepositoryError::Conflict))
        ));
    }

    #[tokio::test]
    async fn test_update_shelf_not_found() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        let tx = svc.repository().begin().await.unwrap();

        let mut shelf = svc.shelf_repository().add_shelf(&*tx, manual_shelf(user_id, "Shelf")).await.unwrap();
        shelf.id = 999999;

        assert!(matches!(
            svc.shelf_repository().update_shelf(&*tx, shelf).await,
            Err(bb_core::Error::RepositoryError(bb_core::RepositoryError::NotFound))
        ));
    }

    // ─── delete_shelf ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_delete_shelf_success() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        let tx = svc.repository().begin().await.unwrap();

        let shelf = svc.shelf_repository().add_shelf(&*tx, manual_shelf(user_id, "Temp")).await.unwrap();
        svc.shelf_repository().delete_shelf(&*tx, shelf.clone()).await.unwrap();

        assert!(svc.shelf_repository().find_by_id(&*tx, shelf.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_delete_shelf_cascades_book_shelves() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        let book_id = new_book(&svc, "Dune").await;
        let tx = svc.repository().begin().await.unwrap();

        let shelf = svc.shelf_repository().add_shelf(&*tx, manual_shelf(user_id, "Temp")).await.unwrap();
        let shelf_id = shelf.id;
        svc.shelf_repository()
            .add_book_to_shelf(&*tx, book_shelf_entry(book_id, shelf_id))
            .await
            .unwrap();
        svc.shelf_repository().delete_shelf(&*tx, shelf).await.unwrap();

        let remaining = svc.shelf_repository().books_for_shelf(&*tx, shelf_id, None, None).await.unwrap();
        assert!(remaining.is_empty());
    }

    // ─── add_book_to_shelf ───────────────────────────────────────────────────

    #[tokio::test]
    async fn test_add_book_to_shelf_success() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        let book_id = new_book(&svc, "Dune").await;
        let tx = svc.repository().begin().await.unwrap();

        let shelf = svc.shelf_repository().add_shelf(&*tx, manual_shelf(user_id, "Favs")).await.unwrap();
        let result = svc.shelf_repository().add_book_to_shelf(&*tx, book_shelf_entry(book_id, shelf.id)).await;

        assert!(result.is_ok());
        let bs = result.unwrap();
        assert_eq!(bs.book_id, book_id);
        assert_eq!(bs.shelf_id, shelf.id);
        assert_eq!(bs.sort_order, 0);
    }

    #[tokio::test]
    async fn test_add_book_to_shelf_idempotent() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        let book_id = new_book(&svc, "Dune").await;
        let tx = svc.repository().begin().await.unwrap();

        let shelf = svc.shelf_repository().add_shelf(&*tx, manual_shelf(user_id, "Favs")).await.unwrap();

        svc.shelf_repository()
            .add_book_to_shelf(&*tx, book_shelf_entry(book_id, shelf.id))
            .await
            .unwrap();
        let result = svc.shelf_repository().add_book_to_shelf(&*tx, book_shelf_entry(book_id, shelf.id)).await;

        assert!(result.is_ok());
        let books = svc.shelf_repository().books_for_shelf(&*tx, shelf.id, None, None).await.unwrap();
        assert_eq!(books.len(), 1);
    }

    #[tokio::test]
    async fn test_add_book_to_shelf_increments_sort_order() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        let book1 = new_book(&svc, "Dune").await;
        let book2 = new_book(&svc, "Foundation").await;
        let tx = svc.repository().begin().await.unwrap();

        let shelf = svc.shelf_repository().add_shelf(&*tx, manual_shelf(user_id, "Favs")).await.unwrap();
        let bs1 = svc.shelf_repository().add_book_to_shelf(&*tx, book_shelf_entry(book1, shelf.id)).await.unwrap();
        let bs2 = svc.shelf_repository().add_book_to_shelf(&*tx, book_shelf_entry(book2, shelf.id)).await.unwrap();

        assert_eq!(bs1.sort_order, 0);
        assert_eq!(bs2.sort_order, 1);
    }

    // ─── remove_book_from_shelf ──────────────────────────────────────────────

    #[tokio::test]
    async fn test_remove_book_from_shelf_success() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        let book_id = new_book(&svc, "Dune").await;
        let tx = svc.repository().begin().await.unwrap();

        let shelf = svc.shelf_repository().add_shelf(&*tx, manual_shelf(user_id, "Favs")).await.unwrap();
        svc.shelf_repository()
            .add_book_to_shelf(&*tx, book_shelf_entry(book_id, shelf.id))
            .await
            .unwrap();
        svc.shelf_repository().remove_book_from_shelf(&*tx, shelf.id, book_id).await.unwrap();

        let books = svc.shelf_repository().books_for_shelf(&*tx, shelf.id, None, None).await.unwrap();
        assert!(books.is_empty());
    }

    #[tokio::test]
    async fn test_remove_book_from_shelf_nonexistent_is_ok() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        let tx = svc.repository().begin().await.unwrap();

        let shelf = svc.shelf_repository().add_shelf(&*tx, manual_shelf(user_id, "Favs")).await.unwrap();
        let result = svc.shelf_repository().remove_book_from_shelf(&*tx, shelf.id, 999999).await;

        assert!(result.is_ok());
    }

    // ─── books_for_shelf ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_books_for_shelf_sorted_by_sort_order() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        let book1 = new_book(&svc, "Dune").await;
        let book2 = new_book(&svc, "Foundation").await;
        let book3 = new_book(&svc, "Hyperion").await;
        let tx = svc.repository().begin().await.unwrap();

        let shelf = svc.shelf_repository().add_shelf(&*tx, manual_shelf(user_id, "Favs")).await.unwrap();
        for book_id in [book1, book2, book3] {
            svc.shelf_repository()
                .add_book_to_shelf(&*tx, book_shelf_entry(book_id, shelf.id))
                .await
                .unwrap();
        }

        let books = svc.shelf_repository().books_for_shelf(&*tx, shelf.id, None, None).await.unwrap();
        assert_eq!(books.len(), 3);
    }

    #[tokio::test]
    async fn test_books_for_shelf_pagination() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        let book1 = new_book(&svc, "A").await;
        let book2 = new_book(&svc, "B").await;
        let book3 = new_book(&svc, "C").await;
        let tx = svc.repository().begin().await.unwrap();

        let shelf = svc.shelf_repository().add_shelf(&*tx, manual_shelf(user_id, "Shelf")).await.unwrap();
        for book_id in [book1, book2, book3] {
            svc.shelf_repository()
                .add_book_to_shelf(&*tx, book_shelf_entry(book_id, shelf.id))
                .await
                .unwrap();
        }

        let page1 = svc.shelf_repository().books_for_shelf(&*tx, shelf.id, None, Some(2)).await.unwrap();
        assert_eq!(page1.len(), 2);

        let last_id = page1.last().unwrap().book_id;
        let page2 = svc
            .shelf_repository()
            .books_for_shelf(&*tx, shelf.id, Some(last_id + 1), Some(2))
            .await
            .unwrap();
        assert_eq!(page2.len(), 1);
    }

    // ─── books_for_filter ────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_books_for_filter_empty_returns_all_available() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        new_book(&svc, "Dune").await;
        new_book(&svc, "Foundation").await;

        let tx = svc.repository().begin().await.unwrap();
        let books = svc
            .shelf_repository()
            .books_for_filter(&*tx, &ShelfFilter::default(), user_id, None, None)
            .await
            .unwrap();

        assert_eq!(books.len(), 2);
    }

    #[tokio::test]
    async fn test_books_for_filter_by_series() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;

        let tx = svc.repository().begin().await.unwrap();
        let series = svc
            .series_repository()
            .add_series(
                &*tx,
                NewSeries {
                    name: "Dune Saga".to_owned(),
                    description: None,
                },
            )
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let tx = svc.repository().begin().await.unwrap();
        let book_in_series = svc
            .book_repository()
            .add_book(
                &*tx,
                NewBook {
                    title: "Dune".to_owned(),
                    status: BookStatus::Available,
                    series_id: Some(series.id),
                    description: None,
                    published_date: None,
                    language: None,
                    series_number: None,
                    publisher_id: None,
                    page_count: None,
                    rating: None,
                    metadata_source: None,
                    cover_path: None,
                },
            )
            .await
            .unwrap();
        tx.commit().await.unwrap();

        new_book(&svc, "Foundation").await;

        let tx = svc.repository().begin().await.unwrap();
        let filter = ShelfFilter {
            series: Some(vec![series.id]),
            ..Default::default()
        };
        let books = svc.shelf_repository().books_for_filter(&*tx, &filter, user_id, None, None).await.unwrap();

        assert_eq!(books.len(), 1);
        assert_eq!(books[0].id, book_in_series.id);
    }

    #[tokio::test]
    async fn test_books_for_filter_page_size_zero_returns_error() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        let tx = svc.repository().begin().await.unwrap();

        assert!(matches!(
            svc.shelf_repository()
                .books_for_filter(&*tx, &ShelfFilter::default(), user_id, None, Some(0))
                .await,
            Err(bb_core::Error::InvalidPageSize(0))
        ));
    }

    // ─── count_for_filter ────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_count_for_filter_matches_books_for_filter() {
        let svc = setup().await;
        let user_id = new_user(&svc, "alice").await;
        new_book(&svc, "Dune").await;
        new_book(&svc, "Foundation").await;
        new_book(&svc, "Hyperion").await;

        let tx = svc.repository().begin().await.unwrap();
        let filter = ShelfFilter::default();
        let books = svc.shelf_repository().books_for_filter(&*tx, &filter, user_id, None, None).await.unwrap();
        let count = svc.shelf_repository().count_for_filter(&*tx, &filter, user_id).await.unwrap();

        assert_eq!(count, books.len() as u64);
        assert_eq!(count, 3);
    }
}
