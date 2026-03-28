---
slug: feature-genre-tags-settings
type: feature
status: needs-triage
description: Admin settings page to view and delete genres and tags, showing book counts per entry
priority: P2
---

# Genre/Tags Management Settings Page

An admin settings page where genres and tags can be managed. Both appear on the same page in separate tiles, each showing the genre/tag name, its book count, and an `x` button to delete it.

Deleting a genre or tag removes it from all books that have it.

## Requirements

- Settings page accessible to admins only
- Genres and tags shown in separate sections (or tiles) on the same page
- Each entry shows: name + number of books using it
- Delete button (`x`) on each entry removes the genre/tag from the system and from all associated books
- Confirmation prompt before deletion (destructive operation)

## Open Questions / Plan Needed

- Where does this page live in the settings nav? (e.g. under a "Library" or "Metadata" section)
- Should deletion be soft (flag as deleted) or hard (remove from DB + all book associations)?
- What capability/permission gates this page? (`ManageLibrary`? New capability?)
- Pagination or infinite scroll for large lists?
- Full implementation plan needed (server fns, core service methods, UI components)
