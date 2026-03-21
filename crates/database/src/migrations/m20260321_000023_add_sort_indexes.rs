use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name("idx_books_created_at")
                    .table(Books::Table)
                    .col(Books::CreatedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(Index::create().name("idx_books_title").table(Books::Table).col(Books::Title).to_owned())
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_book_authors_book_id_sort_order")
                    .table(BookAuthors::Table)
                    .col(BookAuthors::BookId)
                    .col(BookAuthors::SortOrder)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_index(Index::drop().name("idx_book_authors_book_id_sort_order").to_owned()).await?;
        manager.drop_index(Index::drop().name("idx_books_title").to_owned()).await?;
        manager.drop_index(Index::drop().name("idx_books_created_at").to_owned()).await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Books {
    Table,
    CreatedAt,
    Title,
}

#[derive(DeriveIden)]
enum BookAuthors {
    Table,
    BookId,
    SortOrder,
}
