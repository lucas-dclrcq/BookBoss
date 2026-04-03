use sea_orm_migration::prelude::*;

// All Books token = LibraryToken::new(1_u64).to_string() = "LB_YYYYYYYYYYYY4"
// Deterministic from id=1 via bb_utils::token base-32 encoding.
// Verified by test `known_value_encoding` in crates/utils/src/token.rs.
const ALL_BOOKS_LIBRARY_TOKEN: &str = "LB_YYYYYYYYYYYY4";

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite uses INSERT OR IGNORE; Postgres/MySQL use INSERT ... ON CONFLICT DO
        // NOTHING.
        let sql = if manager.get_database_backend() == sea_orm::DatabaseBackend::Sqlite {
            format!(
                "INSERT OR IGNORE INTO user_settings (user_id, key, value, created_at, updated_at) SELECT id, 'default_library', '{ALL_BOOKS_LIBRARY_TOKEN}', \
                 CURRENT_TIMESTAMP, CURRENT_TIMESTAMP FROM users"
            )
        } else {
            format!(
                "INSERT INTO user_settings (user_id, key, value, created_at, updated_at) SELECT id, 'default_library', '{ALL_BOOKS_LIBRARY_TOKEN}', \
                 CURRENT_TIMESTAMP, CURRENT_TIMESTAMP FROM users ON CONFLICT (user_id, key) DO NOTHING"
            )
        };
        manager.get_connection().execute_unprepared(&sql).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DELETE FROM user_settings WHERE key = 'default_library'")
            .await?;
        Ok(())
    }
}
