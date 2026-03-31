use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(Index::create().name("idx_jobs_job_type").table(Jobs::Table).col(Jobs::JobType).to_owned())
            .await?;

        manager
            .create_index(Index::create().name("idx_jobs_updated_at").table(Jobs::Table).col(Jobs::UpdatedAt).to_owned())
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_index(Index::drop().name("idx_jobs_updated_at").to_owned()).await?;
        manager.drop_index(Index::drop().name("idx_jobs_job_type").to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Jobs {
    Table,
    JobType,
    UpdatedAt,
}
