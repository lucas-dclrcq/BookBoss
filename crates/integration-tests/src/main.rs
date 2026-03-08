mod book;
mod context;
mod fixtures;
mod import_job;
mod library;

#[cfg(feature = "postgres")]
mod postgres;

#[cfg(feature = "sqlite")]
#[cfg_attr(feature = "postgres", allow(dead_code))]
mod sqlite;

#[cfg(feature = "mariadb")]
#[cfg_attr(feature = "postgres", allow(dead_code))]
mod mariadb;

#[cfg(feature = "mysql")]
#[cfg_attr(feature = "postgres", allow(dead_code))]
mod mysql;

// Priority: postgres > mysql > sqlite when multiple features are enabled.
#[cfg(all(feature = "mariadb", not(feature = "postgres")))]
pub(crate) use mariadb::setup;
#[cfg(all(feature = "mysql", not(feature = "postgres")))]
pub(crate) use mysql::setup;
#[cfg(feature = "postgres")]
pub(crate) use postgres::setup;
#[cfg(all(feature = "sqlite", not(feature = "postgres"), not(feature = "mysql"), not(feature = "mariadb")))]
pub(crate) use sqlite::setup;

#[cfg(not(any(feature = "postgres", feature = "sqlite", feature = "mysql", feature = "mariadb")))]
compile_error!("At least one database backend feature must be enabled: postgres, sqlite, or mysql");

#[tokio::test]
async fn test_setup() {
    let _ctx = setup().await;
}
