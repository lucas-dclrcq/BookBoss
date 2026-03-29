---
slug: bug-genre-delete
type: bug
status: complete
description: Delete confirmation modal loses focus and Escape key on 2nd+ use (genre and tag)
priority: P2
---

# Issue

The delete confirmation modal in `settings_page/genre_tags_section.rs` has two related symptoms:

1. The Delete button only receives focus on the first modal open. Subsequent modals (different genre or tag) don't auto-focus the button.
2. Pressing Escape does not dismiss the modal on 2nd+ opens.

Both affect genres and tags — they share the same modal code path.

## Root Cause

The modal uses `autofocus: true` on the Delete button (line 439). The HTML `autofocus` attribute only fires on initial page load, not when an element is dynamically mounted/re-rendered by Dioxus. On 2nd+ opens, the button never gets focus.

The `onkeydown` Escape handler (line 420) is on the backdrop `div`. Keyboard events only reach it by bubbling up from a focused child — so when nothing has focus, Escape silently does nothing.

## Fix

Replace `autofocus: true` on the Delete button with:

```rust
onmounted: move |e| {
    spawn(async move { let _ = e.set_focus(true).await; });
},
```

This fires every time the element mounts (each modal open), restoring focus reliably. Once focus is on the Delete button, Escape events bubble up to the backdrop `onkeydown` and dismiss the modal — fixing both symptoms with one change.

## Acceptance Criteria

- [ ] Delete button has focus when modal opens (1st and every subsequent open)
- [ ] Enter key triggers delete
- [ ] Escape key dismisses the modal
- [ ] Applies to both genre and tag delete flows
