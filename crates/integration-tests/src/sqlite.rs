use bb_database::create_repository_service;
use sea_orm::Database;

use crate::context::TestContext;

pub async fn setup() -> TestContext {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let repository_service = create_repository_service(db).await.unwrap();
    let core_services = bb_core::create_services(
        bb_core::test_support::default_external_services_builder()
            .repository_service(repository_service.clone())
            .build()
            .unwrap(),
        "test-encryption-secret",
    )
    .unwrap();

    TestContext::new(core_services, repository_service, ())
}
