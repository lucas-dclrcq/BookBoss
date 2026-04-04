use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 1. Add the new column with a safe default.
        manager
            .alter_table(
                Table::alter()
                    .table(Books::Table)
                    .add_column(ColumnDef::new(Books::HasCover).boolean().not_null().default(false))
                    .to_owned(),
            )
            .await?;

        // 2. Back-fill: set has_cover = true wherever cover_path was set.
        manager
            .get_connection()
            .execute_unprepared("UPDATE books SET has_cover = TRUE WHERE cover_path IS NOT NULL")
            .await?;

        // 3. Drop the old column.
        manager
            .alter_table(Table::alter().table(Books::Table).drop_column(Books::CoverPath).to_owned())
            .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Books {
    Table,
    HasCover,
    CoverPath,
}
