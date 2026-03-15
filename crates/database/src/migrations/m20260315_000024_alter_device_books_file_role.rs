use sea_orm_migration::{
    prelude::*,
    schema::{big_integer, string},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(Table::alter().table(DeviceBooks::Table).drop_column(DeviceBooks::BookFileId).to_owned())
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(DeviceBooks::Table)
                    .add_column(string(DeviceBooks::FileRole).not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(Table::alter().table(DeviceBooks::Table).drop_column(DeviceBooks::FileRole).to_owned())
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
}

#[derive(DeriveIden)]
enum DeviceBooks {
    Table,
    BookFileId,
    FileRole,
}
