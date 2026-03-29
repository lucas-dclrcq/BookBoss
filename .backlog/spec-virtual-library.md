# Virtual Library — Design Spec

**Date:** 2026-03-28
**Status:** Draft
**Epic:** `epic-virtual-library`

---

## Part 1: User-Facing Design

### Overview

Virtual libraries provide privacy boundaries between users sharing the same BookBoss instance.
Each user sees only the books in their assigned libraries. An "All Books" system library contains
every book in the catalog. Admins create personal libraries for users and assign books to them.
Single-user installations are unaffected — everything works as before with no additional setup.

### Roles & Permissions

**Admin:**

- Creates and deletes virtual libraries (Settings → Libraries)
- Creates personal libraries for users (Settings → Users)
- Assigns books to libraries (edit metadata, bulk edit metadata, incoming review)
- Assigns libraries to users
- Can use a `Library` filter rule in the filter builder to find books not in a specific library

**User (multi-library):**

- Sees books from their active library in the book grid and search
- Switches between assigned libraries via the NavBar library picker
- Home icon in NavBar navigates to their default library
- Can set their default library on their profile page
- If assigned the "All Books" library: can browse the full catalog and drag books into their
  default library or onto shelves (which adds the book to the shelf's parent library)

**User (single-library):**

- No library picker — Home icon still visible (navigates to default library)
- No library settings on profile page
- Experience identical to today aside from the Home icon

### NavBar Changes

```
[Home icon] [Library: Scotte's Library ▼] [... rest of nav ...]
```

- **Home icon**: Always visible. Navigates to the books page with the user's default library.
- **Library picker**: Only renders when user has 2+ assigned libraries. Dropdown lists all
  assigned libraries. Selecting one switches the active library — book grid, search, and smart
  shelf contents re-scope. Does NOT change the user's default library setting.

### Shelf Behavior

- Shelves are scoped to a library (`library_id` on the shelf row)
- The shelf bar always shows all owned shelves regardless of the active library
- All shelves are private — the public shelf visibility option is removed from the UI
- Shelves serve as drag-and-drop targets: dropping a book on a shelf adds the book to that shelf
  AND ensures the book is in the shelf's parent library

### Drag-and-Drop Interactions

- **Drop on Home icon**: Adds book to user's default library
- **Drop on a manual shelf**: Adds book to that shelf + ensures book is in the shelf's parent library

### OPDS

```
/opds/              → Root catalog
/opds/all           → User's default library books (was: all books)
/opds/libraries     → List of assigned libraries (only shown if 2+)
/opds/libraries/{t} → Books in a specific library (access-checked)
/opds/shelves       → User's shelves (unchanged)
/opds/shelves/{t}   → Shelf books (unchanged)
```

Existing OPDS client configurations (KOReader, etc.) continue to work — `/opds/all` returns the
default library which for unmigrated users is "All Books."

### Kobo Sync

No user-facing change. The Kobo companion shelf is parented to the user's default library. Only
books in that library matching the sync filter are synced. To get a book from another library
onto a Kobo: drag it to the Home icon (adds to default library), and the next sync picks it up.

### Admin Workflows

#### Settings → Libraries

Same pattern as Genre/Tags settings page:

- List of libraries: name, user count, book count, delete `x`
- `+` button to create a new library
- "All Books" is visible but not deletable or renamable

**Delete library workflow:**

1. Remove `library_books` rows for that library
2. Re-parent all shelves from the deleted library to "All Books"
3. Update any user whose default library was the deleted one to "All Books"
4. Remove `user_libraries` rows

#### Settings → Users (create)

Existing user creation form gains:

- "Create personal library" checkbox
- When checked: enables an input field pre-filled with "{full_name}'s Library" (editable)
- Library assignment pill picker (multi-select including system libraries)
- Default library picker (dropdown from assigned libraries)
- If "Create personal library" is checked, the new library is pre-selected as default

#### Settings → Users (edit)

- "Create personal library" checkbox + name input — only shown if user doesn't already have a
  non-system personal library
- Library assignment pill picker
- Default library picker

**Personal library creation for existing user triggers:**

1. Create the library
2. Assign to user, set as default
3. Re-parent all user's shelves to the new library
4. Copy books from those shelves into the new library
5. Copy books with `UserBookMetadata` records into the new library

#### Edit Metadata / Bulk Edit Metadata

- New "Libraries" pill picker alongside genres/tags/publisher
- Shows non-system libraries only ("All Books" membership is implicit)
- During incoming book review: same picker, defaults all non-system libraries so administrator would
  delete the libraries that shouldn't get the book

#### Bulk Select Add to Library

- the administrator should be able to multi-select books and then bulk add them to a library to
  make them available a user. All non-system libraries should be available as targets
- this is a short-cut to editing metadata to assign the libraries

#### Library Filter Rule (Admin Only)

A `Library` FilterRule is available in the filter builder for admin users only. Supports `SetOp`
operators: `IncludesAny`, `IncludesAll`, `ExcludesAll`, `IsEmpty`, `IsNotEmpty`. Enables
workflows like "show all books not in Scotte's Library."

When a `Library` FilterRule is present in a query, normal library scoping is bypassed — the
filter itself handles the scoping. Safe because only admins can add the rule.

### Profile Page

- **2+ libraries**: Shows default library picker (dropdown of assigned libraries)
- **1 library**: No library-related UI

### Migration Experience

After upgrading, existing installations see zero behavioral change:

- An "All Books" system library is created containing every book
- All users are assigned to "All Books" with it as their default
- All shelves are parented to "All Books"
- The feature is dormant until an admin creates a virtual library

---

## Part 2: Implementation Design

### Architectural Approach

Library scoping is a security boundary enforced at the repository layer (Approach C from design
exploration). Repository methods that return books gain a `library_id: Option<LibraryId>`
parameter. When `Some(id)`, the query joins through `library_books` to restrict results. When
`None`, no library filter — used only for admin operations.

### Preparatory Rename

The existing `LibraryService` and `LibraryRepository` in `crates/core/src/library/` are renamed
to free the `Library` name for the virtual library domain:

| Current             | New                    | Rationale                                                                                                                           |
| ------------------- | ---------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `LibraryService`    | `CollectionService`    | Owns `approve_book`, `reject_book`, `edit_book`, `delete_book`, `search_books`, `library_stats` — operations on the book collection |
| `LibraryRepository` | `CollectionRepository` | Owns `books_for_filter`, `count_for_filter` — filtered book queries                                                                 |

### Data Model

**New tables:**

```
libraries
├── id: LibraryId (PK)
├── version: u64
├── token: LibraryToken (unique, prefix LB_)
├── name: String (unique)
├── is_system: bool               -- true only for "All Books"
├── created_at: DateTime<Utc>
└── updated_at: DateTime<Utc>

library_books (junction)
├── library_id: LibraryId (FK, CASCADE DELETE)
├── book_id: BookId (FK, CASCADE DELETE)
└── added_at: DateTime<Utc>
    PK: (library_id, book_id)

user_libraries (junction)
├── user_id: UserId (FK, CASCADE DELETE)
├── library_id: LibraryId (FK, CASCADE DELETE)
└── added_at: DateTime<Utc>
    PK: (user_id, library_id)
```

**Modified tables:**

```
shelves
└── + library_id: LibraryId (FK, NOT NULL, DEFAULT 1)
```

**"All Books" library:**

- `id = 1` (hard-coded in migration)
- Token generated via `LibraryToken::new(1)` — deterministic from ID
- `is_system = true` — prevents deletion and rename
- `LibraryToken::all_books()` helper in core for application-level references

**Default library per user:**

- Stored as `UserSetting("default_library")` with the library token as the value
- Avoids invariant violations from a boolean flag on the junction table
- Updated via profile page; reset to "All Books" if current default is deleted

### Migration Sequence

**Migration 1** — Create `libraries` table + seed "All Books":

```sql
INSERT INTO libraries (id, token, name, is_system, ...)
VALUES (1, '<LB_ token from LibraryToken::new(1)>', 'All Books', true, ...);
```

**Migration 2** — Create `library_books` junction + populate:

- Insert `(library_id=1, book_id)` for every existing book

**Migration 3** — Create `user_libraries` junction + populate:

- Insert `(user_id, library_id=1)` for every existing user

**Migration 4** — Add `library_id` to `shelves`:

- `ALTER TABLE shelves ADD COLUMN library_id BIGINT NOT NULL DEFAULT 1 REFERENCES libraries(id)`
- All existing shelves get `library_id = 1` via the default

**Migration 5** — Seed default library user setting:

- Insert `UserSetting("default_library", "<All Books token>")` for every existing user

### Query Scoping

**Repository parameter:** `library_id: Option<LibraryId>`

| Repository Method                        | Gets `library_id` |
| ---------------------------------------- | ----------------- |
| `BookRepository::list_books`             | Yes               |
| `CollectionRepository::books_for_filter` | Yes               |
| `CollectionRepository::count_for_filter` | Yes               |
| Search queries                           | Yes               |

When `library_id` is `Some(id)`, the query adds:

```sql
INNER JOIN library_books lb ON lb.book_id = books.id AND lb.library_id = ?
```

When `None`, no join — full catalog access (admin operations only).

**Access validation:** Before executing a library-scoped query, the service layer checks
`user_libraries` for `(user_id, requested_library_id)`. Rejects with permission error if the
user is not assigned to the requested library.

**Library FilterRule bypass:** When a `Library` FilterRule is present in the query, the
`library_id` parameter is ignored — the filter itself handles library scoping via subquery.

### Scoping Matrix

| Surface                    | Scoped by                                                                    |
| -------------------------- | ---------------------------------------------------------------------------- |
| Book grid                  | Active library                                                               |
| Search                     | Active library                                                               |
| Manual shelf contents      | Shelf's parent library (implicit — junction only has books from the library) |
| Smart shelf contents       | Shelf's parent library (`library_id` passed to `books_for_filter`)           |
| OPDS `/opds/all`           | User's default library                                                       |
| OPDS `/opds/libraries/{t}` | Requested library (access-checked)                                           |
| Kobo device sync           | Companion shelf's parent library                                             |
| Edit metadata              | Unscoped (`None`) — admin sees all books                                     |
| Incoming review            | Unscoped (`None`) — admin assigns libraries during review                    |

### Changes by Crate

**`crates/core/`:**

_Preparatory rename:_

- `crates/core/src/library/` → `crates/core/src/collection/`
- `LibraryService` → `CollectionService`
- `LibraryRepository` → `CollectionRepository`
- Update all imports and `CoreServices` wiring

_New domain module `crates/core/src/library/`:_

- `Library` model struct
- `LibraryToken` with `LB_` prefix and `all_books()` helper
- `LibraryRepository` trait — CRUD for `libraries`, `library_books`, `user_libraries`
- `LibraryService` — library CRUD, user assignment, personal library creation with migration
  logic (re-parent shelves, copy books from shelves + `UserBookMetadata`)

_Modified:_

- `CollectionRepository` — `library_id: Option<LibraryId>` on `books_for_filter`, `count_for_filter`
- `BookRepository` — `library_id: Option<LibraryId>` on `list_books`
- `FilterRule` — new `Library` variant with `SetOp` operators
- `Shelf` model — gains `library_id: LibraryId`
- `ShelfService` — companion shelf creation sets `library_id` from user's default library
- `DeviceService` — companion shelf uses user's default library
- Import pipeline — approved books automatically get `library_books` row for "All Books"

**`crates/database/`:**

_Migrations:_ 5 migrations as described above

_New:_

- `libraries` entity
- `library_books` entity
- `user_libraries` entity
- `LibraryRepositoryAdapter` — implements new `LibraryRepository` trait

_Modified:_

- `shelves` entity — add `library_id` column
- `BookRepositoryAdapter` — library join logic on `list_books`
- `CollectionRepositoryAdapter` (renamed from `LibraryRepositoryAdapter`) — library join on `books_for_filter`, `count_for_filter`
- `ShelfRepositoryAdapter` — persist `library_id`
- `filter.rs` — new `library_condition()` for the `Library` FilterRule

**`crates/frontend/`:**

_NavBar:_

- Home icon (always visible, navigates to default library)
- Library picker component (conditional on 2+ libraries, dropdown)
- Active library state signal, initialized from `UserSetting("default_library")`

_Book grid / search:_

- Pass active `library_id` to server fns

_Shelf bar:_

- Always show all owned shelves
- Remove visibility checkbox from shelf create/edit

_Filter builder:_

- `Library` FilterRule variant (admin-only gating in field selector)
- `FilterEntityOptions.libraries` field
- Pill picker for library values

_Settings → Libraries:_

- New section (same pattern as Genre/Tags)
- Library list with name, user count, book count, delete
- `+` to create, "All Books" not deletable

_Settings → Users:_

- "Create personal library" checkbox + editable name input
- Library assignment pill picker
- Default library picker
- Personal library creation trigger (shelves + books migration)

_Edit metadata / Bulk edit:_

- Libraries pill picker (non-system libraries only)

_Incoming review:_

- Libraries pill picker (empty by default — book is only in "All Books")

_Profile page:_

- Default library picker (only if 2+ libraries)

_OPDS:_

- `/opds/all` scoped to default library
- New `/opds/libraries` feed and `/opds/libraries/{token}` (access-checked)
- Root catalog links updated

_Drag-and-drop:_

- Home icon as drop target → add to default library
- Shelf as drop target → add to shelf + shelf's parent library

_Kobo sync:_

- Device companion shelf creation uses user's default library
