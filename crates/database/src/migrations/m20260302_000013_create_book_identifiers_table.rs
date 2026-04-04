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
                    .table(BookIdentifiers::Table)
                    .if_not_exists()
                    .col(big_integer(BookIdentifiers::BookId).not_null())
                    .col(string(BookIdentifiers::IdentifierType).not_null())
                    .col(string(BookIdentifiers::Value))
                    .primary_key(Index::create().col(BookIdentifiers::BookId).col(BookIdentifiers::IdentifierType))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_book_identifiers_book_id")
                            .from(BookIdentifiers::Table, BookIdentifiers::BookId)
                            .to(Books::Table, Books::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_book_identifiers_type_value")
                    .table(BookIdentifiers::Table)
                    .col(BookIdentifiers::IdentifierType)
                    .col(BookIdentifiers::Value)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[derive(DeriveIden)]
enum BookIdentifiers {
    Table,
    BookId,
    IdentifierType,
    Value,
}

#[derive(DeriveIden)]
enum Books {
    Table,
    Id,
}
