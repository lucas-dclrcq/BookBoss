use bb_core::{Error, RepositoryError, device::OnRemovalAction};

use crate::{fixtures, setup};

// ── create_device
// ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn create_device_returns_token() {
    let ctx = setup().await;
    let user = fixtures::insert_user(&ctx.repos, "device_owner").await;

    let token = ctx
        .services
        .device_service
        .create_device(user.id, "My Kobo".to_string(), "kobo".to_string(), OnRemovalAction::Nothing)
        .await
        .unwrap();

    let device = ctx.services.device_service.get_device(token, user.id).await.unwrap();
    assert_eq!(device.name, "My Kobo");
    assert_eq!(device.device_type, "kobo");
    assert_eq!(device.owner_id, user.id);
}

#[tokio::test]
async fn create_device_creates_companion_shelf() {
    let ctx = setup().await;
    let user = fixtures::insert_user(&ctx.repos, "kobo_user").await;

    let token = ctx
        .services
        .device_service
        .create_device(user.id, "My Kobo".to_string(), "kobo".to_string(), OnRemovalAction::Nothing)
        .await
        .unwrap();

    let device = ctx.services.device_service.get_device(token, user.id).await.unwrap();
    let companion = ctx.services.device_service.get_companion_shelf(device.id).await.unwrap();
    assert!(companion.is_some(), "companion shelf must be created with the device");
    let shelf = companion.unwrap();
    assert_eq!(shelf.name, "My Kobo");
}

// ── list_devices_for_user
// ───────────────────────────────────────────────────────

#[tokio::test]
async fn list_devices_for_user_empty_initially() {
    let ctx = setup().await;
    let user = fixtures::insert_user(&ctx.repos, "no_devices_user").await;

    let devices = ctx.services.device_service.list_devices_for_user(user.id).await.unwrap();

    assert!(devices.is_empty());
}

#[tokio::test]
async fn list_devices_for_user_returns_created_devices() {
    let ctx = setup().await;
    let user = fixtures::insert_user(&ctx.repos, "multi_device_user").await;
    ctx.services
        .device_service
        .create_device(user.id, "Kobo".to_string(), "kobo".to_string(), OnRemovalAction::Nothing)
        .await
        .unwrap();
    ctx.services
        .device_service
        .create_device(user.id, "Kindle".to_string(), "kindle".to_string(), OnRemovalAction::MarkRead)
        .await
        .unwrap();

    let devices = ctx.services.device_service.list_devices_for_user(user.id).await.unwrap();

    assert_eq!(devices.len(), 2);
}

#[tokio::test]
async fn list_devices_excludes_other_users_devices() {
    let ctx = setup().await;
    let alice = fixtures::insert_user(&ctx.repos, "alice_device").await;
    let bob = fixtures::insert_user(&ctx.repos, "bob_device").await;
    ctx.services
        .device_service
        .create_device(alice.id, "Alice's Kobo".to_string(), "kobo".to_string(), OnRemovalAction::Nothing)
        .await
        .unwrap();

    let bob_devices = ctx.services.device_service.list_devices_for_user(bob.id).await.unwrap();

    assert!(bob_devices.is_empty(), "Bob should not see Alice's device");
}

// ── get_device
// ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn get_device_fails_for_other_user() {
    let ctx = setup().await;
    let alice = fixtures::insert_user(&ctx.repos, "alice_get").await;
    let bob = fixtures::insert_user(&ctx.repos, "bob_get").await;
    let token = ctx
        .services
        .device_service
        .create_device(alice.id, "Alice's Device".to_string(), "kobo".to_string(), OnRemovalAction::Nothing)
        .await
        .unwrap();

    let result = ctx.services.device_service.get_device(token, bob.id).await;

    assert!(
        matches!(result, Err(Error::RepositoryError(RepositoryError::NotFound))),
        "must not expose another user's device"
    );
}

// ── update_device
// ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn update_device_renames_device_and_companion_shelf() {
    let ctx = setup().await;
    let user = fixtures::insert_user(&ctx.repos, "renamer_user").await;
    let token = ctx
        .services
        .device_service
        .create_device(user.id, "Old Name".to_string(), "kobo".to_string(), OnRemovalAction::Nothing)
        .await
        .unwrap();
    let device = ctx.services.device_service.get_device(token, user.id).await.unwrap();

    ctx.services
        .device_service
        .update_device(token, "New Name".to_string(), OnRemovalAction::MarkRead, user.id)
        .await
        .unwrap();

    let updated = ctx.services.device_service.get_device(token, user.id).await.unwrap();
    assert_eq!(updated.name, "New Name");
    assert_eq!(updated.on_removal_action, OnRemovalAction::MarkRead);
    // Companion shelf should also be renamed
    let companion = ctx.services.device_service.get_companion_shelf(device.id).await.unwrap();
    assert_eq!(companion.map(|s| s.name), Some("New Name".to_string()));
}

// ── delete_device
// ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn delete_device_removes_it_from_list() {
    let ctx = setup().await;
    let user = fixtures::insert_user(&ctx.repos, "deleter_user").await;
    let token = ctx
        .services
        .device_service
        .create_device(user.id, "To Delete".to_string(), "kobo".to_string(), OnRemovalAction::Nothing)
        .await
        .unwrap();

    ctx.services.device_service.delete_device(token, true, user.id).await.unwrap();

    let devices = ctx.services.device_service.list_devices_for_user(user.id).await.unwrap();
    assert!(devices.is_empty(), "device should be gone after deletion");
}

#[tokio::test]
async fn delete_device_with_companion_shelf_removes_shelf() {
    let ctx = setup().await;
    let user = fixtures::insert_user(&ctx.repos, "shelf_deleter_user").await;
    let token = ctx
        .services
        .device_service
        .create_device(user.id, "Temp Device".to_string(), "kobo".to_string(), OnRemovalAction::Nothing)
        .await
        .unwrap();
    let device = ctx.services.device_service.get_device(token, user.id).await.unwrap();
    let device_id = device.id;

    ctx.services.device_service.delete_device(token, true, user.id).await.unwrap();

    let companion = ctx.services.device_service.get_companion_shelf(device_id).await.unwrap();
    assert!(companion.is_none(), "companion shelf should be deleted when delete_companion_shelf=true");
}

// ── default_device_name
// ─────────────────────────────────────────────────────────

#[tokio::test]
async fn default_device_name_uses_user_first_name() {
    let ctx = setup().await;
    let user = fixtures::insert_user(&ctx.repos, "alice_name").await;

    let name = ctx.services.device_service.default_device_name(user.id).await.unwrap();

    // insert_user creates a user with full_name="Test User", so first word is
    // "Test"
    assert!(name.contains("Test"), "default name should include user's first name; got: {name}");
}
