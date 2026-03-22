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
                    .table(SystemMessages::Table)
                    .if_not_exists()
                    .col(big_integer(SystemMessages::Id).primary_key().auto_increment())
                    .col(string(SystemMessages::SourceTask))
                    .col(string(SystemMessages::Severity))
                    .col(text(SystemMessages::Message))
                    .col(timestamp_with_time_zone(SystemMessages::CreatedAt))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("system_messages_created_at")
                    .table(SystemMessages::Table)
                    .col((SystemMessages::CreatedAt, IndexOrder::Desc))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(Index::drop().name("system_messages_created_at").table(SystemMessages::Table).to_owned())
            .await?;

        manager.drop_table(Table::drop().table(SystemMessages::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum SystemMessages {
    Table,
    Id,
    SourceTask,
    Severity,
    Message,
    CreatedAt,
}
