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

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 1. Re-add cover_path as nullable text.
        manager
            .alter_table(
                Table::alter()
                    .table(Books::Table)
                    .add_column(ColumnDef::new(Books::CoverPath).text().null())
                    .to_owned(),
            )
            .await?;

        // 2. Restore the value where has_cover is true.
        manager
            .get_connection()
            .execute_unprepared("UPDATE books SET cover_path = 'cover.jpg' WHERE has_cover = TRUE")
            .await?;

        // 3. Drop has_cover.
        manager
            .alter_table(Table::alter().table(Books::Table).drop_column(Books::HasCover).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Books {
    Table,
    HasCover,
    CoverPath,
}
