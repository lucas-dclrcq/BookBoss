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
            .create_table(
                Table::create()
                    .table(BookFiles::Table)
                    .if_not_exists()
                    .col(big_integer(BookFiles::BookId).not_null())
                    .col(string(BookFiles::Format).not_null())
                    .col(string(BookFiles::FileRole).not_null())
                    .col(big_integer(BookFiles::FileSize))
                    .col(string(BookFiles::FileHash))
                    .primary_key(Index::create().col(BookFiles::BookId).col(BookFiles::Format).col(BookFiles::FileRole))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_book_files_book_id")
                            .from(BookFiles::Table, BookFiles::BookId)
                            .to(Books::Table, Books::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_book_files_file_hash")
                    .table(BookFiles::Table)
                    .col(BookFiles::FileHash)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_index(Index::drop().name("idx_book_files_file_hash").to_owned()).await?;
        manager.drop_table(Table::drop().table(BookFiles::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum BookFiles {
    Table,
    BookId,
    Format,
    FileRole,
    FileSize,
    FileHash,
}

#[derive(DeriveIden)]
enum Books {
    Table,
    Id,
}
