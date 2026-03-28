---
slug: epic-virtual-library
type: epic
status: needs-triage
description: Virtual library support — users see only the books in their assigned libraries
priority: P3
---

# Virtual Library Support

Different users may want access to different subsets of books. Virtual libraries provide scoped views so users only see books relevant to them.

## Requirements

- On user creation, admin can: create a personal library for the user, assign existing libraries, set a default library
- Imported books always land in "All Books" — admin can also assign them to other libraries
- All shelves have a virtual library as their source; migrating existing shelves assigns the owner's default library
- Users with access to "All Books" can add a book from it to their personal library
- Users can remove a book from their personal library (does not delete the book)
- Admins can add any book to any library (location TBD — possibly edit metadata / bulk edit screens)

## Spec

See [spec-virtual-library.md](spec-virtual-library.md) for the full architecture and implementation plan.

## Open Questions / Plan Needed

- How are virtual libraries represented in the DB? New `libraries` table + `library_books` junction? Or reuse shelves?
- How does "All Books" work — is it a special sentinel library or implicit (no filter)?
- Should library assignment be visible on the book card / book detail page?
- How does the library scope affect search, OPDS, and Kobo sync?
- Where does admin library assignment UI live — edit metadata screen, bulk edit, or a dedicated library management page?
- Permission model: new capability for library management? Does library scoping replace or augment existing capabilities?
- Migration: existing users get access to "All Books" (or their current full view) as their default library
- Full architecture and implementation plan needed before decomposing into child issues
