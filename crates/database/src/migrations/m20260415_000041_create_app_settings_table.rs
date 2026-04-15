use sea_orm_migration::{
    prelude::*,
    schema::{string, text, timestamp_with_time_zone},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AppSettings::Table)
                    .if_not_exists()
                    .col(string(AppSettings::Key).not_null().primary_key())
                    .col(text(AppSettings::Value).not_null())
                    .col(timestamp_with_time_zone(AppSettings::UpdatedAt).not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[derive(DeriveIden)]
enum AppSettings {
    Table,
    Key,
    Value,
    UpdatedAt,
}
