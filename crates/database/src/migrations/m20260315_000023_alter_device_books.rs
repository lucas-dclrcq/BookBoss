use sea_orm_migration::{
    prelude::*,
    schema::{big_integer, timestamp_with_time_zone},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(Table::alter().table(DeviceBooks::Table).drop_column(DeviceBooks::RemovedAt).to_owned())
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(DeviceBooks::Table)
                    .add_column(big_integer(DeviceBooks::BookFileId).not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(Table::alter().table(DeviceBooks::Table).drop_column(DeviceBooks::BookFileId).to_owned())
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(DeviceBooks::Table)
                    .add_column(timestamp_with_time_zone(DeviceBooks::RemovedAt).null())
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum DeviceBooks {
    Table,
    RemovedAt,
    BookFileId,
}
