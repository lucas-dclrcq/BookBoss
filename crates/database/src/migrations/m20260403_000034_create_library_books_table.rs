use sea_orm_migration::{prelude::*, schema::timestamp_with_time_zone};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(LibraryBooks::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(LibraryBooks::LibraryId).big_integer().not_null())
                    .col(ColumnDef::new(LibraryBooks::BookId).big_integer().not_null())
                    .col(timestamp_with_time_zone(LibraryBooks::AddedAt))
                    .primary_key(Index::create().col(LibraryBooks::LibraryId).col(LibraryBooks::BookId))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_library_books_library_id")
                            .from(LibraryBooks::Table, LibraryBooks::LibraryId)
                            .to(Libraries::Table, Libraries::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_library_books_book_id")
                            .from(LibraryBooks::Table, LibraryBooks::BookId)
                            .to(Books::Table, Books::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Populate: every existing book belongs to "All Books" (library_id=1)
        manager
            .get_connection()
            .execute_unprepared("INSERT INTO library_books (library_id, book_id, added_at) SELECT 1, id, CURRENT_TIMESTAMP FROM books")
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(LibraryBooks::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum LibraryBooks {
    Table,
    LibraryId,
    BookId,
    AddedAt,
}
#[derive(DeriveIden)]
enum Libraries {
    Table,
    Id,
}
#[derive(DeriveIden)]
enum Books {
    Table,
    Id,
}
