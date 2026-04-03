# Shelves & Reading State

## Shelves

Shelves are named collections you create to organise your books. Each shelf belongs to a
library — the shelf pills shown at the top of the book grid are those that belong to the
currently active library. There are two types of shelf:

### Manual Shelves

Manual shelves contain exactly the books you add to them.

**Creating a shelf:** Open **Settings > Shelves** or use the sidebar to create a new shelf.

**Adding books:** Drag a book's cover from the grid onto a shelf pill at the top of the screen.

**Removing books:** Open the shelf, then remove individual books from the shelf view.

### Smart Shelves

Smart shelves are defined by filter rules and update automatically. For example, you can create a smart shelf that contains all books with reading status "Reading", or all books by a specific author.

Smart shelves recalculate their contents whenever books or reading state change — no manual maintenance needed.

**Available filter rules include:**

| Rule | Description |
| ---- | ----------- |
| Reading status | Match by read/reading/unread/etc. |
| Author | Match books by a specific author |
| Series | Match books in a specific series |
| Genre / Tag | Match books with a specific genre or tag |
| Library | Match books that belong to a specific library (admin only) |

The **Library** rule is only visible in the filter builder for admin accounts. It lets you build cross-library smart collections, such as a shelf that spans books from two different personal libraries.

---

## Reading State

Each user tracks their own reading state per book. Reading status options are:

| Status        | Meaning                                 |
| ------------- | --------------------------------------- |
| **Unread**    | Default — book has not been read        |
| **Reading**   | Currently reading                       |
| **Paused**    | Reading paused                          |
| **Rereading** | Reading again                           |
| **Read**      | Finished reading                        |
| **Abandoned** | Stopped reading, not planning to finish |

### Setting Reading Status

You can set reading status in several ways:

- **Book detail page** — change the status for a single book
- **Bulk select** — select multiple books in the grid and set status for all at once
- **Kobo sync** — reading progress syncs automatically from Kobo devices

Reading state is per-user — each user in a multi-user setup has independent tracking.
