---
slug: feature-shelf-rule
type: feature
status: not-started
description: Add a Shelf filter rule for smart shelves (include/exclude by manual shelf membership)
priority: P2
---

# Shelf Filter Rule for Smart Shelves

A smart-filter rule to match "is on shelf" — allows a smart shelf to include or exclude books based on their manual shelf membership.

## Requirements

- New `Shelf` filter rule with `SetOp` operators: `IncludesAny`, `IncludesAll`, `ExcludesAll`, `IsEmpty`, `IsNotEmpty`
- Rule evaluated via subquery against the `book_shelves` junction table (same pattern as other junction rules)
- Entity picker in the UI shows the user's manual shelves
- The current shelf being edited is excluded from the picker (can't reference itself)
- Round-trip serde test for the new variant

## Architecture

Three layers must all be updated in lockstep:

1. **Core model** (`crates/core/src/filter/model.rs`) — canonical `FilterRule` enum, serialized as JSONB in the DB
2. **Database filter** (`crates/database/src/filter.rs`) — translates `FilterRule` to SeaORM `Condition` via subqueries
3. **Frontend mirror** (`crates/frontend/src/components/filter_builder.rs`) — mirrors the core enum exactly for JSON round-trip compatibility; also holds `FilterEntityOptions` and all UI rendering

The `FilterEntityOptions` struct (`filter_builder.rs:137`) is populated by `get_filter_entity_options()` in `shelf_page.rs:278`. Shelves must be added to both.

The current-shelf exclusion is **client-side**: the server fn returns all manual shelves; `FilterBuilder` receives the current shelf's token and filters it from the list before rendering.

## Plan

### Step 1 — Core model (`crates/core/src/filter/model.rs`)

Add `Shelf` variant to `FilterRule` (after `Publisher`, alongside other entity pill-picker rules):

```rust
Shelf {
    op: SetOp,
    values: Vec<EntityRef>,  // EntityRef.id = shelf.id (i64), EntityRef.label = shelf name
},
```

- `contains_user_scoped_rules()` — add arm `FilterRule::Shelf { .. } => false`
- Add round-trip test following the pattern of `all_filter_rule_variants_round_trip`

### Step 2 — Database filter (`crates/database/src/filter.rs`)

- Import `book_shelves` entity
- Add `shelf_condition(op: SetOp, values: &[EntityRef]) -> Condition` — follows exact pattern of `author_condition` / `genre_condition`, using `book_shelves::Column::BookId` and `book_shelves::Column::ShelfId`
- Add dispatch arm to `rule_condition()`: `FilterRule::Shelf { op, values } => Ok(shelf_condition(*op, values))`

### Step 3 — Frontend types (`crates/frontend/src/components/filter_builder.rs`)

- Add `Shelf { op: SetOp, values: Vec<EntityRef> }` variant to frontend `FilterRule`
- Add `shelves: Vec<(i64, String)>` to `FilterEntityOptions` (manual shelves only)
- Add `field_key()`, `default_rule_for_field()`, `rule_to_summary()` arms
- Add `"shelf"` / `"Shelf"` to field selector dropdown
- Add rule value editor arm: `SetOp` dropdown + entity pill-picker fed from `entity_options.shelves`
- Add `current_shelf_id: Option<i64>` prop to `FilterBuilder`; filter out self before rendering picker

### Step 4 — Server fn (`crates/frontend/src/routes/shelf_page.rs`)

- `get_filter_entity_options()` — fetch user's manual shelves and include in response
- Pass `current_shelf_id` to `FilterBuilder` at the call site (~line 516)

## Files to Touch

| File | Change |
|------|--------|
| `crates/core/src/filter/model.rs` | Add `Shelf` variant + test |
| `crates/database/src/filter.rs` | Add `shelf_condition()` + dispatch arm + import |
| `crates/frontend/src/components/filter_builder.rs` | Add variant, `FilterEntityOptions` field, field key/default/summary/editor/picker |
| `crates/frontend/src/routes/shelf_page.rs` | Fetch manual shelves in `get_filter_entity_options`, pass `current_shelf_id` to `FilterBuilder` |
