use bb_core::book::{BookQuery, BookStatus};

use crate::{fixtures, setup};

#[tokio::test]
async fn book_created_and_found_by_token() {
    let ctx = setup().await;
    let book = fixtures::insert_book(&ctx.repos, "Dune", BookStatus::Available).await;

    let found = ctx.services.book_service.find_book_by_token(book.token).await.unwrap();

    assert!(found.is_some());
    assert_eq!(found.unwrap().title, "Dune");
}

#[tokio::test]
async fn find_book_by_token_returns_none_for_unknown_token() {
    let ctx = setup().await;
    let fake_token = bb_core::book::BookToken::new(999_999);

    let found = ctx.services.book_service.find_book_by_token(fake_token).await.unwrap();

    assert!(found.is_none());
}

#[tokio::test]
async fn list_books_returns_empty_initially() {
    let ctx = setup().await;
    let filter = BookQuery::default();

    let books = ctx.services.book_service.list_books(&filter, None, None, None).await.unwrap();

    assert!(books.is_empty());
}

#[tokio::test]
async fn list_books_filters_by_status() {
    let ctx = setup().await;
    fixtures::insert_book(&ctx.repos, "Available Book", BookStatus::Available).await;
    fixtures::insert_book(&ctx.repos, "Incoming Book", BookStatus::Incoming).await;

    let filter = BookQuery::default();
    let books = ctx.services.book_service.list_books(&filter, None, None, None).await.unwrap();

    assert_eq!(books.len(), 1);
    assert_eq!(books[0].title, "Available Book");
}

#[tokio::test]
async fn list_books_returns_all_available_without_filter() {
    let ctx = setup().await;
    fixtures::insert_book(&ctx.repos, "Book A", BookStatus::Available).await;
    fixtures::insert_book(&ctx.repos, "Book B", BookStatus::Available).await;
    fixtures::insert_book(&ctx.repos, "Book C", BookStatus::Incoming).await;

    let books = ctx.services.book_service.list_books(&BookQuery::default(), None, None, None).await.unwrap();

    assert_eq!(books.len(), 2);
}

#[tokio::test]
async fn authors_for_book_returns_linked_author() {
    let ctx = setup().await;
    let book = fixtures::insert_book(&ctx.repos, "Foundation", BookStatus::Available).await;
    let author = fixtures::insert_author(&ctx.repos, "Isaac Asimov").await;
    fixtures::link_book_author(&ctx.repos, book.id, author.id).await;

    let authors = ctx.services.book_service.authors_for_book(book.id).await.unwrap();

    assert_eq!(authors.len(), 1);
    assert_eq!(authors[0].author_id, author.id);
}

#[tokio::test]
async fn list_authors_returns_inserted_author() {
    let ctx = setup().await;
    fixtures::insert_author(&ctx.repos, "Ursula K. Le Guin").await;

    let authors = ctx.services.book_service.list_authors(None, None).await.unwrap();

    assert_eq!(authors.len(), 1);
    assert_eq!(authors[0].name, "Ursula K. Le Guin");
}
