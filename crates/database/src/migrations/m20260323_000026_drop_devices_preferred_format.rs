use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(Table::alter().table(Devices::Table).drop_column(Devices::PreferredFormat).to_owned())
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Devices::Table)
                    .add_column(ColumnDef::new(Devices::PreferredFormat).string().null())
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Devices {
    Table,
    PreferredFormat,
}
