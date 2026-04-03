use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Shelves::Table)
                    .add_column(ColumnDef::new(Shelves::LibraryId).big_integer().not_null().default(1i64))
                    .to_owned(),
            )
            .await?;

        // SQLite does not support adding foreign keys to existing tables via ALTER
        // TABLE.
        if manager.get_database_backend() != sea_orm::DatabaseBackend::Sqlite {
            manager
                .create_foreign_key(
                    ForeignKey::create()
                        .name("fk_shelves_library_id")
                        .from(Shelves::Table, Shelves::LibraryId)
                        .to(Libraries::Table, Libraries::Id)
                        .on_delete(ForeignKeyAction::Restrict)
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.get_database_backend() != sea_orm::DatabaseBackend::Sqlite {
            manager
                .drop_foreign_key(ForeignKey::drop().name("fk_shelves_library_id").table(Shelves::Table).to_owned())
                .await?;
        }
        manager
            .alter_table(Table::alter().table(Shelves::Table).drop_column(Shelves::LibraryId).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Shelves {
    Table,
    LibraryId,
}
#[derive(DeriveIden)]
enum Libraries {
    Table,
    Id,
}
