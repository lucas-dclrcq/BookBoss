use bb_core::{RepositoryError, book::BookStatus, repository::read_only_transaction};

use crate::{fixtures, setup};

/// Build a `CoreServices` backed by the same repos as `ctx` but with a
/// `SilentFileStore` so that `LibraryService::delete_book` can complete
/// without panicking on the no-op store.
fn library_services(ctx: &crate::context::TestContext) -> std::sync::Arc<bb_core::CoreServices> {
    bb_core::create_services(
        bb_core::test_support::default_external_services_builder()
            .repository_service(ctx.repos.clone())
            .file_store(fixtures::silent_file_store())
            .build()
            .unwrap(),
        "test-encryption-secret",
    )
    .unwrap()
}

#[tokio::test]
async fn delete_book_removes_book_record() {
    let ctx = setup().await;
    let svc = library_services(&ctx);
    let book = fixtures::insert_book(&ctx.repos, "To Delete", BookStatus::Available).await;

    svc.library_service.delete_book(book.token).await.unwrap();

    let found = svc.book_service.find_book_by_token(book.token).await.unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn delete_book_removes_linked_import_job() {
    let ctx = setup().await;
    let svc = library_services(&ctx);
    let book = fixtures::insert_book(&ctx.repos, "Book With Job", BookStatus::Available).await;
    let job = fixtures::insert_import_job(&ctx.repos, "job_hash_for_delete").await;
    let job = fixtures::link_job_to_book(&ctx.repos, job, book.id).await;

    svc.library_service.delete_book(book.token).await.unwrap();

    let found_job = fixtures::find_job_by_id(&ctx.repos, job.id).await;
    assert!(found_job.is_none(), "import job should be deleted with the book");
}

#[tokio::test]
async fn delete_book_removes_orphan_author() {
    let ctx = setup().await;
    let svc = library_services(&ctx);
    let book = fixtures::insert_book(&ctx.repos, "Only Book By Author", BookStatus::Available).await;
    let author = fixtures::insert_author(&ctx.repos, "Orphan Author").await;
    fixtures::link_book_author(&ctx.repos, book.id, author.id).await;

    svc.library_service.delete_book(book.token).await.unwrap();

    // Author should be cleaned up because they have no other books
    let author_repo = ctx.repos.author_repository().clone();
    let found = read_only_transaction(&**ctx.repos.repository(), |tx| {
        let author_repo = author_repo.clone();
        Box::pin(async move { author_repo.find_by_id(tx, author.id).await })
    })
    .await
    .unwrap();
    assert!(found.is_none(), "orphan author should be deleted");
}

#[tokio::test]
async fn delete_book_preserves_author_with_other_books() {
    let ctx = setup().await;
    let svc = library_services(&ctx);
    let book1 = fixtures::insert_book(&ctx.repos, "Book One", BookStatus::Available).await;
    let book2 = fixtures::insert_book(&ctx.repos, "Book Two", BookStatus::Available).await;
    let author = fixtures::insert_author(&ctx.repos, "Shared Author").await;
    fixtures::link_book_author(&ctx.repos, book1.id, author.id).await;
    fixtures::link_book_author(&ctx.repos, book2.id, author.id).await;

    svc.library_service.delete_book(book1.token).await.unwrap();

    // Author still has book2 — should not be deleted
    let author_repo = ctx.repos.author_repository().clone();
    let found = read_only_transaction(&**ctx.repos.repository(), |tx| {
        let author_repo = author_repo.clone();
        Box::pin(async move { author_repo.find_by_id(tx, author.id).await })
    })
    .await
    .unwrap();
    assert!(found.is_some(), "shared author should be preserved");
}

#[tokio::test]
async fn delete_book_returns_not_found_for_missing_token() {
    let ctx = setup().await;
    let svc = library_services(&ctx);
    let ghost_token = bb_core::book::BookToken::new(999_999);

    let result = svc.library_service.delete_book(ghost_token).await;

    assert!(
        matches!(result, Err(bb_core::Error::RepositoryError(RepositoryError::NotFound))),
        "expected NotFound, got: {result:?}"
    );
}

#[tokio::test]
async fn library_stats_counts_authors() {
    let ctx = setup().await;
    let book = fixtures::insert_book(&ctx.repos, "Some Book", BookStatus::Available).await;
    let author = fixtures::insert_author(&ctx.repos, "Some Author").await;
    fixtures::link_book_author(&ctx.repos, book.id, author.id).await;

    let stats = ctx.services.library_service.library_stats().await.unwrap();

    assert_eq!(stats.authors, 1);
}
