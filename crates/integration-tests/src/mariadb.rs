use bb_database::create_repository_service;
use sea_orm::Database;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::mariadb::Mariadb;

use crate::context::TestContext;

pub async fn setup() -> TestContext {
    let container = Mariadb::default().start().await.unwrap();
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(3306).await.unwrap();
    let url = format!("mysql://root@{host}:{port}/mysql");

    let db = Database::connect(&url).await.unwrap();
    let repository_service = create_repository_service(db).await.unwrap();
    let core_services = bb_core::create_services(
        bb_core::test_support::default_external_services_builder()
            .repository_service(repository_service.clone())
            .build()
            .unwrap(),
        "test-encryption-secret",
    )
    .unwrap();

    TestContext::new(core_services, repository_service, container)
}
