use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name("idx_import_jobs_updated_at")
                    .table(ImportJobs::Table)
                    .col(ImportJobs::UpdatedAt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_index(Index::drop().name("idx_import_jobs_updated_at").to_owned()).await
    }
}

#[derive(DeriveIden)]
enum ImportJobs {
    Table,
    UpdatedAt,
}
