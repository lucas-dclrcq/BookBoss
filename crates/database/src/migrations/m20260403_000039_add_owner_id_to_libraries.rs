use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Add the nullable owner_id column. For Postgres / MySQL we also create
        // the FK with ON DELETE CASCADE — deleting the owning user cascades to
        // their personal library (the service layer re-parents shelves first so
        // the RESTRICT on shelves.library_id is never violated). SQLite cannot
        // add FK constraints to existing tables via ALTER TABLE, and the
        // rename→recreate workaround is incompatible with sea_orm_migration
        // transactions (PRAGMA foreign_keys cannot be toggled inside a
        // transaction). Since SQLite is used for testing only and the service
        // layer performs explicit library cleanup before user deletion, we skip
        // the FK on SQLite.
        manager
            .alter_table(
                Table::alter()
                    .table(Libraries::Table)
                    .add_column(ColumnDef::new(Libraries::OwnerId).big_integer().null())
                    .to_owned(),
            )
            .await?;

        if manager.get_database_backend() != sea_orm::DatabaseBackend::Sqlite {
            manager
                .create_foreign_key(
                    ForeignKey::create()
                        .name("fk_libraries_owner_id")
                        .from(Libraries::Table, Libraries::OwnerId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Libraries {
    Table,
    OwnerId,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
