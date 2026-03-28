---
slug: feature-genre-tags-settings
type: feature
status: complete
description: Admin settings page to view, add, and delete genres and tags, showing book counts per entry
priority: P2
---

# Genre/Tags Management Settings Page

An admin settings page ("Genre/Tags") where genres and tags can be managed. Both appear on the
same page in separate panels. Each entry shows the name, book count, and an `x` delete button.
A `+` button at the top of each panel lets the admin pre-fill genres and tags before any books
have been imported.

Deleting a genre or tag removes it from all books. Adding creates it immediately so it is
available when reviewing incoming books.

## Requirements

- Settings page accessible to admins only (inherits existing admin gate)
- New "Genre/Tags" section in the settings nav (hash `#genre-tags`)
- Genres and tags in separate panels on the same page
- Each row: name + book count + `x` delete button
- `+` button per panel opens an inline add form (name input, confirm on Enter or button)
- Duplicate name shows an inline error; does not close the form
- Delete shows a confirmation modal before proceeding (same pattern as users section)
- Hard delete: cascade on `book_genres`/`book_tags` FKs automatically removes all associations

## Key Decisions

- **Section name**: "Genre/Tags" (nav label), hash `#genre-tags`
- **Deletion**: Hard delete — `book_genres` and `book_tags` have CASCADE DELETE on the FK,
  so deleting a genre/tag row automatically cleans up all book associations
- **Permissions**: No new capability — existing admin gate on the settings page is sufficient
- **Pagination**: None — `list_all_genres` / `list_all_tags` return all records ordered by
  name; can add pagination later if needed

## Implementation Plan

### Change 1 — Core traits + BookService

**`crates/core/src/book/repository/genre.rs`** — add two methods to `GenreRepository`:

```rust
async fn delete_genre(&self, transaction: &dyn Transaction, id: GenreId) -> Result<(), Error>;
async fn list_genres_with_counts(&self, transaction: &dyn Transaction) -> Result<Vec<(Genre, u64)>, Error>;
```

**`crates/core/src/book/repository/tag.rs`** — add two methods to `TagRepository`:

```rust
async fn delete_tag(&self, transaction: &dyn Transaction, id: TagId) -> Result<(), Error>;
async fn list_tags_with_counts(&self, transaction: &dyn Transaction) -> Result<Vec<(Tag, u64)>, Error>;
```

**`crates/core/src/book/service.rs`** — add six methods to `BookService` trait + `BookServiceImpl`:

```rust
// Create
async fn create_genre(&self, name: String) -> Result<Genre, Error>;
async fn create_tag(&self, name: String) -> Result<Tag, Error>;

// Delete (by token — consistent with how the frontend identifies entities)
async fn delete_genre(&self, token: GenreToken) -> Result<(), Error>;
async fn delete_tag(&self, token: TagToken) -> Result<(), Error>;

// List with counts (replaces list_all_genres / list_all_tags for this page)
async fn list_genres_with_counts(&self) -> Result<Vec<(Genre, u64)>, Error>;
async fn list_tags_with_counts(&self) -> Result<Vec<(Tag, u64)>, Error>;
```

`create_genre` wraps `genre_repository.add_genre(tx, NewGenre { name })`.
`delete_genre` resolves the token to an id via `find_by_token`, then calls `genre_repository.delete_genre(tx, id)`.

End-of-task routine for this change:

1. `just fmt`
2. `just clippy`
3. `just component-tests`
4. `jj desc -m "feat(core): add delete/create/list_with_counts to GenreRepository, TagRepository, BookService"`

---

### Change 2 — Database adapters

**`crates/database/src/adapters/genre.rs`** — implement the two new repo methods:

`delete_genre`: simple DELETE by id. The `book_genres` FK has CASCADE DELETE so no extra
cleanup is needed:

```rust
genres::Entity::delete_by_id(id.as_i64())
    .exec(tx)
    .await?;
```

`list_genres_with_counts`: LEFT JOIN `book_genres`, GROUP BY genre id, ORDER BY name:

```sql
SELECT g.*, COUNT(bg.book_id) AS book_count
FROM genres g
LEFT JOIN book_genres bg ON g.id = bg.genre_id
GROUP BY g.id
ORDER BY g.name
```

Return type `Vec<(Genre, u64)>`. Use SeaORM `QuerySelect` / raw query as appropriate —
check how other count queries in the codebase are done (e.g. `count_books_for_author`).

**`crates/database/src/adapters/tag.rs`** — identical pattern for `delete_tag` and
`list_tags_with_counts` (joined against `book_tags`).

End-of-task routine:

1. `just fmt`
2. `just clippy`
3. `just component-tests`
4. `jj desc -m "feat(database): implement delete and list_with_counts for genres and tags"`

---

### Change 3 — New `genre_tags_section.rs`

Create `crates/frontend/src/routes/settings_page/genre_tags_section.rs`.

**Server fn DTOs** (use `serde` + `derive`):

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct GenreTagEntry {
    pub token: String,
    pub name: String,
    pub book_count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct GenreTagsData {
    pub genres: Vec<GenreTagEntry>,
    pub tags: Vec<GenreTagEntry>,
}
```

**Server functions** (follow the pattern in `users_section.rs` — check auth via
`auth_session`, extract `core_services` from extensions):

```rust
#[server] pub async fn get_genre_tags() -> Result<GenreTagsData, ServerFnError>
#[server] pub async fn admin_create_genre(name: String) -> Result<(), ServerFnError>
#[server] pub async fn admin_delete_genre(token: String) -> Result<(), ServerFnError>
#[server] pub async fn admin_create_tag(name: String) -> Result<(), ServerFnError>
#[server] pub async fn admin_delete_tag(token: String) -> Result<(), ServerFnError>
```

All mutating server fns check `Admin` permissions and return
`ServerFnError::new("Insufficient permissions")` if not met.

`admin_create_genre`/`admin_create_tag` should surface duplicate-name errors to the
frontend (the DB has a unique constraint on `name`).

**`GenreTagsSection` component**:

```
┌─────────────────────────────────────────────────────────┐
│  Genres                                     [+ Add]     │
│  ─────────────────────────────────────────────────────  │
│  Fantasy                              42 books    [x]   │
│  Science Fiction                      18 books    [x]   │
│  ...                                                    │
│                                                         │
│  Tags                                       [+ Add]     │
│  ─────────────────────────────────────────────────────  │
│  Award Winner                          7 books    [x]   │
│  ...                                                    │
└─────────────────────────────────────────────────────────┘
```

State signals:

- `genre_tags: Resource<GenreTagsData>` — loaded via `use_server_future(get_genre_tags)`
- `adding_genre: Signal<bool>` / `adding_tag: Signal<bool>` — toggles inline add form
- `new_genre_name: Signal<String>` / `new_tag_name: Signal<String>` — add form input
- `add_genre_error: Signal<Option<String>>` / `add_tag_error: Signal<Option<String>>`
- `deleting: Signal<Option<String>>` — token of the item pending delete confirmation
- `delete_error: Signal<Option<String>>`

**Add flow**: `+` button sets `adding_genre = true`, showing an inline `<input>` +
confirm button below the panel header. On submit, call `admin_create_genre`, clear the
form and refresh `genre_tags` on success, show `add_genre_error` on failure (e.g.
"Genre already exists"). Press Escape or click Cancel to dismiss.

**Delete flow**: `x` button sets `deleting = Some(token)`, which renders the
confirmation modal (same structure as in `users_section.rs` — fixed overlay, item name
in the message, Cancel + Delete buttons, disabled during in-flight request). On
confirm, call `admin_delete_genre`, close modal and refresh `genre_tags` on success.

**Extract a shared `EntityPanel` component** for the genre and tag panels to avoid
duplicating the list + add-form + delete logic. Props:

```rust
title: &'static str,
entries: Vec<GenreTagEntry>,
on_add: EventHandler<String>,        // called with the new name
on_delete: EventHandler<String>,     // called with the token
add_error: Option<String>,
```

End-of-task routine:

1. `just fmt`
2. `just clippy`
3. `just component-tests`
4. `jj desc -m "feat(frontend): add GenreTagsSection with create/delete for genres and tags"`

---

### Change 4 — Wire into settings page

**`crates/frontend/src/routes/settings_page/mod.rs`**:

1. Add `mod genre_tags_section;` (alongside the other section mods)
2. Add `GenreTags` variant to the `Section` enum
3. Add nav button "Genre/Tags" in the sidebar (same style as existing buttons, hash `#genre-tags`)
4. Add a `Section::GenreTags => rsx! { GenreTagsSection {} }` arm in the section renderer

End-of-task routine:

1. `just fmt`
2. `just clippy`
3. `just component-tests`
4. `jj desc -m "feat(frontend): add Genre/Tags section to settings nav"`

## Files to Touch

| File                                                             | Change                                              |
| ---------------------------------------------------------------- | --------------------------------------------------- |
| `crates/core/src/book/repository/genre.rs`                       | Add `delete_genre`, `list_genres_with_counts`       |
| `crates/core/src/book/repository/tag.rs`                         | Add `delete_tag`, `list_tags_with_counts`           |
| `crates/core/src/book/service.rs`                                | Add 6 new methods to trait + impl                   |
| `crates/database/src/adapters/genre.rs`                          | Implement `delete_genre`, `list_genres_with_counts` |
| `crates/database/src/adapters/tag.rs`                            | Implement `delete_tag`, `list_tags_with_counts`     |
| `crates/frontend/src/routes/settings_page/genre_tags_section.rs` | New file — server fns + UI                          |
| `crates/frontend/src/routes/settings_page/mod.rs`                | Add `Section::GenreTags`, nav button, render        |
