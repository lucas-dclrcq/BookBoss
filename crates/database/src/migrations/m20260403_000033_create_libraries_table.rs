use sea_orm_migration::{
    prelude::*,
    schema::{big_integer, boolean, string, timestamp_with_time_zone},
};

// All Books token = LibraryToken::new(1_u64).to_string() = "LB_YYYYYYYYYYYY4"
// Deterministic from id=1 via bb_utils::token base-32 encoding.
// Verified by test `known_value_encoding` in crates/utils/src/token.rs.
const ALL_BOOKS_LIBRARY_TOKEN: &str = "LB_YYYYYYYYYYYY4";

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Libraries::Table)
                    .if_not_exists()
                    .col(big_integer(Libraries::Id).primary_key())
                    .col(string(Libraries::Token).unique_key())
                    .col(ColumnDef::new(Libraries::Version).big_integer().not_null())
                    .col(string(Libraries::Name).unique_key())
                    .col(boolean(Libraries::IsSystem).not_null().default(false))
                    .col(timestamp_with_time_zone(Libraries::CreatedAt))
                    .col(timestamp_with_time_zone(Libraries::UpdatedAt))
                    .to_owned(),
            )
            .await?;

        // Seed "All Books" system library
        manager
            .get_connection()
            .execute_unprepared(&format!(
                "INSERT INTO libraries (id, token, version, name, is_system, created_at, updated_at) VALUES (1, '{ALL_BOOKS_LIBRARY_TOKEN}', 1, 'All Books', \
                 true, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)"
            ))
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Libraries::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Libraries {
    Table,
    Id,
    Token,
    Version,
    Name,
    IsSystem,
    CreatedAt,
    UpdatedAt,
}
