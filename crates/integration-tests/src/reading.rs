use bb_core::{
    Error,
    book::BookStatus,
    reading::{ReadStatus, service::DEFAULT_AUTO_READ_THRESHOLD},
};

use crate::{fixtures, setup};

// ── Helpers
// ──────────────────────────────────────────────────────────────────────

async fn setup_user_and_book(ctx: &crate::context::TestContext) -> (u64, u64) {
    let user = fixtures::insert_user(&ctx.repos, "reader").await;
    let book = fixtures::insert_book(&ctx.repos, "Dune", BookStatus::Available).await;
    (user.id, book.id)
}

// ── get_reading_state
// ───────────────────────────────────────────────────────────

#[tokio::test]
async fn get_reading_state_returns_none_before_any_interaction() {
    let ctx = setup().await;
    let (user_id, book_id) = setup_user_and_book(&ctx).await;

    let state = ctx.services.reading_service.get_reading_state(user_id, book_id).await.unwrap();

    assert!(state.is_none(), "no row should exist before the user interacts with the book");
}

// ── set_status
// ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn set_status_reading_creates_initial_record() {
    let ctx = setup().await;
    let (user_id, book_id) = setup_user_and_book(&ctx).await;

    let state = ctx.services.reading_service.set_status(user_id, book_id, ReadStatus::Reading).await.unwrap();

    assert_eq!(state.read_status, ReadStatus::Reading);
    assert!(state.date_started.is_some(), "date_started should be set when transitioning to Reading");
}

#[tokio::test]
async fn set_status_read_increments_times_read() {
    let ctx = setup().await;
    let (user_id, book_id) = setup_user_and_book(&ctx).await;
    ctx.services.reading_service.set_status(user_id, book_id, ReadStatus::Reading).await.unwrap();

    let state = ctx.services.reading_service.set_status(user_id, book_id, ReadStatus::Read).await.unwrap();

    assert_eq!(state.read_status, ReadStatus::Read);
    assert_eq!(state.times_read, 1, "times_read must be incremented when transitioning to Read");
    assert!(state.date_finished.is_some(), "date_finished should be set on Read transition");
}

#[tokio::test]
async fn set_status_is_persisted_and_retrievable() {
    let ctx = setup().await;
    let (user_id, book_id) = setup_user_and_book(&ctx).await;
    ctx.services.reading_service.set_status(user_id, book_id, ReadStatus::Paused).await.unwrap();

    let state = ctx.services.reading_service.get_reading_state(user_id, book_id).await.unwrap();

    assert_eq!(state.map(|s| s.read_status), Some(ReadStatus::Paused));
}

// ── update_progress
// ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn update_progress_transitions_unread_to_reading() {
    let ctx = setup().await;
    let (user_id, book_id) = setup_user_and_book(&ctx).await;

    let state = ctx
        .services
        .reading_service
        .update_progress(user_id, book_id, 500, None, Some(DEFAULT_AUTO_READ_THRESHOLD))
        .await
        .unwrap();

    assert_eq!(
        state.read_status,
        ReadStatus::Reading,
        "first progress update should auto-transition to Reading"
    );
    assert_eq!(state.progress_percentage, Some(500));
}

#[tokio::test]
async fn update_progress_auto_advances_to_read_at_threshold() {
    let ctx = setup().await;
    let (user_id, book_id) = setup_user_and_book(&ctx).await;
    ctx.services.reading_service.set_status(user_id, book_id, ReadStatus::Reading).await.unwrap();

    let state = ctx
        .services
        .reading_service
        .update_progress(user_id, book_id, DEFAULT_AUTO_READ_THRESHOLD, None, Some(DEFAULT_AUTO_READ_THRESHOLD))
        .await
        .unwrap();

    assert_eq!(state.read_status, ReadStatus::Read, "reaching the threshold should auto-advance to Read");
    assert_eq!(state.times_read, 1);
}

#[tokio::test]
async fn update_progress_no_auto_advance_when_threshold_disabled() {
    let ctx = setup().await;
    let (user_id, book_id) = setup_user_and_book(&ctx).await;
    ctx.services.reading_service.set_status(user_id, book_id, ReadStatus::Reading).await.unwrap();

    // Passing None disables auto-advance
    let state = ctx
        .services
        .reading_service
        .update_progress(user_id, book_id, 10_000, None, None)
        .await
        .unwrap();

    assert_eq!(state.read_status, ReadStatus::Reading, "auto-advance disabled — should stay Reading at 100%");
}

#[tokio::test]
async fn update_progress_stores_position_token() {
    let ctx = setup().await;
    let (user_id, book_id) = setup_user_and_book(&ctx).await;

    let state = ctx
        .services
        .reading_service
        .update_progress(user_id, book_id, 1000, Some("epubcfi(/6/4!/4/2/2:0)".to_string()), None)
        .await
        .unwrap();

    assert_eq!(state.position_token.as_deref(), Some("epubcfi(/6/4!/4/2/2:0)"));
}

// ── set_rating / clear_rating
// ───────────────────────────────────────────────────

#[tokio::test]
async fn set_rating_stores_rating() {
    let ctx = setup().await;
    let (user_id, book_id) = setup_user_and_book(&ctx).await;

    let state = ctx.services.reading_service.set_rating(user_id, book_id, 4).await.unwrap();

    assert_eq!(state.personal_rating, Some(4));
}

#[tokio::test]
async fn set_rating_rejects_zero() {
    let ctx = setup().await;
    let (user_id, book_id) = setup_user_and_book(&ctx).await;

    let result = ctx.services.reading_service.set_rating(user_id, book_id, 0).await;

    assert!(matches!(result, Err(Error::Validation(_))), "rating 0 must be rejected");
}

#[tokio::test]
async fn set_rating_rejects_above_five() {
    let ctx = setup().await;
    let (user_id, book_id) = setup_user_and_book(&ctx).await;

    let result = ctx.services.reading_service.set_rating(user_id, book_id, 6).await;

    assert!(matches!(result, Err(Error::Validation(_))), "rating > 5 must be rejected");
}

#[tokio::test]
async fn clear_rating_removes_stored_rating() {
    let ctx = setup().await;
    let (user_id, book_id) = setup_user_and_book(&ctx).await;
    ctx.services.reading_service.set_rating(user_id, book_id, 5).await.unwrap();

    let state = ctx.services.reading_service.clear_rating(user_id, book_id).await.unwrap();

    assert_eq!(state.personal_rating, None, "rating should be cleared");
}

// ── set_notes
// ───────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn set_notes_stores_and_retrieves_notes() {
    let ctx = setup().await;
    let (user_id, book_id) = setup_user_and_book(&ctx).await;

    let state = ctx
        .services
        .reading_service
        .set_notes(user_id, book_id, "A timeless classic".to_string())
        .await
        .unwrap();

    assert_eq!(state.notes.as_deref(), Some("A timeless classic"));
}

// ── list_for_user
// ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_for_user_returns_all_reading_state() {
    let ctx = setup().await;
    let user = fixtures::insert_user(&ctx.repos, "avid_reader").await;
    let book1 = fixtures::insert_book(&ctx.repos, "Book One", BookStatus::Available).await;
    let book2 = fixtures::insert_book(&ctx.repos, "Book Two", BookStatus::Available).await;
    ctx.services.reading_service.set_status(user.id, book1.id, ReadStatus::Reading).await.unwrap();
    ctx.services.reading_service.set_status(user.id, book2.id, ReadStatus::Read).await.unwrap();

    let all = ctx.services.reading_service.list_for_user(user.id, None).await.unwrap();

    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn list_for_user_filters_by_status() {
    let ctx = setup().await;
    let user = fixtures::insert_user(&ctx.repos, "selective_reader").await;
    let book1 = fixtures::insert_book(&ctx.repos, "Currently Reading", BookStatus::Available).await;
    let book2 = fixtures::insert_book(&ctx.repos, "Already Read", BookStatus::Available).await;
    ctx.services.reading_service.set_status(user.id, book1.id, ReadStatus::Reading).await.unwrap();
    ctx.services.reading_service.set_status(user.id, book2.id, ReadStatus::Read).await.unwrap();

    let reading = ctx.services.reading_service.list_for_user(user.id, Some(ReadStatus::Reading)).await.unwrap();

    assert_eq!(reading.len(), 1);
    assert_eq!(reading[0].read_status, ReadStatus::Reading);
    assert_eq!(reading[0].book_id, book1.id);
}

#[tokio::test]
async fn list_for_user_excludes_other_users_state() {
    let ctx = setup().await;
    let alice = fixtures::insert_user(&ctx.repos, "alice_reads").await;
    let bob = fixtures::insert_user(&ctx.repos, "bob_reads").await;
    let book = fixtures::insert_book(&ctx.repos, "Shared Book", BookStatus::Available).await;
    ctx.services.reading_service.set_status(alice.id, book.id, ReadStatus::Reading).await.unwrap();
    ctx.services.reading_service.set_status(bob.id, book.id, ReadStatus::Read).await.unwrap();

    let alice_list = ctx.services.reading_service.list_for_user(alice.id, None).await.unwrap();

    assert_eq!(alice_list.len(), 1);
    assert_eq!(alice_list[0].user_id, alice.id);
}
