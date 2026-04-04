use sea_orm_migration::{
    prelude::*,
    schema::{big_integer, string, text, timestamp_with_time_zone},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Authors::Table)
                    .if_not_exists()
                    .col(big_integer(Authors::Id).primary_key())
                    .col(string(Authors::Token).unique_key())
                    .col(string(Authors::Name))
                    .col(text(Authors::Bio).null())
                    .col(big_integer(Authors::Version))
                    .col(timestamp_with_time_zone(Authors::CreatedAt))
                    .col(timestamp_with_time_zone(Authors::UpdatedAt))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Authors {
    Table,
    Id,
    Token,
    Name,
    Bio,
    Version,
    CreatedAt,
    UpdatedAt,
}
