use sea_orm_migration::{prelude::*, schema::timestamp_with_time_zone};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(UserLibraries::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(UserLibraries::UserId).big_integer().not_null())
                    .col(ColumnDef::new(UserLibraries::LibraryId).big_integer().not_null())
                    .col(timestamp_with_time_zone(UserLibraries::AddedAt))
                    .primary_key(Index::create().col(UserLibraries::UserId).col(UserLibraries::LibraryId))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_user_libraries_user_id")
                            .from(UserLibraries::Table, UserLibraries::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_user_libraries_library_id")
                            .from(UserLibraries::Table, UserLibraries::LibraryId)
                            .to(Libraries::Table, Libraries::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Populate: every existing user is assigned to "All Books"
        manager
            .get_connection()
            .execute_unprepared("INSERT INTO user_libraries (user_id, library_id, added_at) SELECT id, 1, CURRENT_TIMESTAMP FROM users")
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(UserLibraries::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum UserLibraries {
    Table,
    UserId,
    LibraryId,
    AddedAt,
}
#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
#[derive(DeriveIden)]
enum Libraries {
    Table,
    Id,
}
