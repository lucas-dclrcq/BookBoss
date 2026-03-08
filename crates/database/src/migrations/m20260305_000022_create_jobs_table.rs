use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Jobs::Table)
                    .if_not_exists()
                    .col(big_integer(Jobs::Id).primary_key().auto_increment())
                    .col(string(Jobs::JobType))
                    .col(json_binary(Jobs::Payload))
                    .col(string(Jobs::Status))
                    .col(small_integer(Jobs::Priority))
                    .col(small_integer(Jobs::Attempt))
                    .col(small_integer(Jobs::MaxAttempts))
                    .col(integer(Jobs::Version))
                    .col(timestamp_with_time_zone(Jobs::ScheduledAt))
                    .col(timestamp_with_time_zone(Jobs::StartedAt).null())
                    .col(timestamp_with_time_zone(Jobs::CompletedAt).null())
                    .col(text(Jobs::ErrorMessage).null())
                    .col(timestamp_with_time_zone(Jobs::CreatedAt))
                    .col(timestamp_with_time_zone(Jobs::UpdatedAt))
                    .to_owned(),
            )
            .await?;

        // Partial index for the claim query — not expressible via SeaORM DSL.
        // MySQL does not support partial indexes, so we create a regular covering index there.
        let create_index = match manager.get_database_backend() {
            sea_orm::DatabaseBackend::MySql => {
                "CREATE INDEX jobs_claim ON jobs (status, priority DESC, scheduled_at ASC)"
            }
            _ => "CREATE INDEX jobs_claim ON jobs (priority DESC, scheduled_at ASC) WHERE status = 'pending'",
        };
        manager.get_connection().execute_unprepared(create_index).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let drop_index = match manager.get_database_backend() {
            sea_orm::DatabaseBackend::MySql => "DROP INDEX IF EXISTS jobs_claim ON jobs",
            _ => "DROP INDEX IF EXISTS jobs_claim",
        };
        manager.get_connection().execute_unprepared(drop_index).await?;

        manager.drop_table(Table::drop().table(Jobs::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Jobs {
    Table,
    Id,
    JobType,
    Payload,
    Status,
    Priority,
    Attempt,
    MaxAttempts,
    Version,
    ScheduledAt,
    StartedAt,
    CompletedAt,
    ErrorMessage,
    CreatedAt,
    UpdatedAt,
}
