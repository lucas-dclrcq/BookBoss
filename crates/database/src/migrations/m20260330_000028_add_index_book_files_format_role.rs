use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name("idx_book_files_format_role")
                    .table(BookFiles::Table)
                    .col(BookFiles::Format)
                    .col(BookFiles::FileRole)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_index(Index::drop().name("idx_book_files_format_role").to_owned()).await
    }
}

#[derive(DeriveIden)]
enum BookFiles {
    Table,
    Format,
    FileRole,
}
