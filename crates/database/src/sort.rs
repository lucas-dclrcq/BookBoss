use bb_core::book::{BookSortField, BookSortOrder, SortDirection};
use sea_orm::{EntityTrait, ExprTrait, QueryOrder, Select};

use crate::entities::books;

/// Applies a `BookSortOrder` to a books query.
///
/// When `sort` is `None`, falls back to `id ASC` for deterministic ordering.
/// Every sort clause includes `books.id` as a tiebreaker.
pub fn apply_book_sort<E: EntityTrait>(mut query: Select<E>, sort: Option<BookSortOrder>) -> Select<E> {
    let Some(sort) = sort else {
        return query.order_by_asc(books::Column::Id);
    };

    match sort.field {
        BookSortField::DateAdded => {
            query = match sort.direction {
                SortDirection::Asc => query.order_by_asc(books::Column::CreatedAt),
                SortDirection::Desc => query.order_by_desc(books::Column::CreatedAt),
            };
        }
        BookSortField::Title => {
            query = match sort.direction {
                SortDirection::Asc => query.order_by_asc(books::Column::Title),
                SortDirection::Desc => query.order_by_desc(books::Column::Title),
            };
        }
        BookSortField::AuthorTitle => {
            use sea_orm::sea_query::{Alias, Condition, Expr, Query, SimpleExpr, SubQueryStatement};

            let ba = Alias::new("ba");
            let a = Alias::new("a");

            let mut sq = Query::select();
            sq.expr(Expr::col((a.clone(), Alias::new("name"))))
                .from_as(Alias::new("authors"), a.clone())
                .join_as(
                    sea_orm::sea_query::JoinType::InnerJoin,
                    Alias::new("book_authors"),
                    ba.clone(),
                    Condition::all().add(Expr::col((ba.clone(), Alias::new("author_id"))).equals((a, Alias::new("id")))),
                )
                .and_where(Expr::col((ba.clone(), Alias::new("book_id"))).equals((Alias::new("books"), Alias::new("id"))))
                .and_where(Expr::col((ba, Alias::new("sort_order"))).eq(0i32))
                .limit(1);

            let subquery_expr = SimpleExpr::SubQuery(None, Box::new(SubQueryStatement::SelectStatement(sq)));

            query = match sort.direction {
                SortDirection::Asc => query.order_by(subquery_expr, sea_orm::Order::Asc).order_by_asc(books::Column::Title),
                SortDirection::Desc => query.order_by(subquery_expr, sea_orm::Order::Desc).order_by_desc(books::Column::Title),
            };
        }
    }

    query.order_by_asc(books::Column::Id)
}
