---
slug: bug-delete-genre-no-enrichment
type: bug
status: needs-triaging
description: Deleting a genre not triggering enrichment
priority: P2
---

# Issue

When a genre (and likely tag) is deleted from the settings page and they are attached to a book, the enriched books should be enqueued to be rebuilt.
The book_genre and book_tag have no way of triggering the recovery by ensure enrichments job.
