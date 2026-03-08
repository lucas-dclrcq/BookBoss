use bb_core::{Error, book::BookStatus, import::ImportStatus};

use crate::{fixtures, setup};

#[tokio::test]
async fn list_pending_returns_empty_initially() {
    let ctx = setup().await;

    let jobs = ctx.services.import_job_service.list_pending(None, None).await.unwrap();

    assert!(jobs.is_empty());
}

#[tokio::test]
async fn job_created_and_found_by_token() {
    let ctx = setup().await;
    let job = fixtures::insert_import_job(&ctx.repos, "abc123").await;

    let found = ctx.services.import_job_service.find_by_token(&job.token).await.unwrap();

    assert!(found.is_some());
    assert_eq!(found.unwrap().file_hash, "abc123");
}

#[tokio::test]
async fn find_by_token_returns_none_for_unknown_token() {
    let ctx = setup().await;
    let fake_token = bb_core::import::ImportJobToken::new(999_999);

    let found = ctx.services.import_job_service.find_by_token(&fake_token).await.unwrap();

    assert!(found.is_none());
}

#[tokio::test]
async fn list_pending_returns_pending_job() {
    let ctx = setup().await;
    fixtures::insert_import_job(&ctx.repos, "pending_hash").await;

    let jobs = ctx.services.import_job_service.list_pending(None, None).await.unwrap();

    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].status, ImportStatus::Pending);
}

#[tokio::test]
async fn list_needs_review_returns_only_review_jobs() {
    let ctx = setup().await;
    let pending = fixtures::insert_import_job(&ctx.repos, "pending_hash").await;
    let review = fixtures::insert_import_job(&ctx.repos, "review_hash").await;
    fixtures::set_job_status(&ctx.repos, review, ImportStatus::NeedsReview).await;

    let pending_jobs = ctx.services.import_job_service.list_pending(None, None).await.unwrap();
    let review_jobs = ctx.services.import_job_service.list_needs_review(None, None).await.unwrap();

    // pending job was never transitioned so it stays Pending
    let _ = pending;
    assert_eq!(pending_jobs.len(), 1);
    assert_eq!(review_jobs.len(), 1);
    assert_eq!(review_jobs[0].status, ImportStatus::NeedsReview);
}

#[tokio::test]
async fn approve_job_transitions_to_approved() {
    let ctx = setup().await;
    let user = fixtures::insert_user(&ctx.repos, "reviewer").await;
    let job = fixtures::insert_import_job(&ctx.repos, "approve_hash").await;
    let job = fixtures::set_job_status(&ctx.repos, job, ImportStatus::NeedsReview).await;

    let approved = ctx.services.import_job_service.approve_job(job, user.id).await.unwrap();

    assert_eq!(approved.status, ImportStatus::Approved);
    assert_eq!(approved.reviewed_by, Some(user.id));
}

#[tokio::test]
async fn approve_job_fails_when_not_needs_review() {
    let ctx = setup().await;
    let job = fixtures::insert_import_job(&ctx.repos, "bad_status_hash").await;
    // job is Pending, not NeedsReview

    let result = ctx.services.import_job_service.approve_job(job, 1).await;

    assert!(matches!(result, Err(Error::Validation(_))));
}

#[tokio::test]
async fn reject_job_transitions_to_rejected() {
    let ctx = setup().await;
    let user = fixtures::insert_user(&ctx.repos, "reviewer2").await;
    let job = fixtures::insert_import_job(&ctx.repos, "reject_hash").await;
    let job = fixtures::set_job_status(&ctx.repos, job, ImportStatus::NeedsReview).await;

    let rejected = ctx.services.import_job_service.reject_job(job, user.id).await.unwrap();

    assert_eq!(rejected.status, ImportStatus::Rejected);
}

#[tokio::test]
async fn library_stats_counts_available_books() {
    let ctx = setup().await;
    fixtures::insert_book(&ctx.repos, "Available One", BookStatus::Available).await;
    fixtures::insert_book(&ctx.repos, "Incoming One", BookStatus::Incoming).await;

    let stats = ctx.services.library_service.library_stats().await.unwrap();

    // Only Available books count
    assert_eq!(stats.books, 1);
}
