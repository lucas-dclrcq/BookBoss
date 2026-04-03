# Libraries

Libraries are virtual collections that let you slice the full book catalogue into curated views.
Each user sees only the books in their active library. A multi-user household might have
a shared family library alongside personal ones; a single-user install can ignore libraries
entirely and use the built-in **All Books** library.

---

## How Libraries Work

Every approved book is automatically added to the **All Books** system library. This library
always exists, contains every book in the catalogue, and cannot be deleted or renamed.

Beyond All Books, an administrator can create:

- **Shared libraries** — admin-created collections assigned to any number of users.
  Useful for genre or topic collections shared across the household.
- **Personal libraries** — user-owned collections seeded from the user's existing shelves
  and reading state. Each user can have at most one personal library.

Users only see books that belong to their active library. The active library is switched
from the NavBar picker, and the home button always returns to the user's default library.

---

## For Administrators

### Settings → Libraries

Navigate to **Settings → Libraries** to manage the library catalogue.

The panel lists every library with its user count and book count:

- **All Books** — displays a grey _system_ badge; the delete button is absent.
  You cannot delete or rename this library.
- **Custom libraries** — have a delete button (trash icon). Deleting a library re-parents
  all shelves that belonged to it back to All Books, and resets any user whose default was
  that library back to All Books. The books themselves are not deleted.

**Creating a library:**

1. Click **Create Library**.
2. Enter a unique name and confirm.

The new library starts empty. Assign users to it and add books via bulk operations or
the edit-metadata form.

**Adding books to a library:**

- From the book grid, select books and use **Add to Library** in the selection bar.
- From the bulk edit modal, use the **Libraries** checkboxes.
- From the edit-metadata or incoming review form, use the **Libraries** field.
- Drag a book cover onto the **Home** icon in the NavBar to add it to your default library.

### Settings → Users — Library Assignment

When you create or edit a user, the modal includes a library management section:

**Library checkboxes** — tick each library the user should have access to. Users can only
browse libraries they are assigned to. All Books is always available and cannot be unassigned.

**Default library** — a picker (shown once the user is assigned to two or more libraries)
sets which library loads when the user first logs in or clicks the Home button.

**Personal library** — a checkbox (shown only when the user does not yet have one) creates
a personal library for the user:

- Enter a name or accept the auto-filled suggestion.
- On save, BookBoss creates the library, assigns the user to it, re-parents their existing
  shelves into it, and seeds it with books they already have a shelf or reading-state
  relationship with.
- The personal library is set as the user's default.

**Renaming a personal library** — when a user already has a personal library, an editable
name field replaces the creation checkbox. The rename is applied when you save the form.

**Deleting a personal library** — click the × button next to the name field. An inline
confirmation appears. Confirm to mark it for deletion; an **Undo** link reverses the choice
before you save. The deletion is applied on save: shelves re-parent to All Books and the
user's default resets to All Books.

---

## For Users

### Switching Libraries

If you are assigned to two or more libraries, a library picker appears in the NavBar between
the search bar and the user menu. Select a library from the dropdown to scope the book grid
and search to that collection.

The **Home** button (house icon) is always visible. Clicking it switches back to your default
library.

### Setting Your Default Library

Go to your **Profile** page. If you have two or more assigned libraries a **Default Library**
picker is shown. The library you choose here is the one that loads on login and when you click
the Home button.

### Adding Books to Your Library

Drag any book cover from the grid onto the **Home** icon in the NavBar. The icon briefly
turns green to confirm the drop. The book is added to your current default library.

### Libraries and Shelves

Each shelf belongs to a library. When you are browsing a library, the shelf pills at the top
of the screen show only the shelves that belong to that library. Shelves created while a
personal library is active are automatically placed in that library.

If your personal library is ever deleted by an administrator, your shelves are moved back to
All Books rather than being deleted.

### Library Filter Rule in Smart Shelves

When building a smart shelf filter (admin accounts only), a **Library** rule is available.
This matches books that belong to a specific library, and can be combined with other rules
to build cross-library smart collections.
