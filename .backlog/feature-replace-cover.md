---
slug: feature-replace-cover
type: feature
status: needs-triage
description: Allow replacing a book's cover by dragging an image file onto the cover in the review/edit metadata screens
priority: P2
---

# Replace Cover via Drag-and-Drop

While reviewing an incoming book or editing a book's metadata, the user should be able to drag a JPG (and possibly PNG) image file from their computer onto the current cover picture to replace it.

## Requirements

- Drag-and-drop a JPG onto the cover image replaces it
- Available on both the incoming book review screen and the edit metadata screen
- PNG support (open question — confirm whether to support it)

## Open Questions / Plan Needed

- Should PNG be supported in addition to JPG?
- What is the upload mechanism — server fn with multipart form data, or base64-encoded in a server fn arg?
- Where is cover storage wired? (`FileStoreService` / `PipelineService::edit_book` currently handles cover storage — the drag-drop handler needs to call the same path)
- Does the dropped image replace the cover immediately (optimistic UI) or after server confirmation?
- Should there be a size/dimension limit on the uploaded image?
- Full implementation plan needed
