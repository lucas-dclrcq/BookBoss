use chrono::{DateTime, NaiveDate, Utc};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

// ── Frontend-side filter types ─────────────────────────────────────────────
//
// These mirror `bb_core::filter::*` exactly (same serde layout) so that JSON
// produced here can be deserialized server-side as
// `bb_core::filter::BookFilter`.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub(crate) enum BookFilter {
    Group(FilterGroup),
    Rule(FilterRule),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct FilterGroup {
    pub condition: FilterCondition,
    pub items: Vec<BookFilter>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FilterCondition {
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EntityRef {
    pub id: i64,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TextOp {
    Contains,
    DoesntContain,
    StartsWith,
    EndsWith,
    Equals,
    NotEquals,
    IsEmpty,
    IsNotEmpty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SetOp {
    IncludesAny,
    IncludesAll,
    ExcludesAll,
    IsEmpty,
    IsNotEmpty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NumericOp {
    Eq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DateOp {
    Before,
    After,
    IsEmpty,
    IsNotEmpty,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FilterReadStatus {
    Unread,
    Reading,
    Paused,
    Rereading,
    Read,
    Abandoned,
    Active,
}

impl FilterReadStatus {
    fn label(&self) -> &'static str {
        match self {
            Self::Unread => "Unread",
            Self::Reading => "Reading",
            Self::Paused => "Paused",
            Self::Rereading => "Rereading",
            Self::Read => "Read",
            Self::Abandoned => "Abandoned",
            Self::Active => "Active",
        }
    }
    fn all() -> &'static [Self] {
        &[
            Self::Active,
            Self::Unread,
            Self::Reading,
            Self::Paused,
            Self::Rereading,
            Self::Read,
            Self::Abandoned,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "field", content = "params", rename_all = "snake_case")]
pub(crate) enum FilterRule {
    TitleText { op: TextOp, value: String },
    AuthorText { op: TextOp, value: String },
    Author { op: SetOp, values: Vec<EntityRef> },
    Series { op: SetOp, values: Vec<EntityRef> },
    Genre { op: SetOp, values: Vec<EntityRef> },
    Tag { op: SetOp, values: Vec<EntityRef> },
    Publisher { op: SetOp, values: Vec<EntityRef> },
    Language { op: SetOp, values: Vec<String> },
    ReadStatus { op: SetOp, values: Vec<FilterReadStatus> },
    Rating { op: NumericOp, value: u8 },
    DateAdded { op: DateOp, value: Option<DateTime<Utc>> },
}

// ── DTO ──────────────────────────────────────────────────────────────────────

/// All entity options for the filter builder's entity pickers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub(crate) struct FilterEntityOptions {
    pub authors: Vec<(i64, String)>,
    pub series: Vec<(i64, String)>,
    pub genres: Vec<(i64, String)>,
    pub tags: Vec<(i64, String)>,
    pub publishers: Vec<(i64, String)>,
}

// ── Public helpers
// ────────────────────────────────────────────────────────────

/// Converts a `BookFilter` into a compact human-readable summary string,
/// suitable for use as a tooltip.
pub(crate) fn filter_to_summary(filter: &BookFilter) -> String {
    match filter {
        BookFilter::Group(g) => group_to_summary(g, true),
        BookFilter::Rule(r) => rule_to_summary(r),
    }
}

fn group_to_summary(group: &FilterGroup, is_top: bool) -> String {
    if group.items.is_empty() {
        return String::from("(empty filter)");
    }
    let join = match group.condition {
        FilterCondition::And => " AND ",
        FilterCondition::Or => " OR ",
    };
    let parts: Vec<String> = group
        .items
        .iter()
        .map(|item| match item {
            BookFilter::Group(g) => format!("({})", group_to_summary(g, false)),
            BookFilter::Rule(r) => rule_to_summary(r),
        })
        .collect();
    if is_top && parts.len() == 1 {
        return parts.into_iter().next().unwrap();
    }
    parts.join(join)
}

fn rule_to_summary(rule: &FilterRule) -> String {
    match rule {
        FilterRule::TitleText { op, value } => {
            if matches!(op, TextOp::IsEmpty | TextOp::IsNotEmpty) {
                format!("title {}", text_op_summary(op))
            } else {
                format!("title {} \"{}\"", text_op_summary(op), value)
            }
        }
        FilterRule::AuthorText { op, value } => {
            if matches!(op, TextOp::IsEmpty | TextOp::IsNotEmpty) {
                format!("author text {}", text_op_summary(op))
            } else {
                format!("author text {} \"{}\"", text_op_summary(op), value)
            }
        }
        FilterRule::Author { op, values } => format!("author {} {}", set_op_summary(op), entity_label_list(values)),
        FilterRule::Series { op, values } => format!("series {} {}", set_op_summary(op), entity_label_list(values)),
        FilterRule::Genre { op, values } => format!("genre {} {}", set_op_summary(op), entity_label_list(values)),
        FilterRule::Tag { op, values } => format!("tag {} {}", set_op_summary(op), entity_label_list(values)),
        FilterRule::Publisher { op, values } => format!("publisher {} {}", set_op_summary(op), entity_label_list(values)),
        FilterRule::Language { op, values } => format!("language {} {}", set_op_summary(op), values.join(", ")),
        FilterRule::ReadStatus { op, values } => {
            let labels: Vec<&str> = values.iter().map(FilterReadStatus::label).collect();
            format!("status {} {}", set_op_summary(op), labels.join(", "))
        }
        FilterRule::Rating { op, value } => format!("rating {} {value}", numeric_op_summary(op)),
        FilterRule::DateAdded { op, value } => {
            if matches!(op, DateOp::IsEmpty | DateOp::IsNotEmpty) {
                format!("date added {}", date_op_summary(op))
            } else if let Some(dt) = value {
                format!("date added {} {}", date_op_summary(op), dt.format("%Y-%m-%d"))
            } else {
                format!("date added {}", date_op_summary(op))
            }
        }
    }
}

fn entity_label_list(values: &[EntityRef]) -> String {
    if values.is_empty() {
        return String::from("(none)");
    }
    values.iter().map(|e| e.label.as_str()).collect::<Vec<_>>().join(", ")
}

fn text_op_summary(op: &TextOp) -> &'static str {
    match op {
        TextOp::Contains => "contains",
        TextOp::DoesntContain => "doesn't contain",
        TextOp::StartsWith => "starts with",
        TextOp::EndsWith => "ends with",
        TextOp::Equals => "is",
        TextOp::NotEquals => "is not",
        TextOp::IsEmpty => "is empty",
        TextOp::IsNotEmpty => "is not empty",
    }
}

fn set_op_summary(op: &SetOp) -> &'static str {
    match op {
        SetOp::IncludesAny => "is any of",
        SetOp::IncludesAll => "includes all of",
        SetOp::ExcludesAll => "excludes",
        SetOp::IsEmpty => "is empty",
        SetOp::IsNotEmpty => "is not empty",
    }
}

fn numeric_op_summary(op: &NumericOp) -> &'static str {
    match op {
        NumericOp::Eq => "=",
        NumericOp::NotEq => "≠",
        NumericOp::Lt => "<",
        NumericOp::Lte => "≤",
        NumericOp::Gt => ">",
        NumericOp::Gte => "≥",
    }
}

fn date_op_summary(op: &DateOp) -> &'static str {
    match op {
        DateOp::Before => "before",
        DateOp::After => "after",
        DateOp::IsEmpty => "is empty",
        DateOp::IsNotEmpty => "is not empty",
    }
}

/// Default starting filter for a new smart shelf (AND group with one empty
/// title rule).
pub(crate) fn default_book_filter() -> BookFilter {
    BookFilter::Group(FilterGroup {
        condition: FilterCondition::And,
        items: vec![BookFilter::Rule(FilterRule::TitleText {
            op: TextOp::Contains,
            value: String::new(),
        })],
    })
}

// ── Internal helpers
// ──────────────────────────────────────────────────────────

fn word_match(candidate: &str, query: &str) -> bool {
    let lower = candidate.to_lowercase();
    query.split_whitespace().all(|w| lower.contains(&w.to_lowercase()))
}

fn field_key(rule: &FilterRule) -> &'static str {
    match rule {
        FilterRule::TitleText { .. } => "title_text",
        FilterRule::AuthorText { .. } => "author_text",
        FilterRule::Author { .. } => "author",
        FilterRule::Series { .. } => "series",
        FilterRule::Genre { .. } => "genre",
        FilterRule::Tag { .. } => "tag",
        FilterRule::Publisher { .. } => "publisher",
        FilterRule::Language { .. } => "language",
        FilterRule::ReadStatus { .. } => "read_status",
        FilterRule::Rating { .. } => "rating",
        FilterRule::DateAdded { .. } => "date_added",
    }
}

fn default_rule_for_field(field: &str) -> FilterRule {
    match field {
        "author_text" => FilterRule::AuthorText {
            op: TextOp::Contains,
            value: String::new(),
        },
        "author" => FilterRule::Author {
            op: SetOp::IncludesAny,
            values: vec![],
        },
        "series" => FilterRule::Series {
            op: SetOp::IncludesAny,
            values: vec![],
        },
        "genre" => FilterRule::Genre {
            op: SetOp::IncludesAny,
            values: vec![],
        },
        "tag" => FilterRule::Tag {
            op: SetOp::IncludesAny,
            values: vec![],
        },
        "publisher" => FilterRule::Publisher {
            op: SetOp::IncludesAny,
            values: vec![],
        },
        "language" => FilterRule::Language {
            op: SetOp::IncludesAny,
            values: vec![],
        },
        "read_status" => FilterRule::ReadStatus {
            op: SetOp::IncludesAny,
            values: vec![],
        },
        "rating" => FilterRule::Rating { op: NumericOp::Gte, value: 1 },
        "date_added" => FilterRule::DateAdded {
            op: DateOp::After,
            value: None,
        },
        // "title_text" => FilterRule::TitleText {
        //     op: TextOp::Contains,
        //     value: String::new(),
        // },
        _ => FilterRule::TitleText {
            op: TextOp::Contains,
            value: String::new(),
        },
    }
}

fn text_op_key(op: &TextOp) -> &'static str {
    match op {
        TextOp::Contains => "contains",
        TextOp::DoesntContain => "doesnt_contain",
        TextOp::StartsWith => "starts_with",
        TextOp::EndsWith => "ends_with",
        TextOp::Equals => "equals",
        TextOp::NotEquals => "not_equals",
        TextOp::IsEmpty => "is_empty",
        TextOp::IsNotEmpty => "is_not_empty",
    }
}

fn parse_text_op(s: &str) -> TextOp {
    match s {
        "doesnt_contain" => TextOp::DoesntContain,
        "starts_with" => TextOp::StartsWith,
        "ends_with" => TextOp::EndsWith,
        "equals" => TextOp::Equals,
        "not_equals" => TextOp::NotEquals,
        "is_empty" => TextOp::IsEmpty,
        "is_not_empty" => TextOp::IsNotEmpty,
        // "contains" => TextOp::Contains,
        _ => TextOp::Contains,
    }
}

fn set_op_key(op: &SetOp) -> &'static str {
    match op {
        SetOp::IncludesAny => "includes_any",
        SetOp::IncludesAll => "includes_all",
        SetOp::ExcludesAll => "excludes_all",
        SetOp::IsEmpty => "is_empty",
        SetOp::IsNotEmpty => "is_not_empty",
    }
}

fn parse_set_op(s: &str) -> SetOp {
    match s {
        "includes_all" => SetOp::IncludesAll,
        "excludes_all" => SetOp::ExcludesAll,
        "is_empty" => SetOp::IsEmpty,
        "is_not_empty" => SetOp::IsNotEmpty,
        // "includes_any" => SetOp::IncludesAny,
        _ => SetOp::IncludesAny,
    }
}

fn numeric_op_key(op: &NumericOp) -> &'static str {
    match op {
        NumericOp::Eq => "eq",
        NumericOp::NotEq => "not_eq",
        NumericOp::Lt => "lt",
        NumericOp::Lte => "lte",
        NumericOp::Gt => "gt",
        NumericOp::Gte => "gte",
    }
}

fn parse_numeric_op(s: &str) -> NumericOp {
    match s {
        "eq" => NumericOp::Eq,
        "not_eq" => NumericOp::NotEq,
        "lt" => NumericOp::Lt,
        "lte" => NumericOp::Lte,
        "gt" => NumericOp::Gt,
        // "gte" => NumericOp::Gte,
        _ => NumericOp::Gte,
    }
}

fn date_op_key(op: &DateOp) -> &'static str {
    match op {
        DateOp::Before => "before",
        DateOp::After => "after",
        DateOp::IsEmpty => "is_empty",
        DateOp::IsNotEmpty => "is_not_empty",
    }
}

fn parse_date_op(s: &str) -> DateOp {
    match s {
        "before" => DateOp::Before,
        "is_empty" => DateOp::IsEmpty,
        "is_not_empty" => DateOp::IsNotEmpty,
        // "after" => DateOp::After,
        _ => DateOp::After,
    }
}

fn date_str_to_datetime(s: &str) -> Option<DateTime<Utc>> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc())
}

fn datetime_to_date_str(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d").to_string()
}

// ── Components
// ────────────────────────────────────────────────────────────────

/// Top-level filter builder. Holds the `BookFilter` tree and renders a
/// `FilterGroupEditor` for the root group.
#[component]
pub(crate) fn FilterBuilder(mut filter: Signal<BookFilter>, entity_options: FilterEntityOptions) -> Element {
    let root = filter();
    let root_group = match root {
        BookFilter::Group(g) => g,
        BookFilter::Rule(r) => FilterGroup {
            condition: FilterCondition::And,
            items: vec![BookFilter::Rule(r)],
        },
    };
    rsx! {
        FilterGroupEditor {
            group: root_group,
            entity_options,
            is_root: true,
            on_change: move |g: FilterGroup| filter.set(BookFilter::Group(g)),
            on_remove: move |()| {},
        }
    }
}

/// Renders a filter group (AND/OR) with its child rules and sub-groups.
#[component]
fn FilterGroupEditor(
    group: FilterGroup,
    entity_options: FilterEntityOptions,
    is_root: bool,
    on_change: EventHandler<FilterGroup>,
    on_remove: EventHandler<()>,
) -> Element {
    let is_and = group.condition == FilterCondition::And;

    let condition_oc = on_change;
    let condition_group = group.clone();
    let add_rule_oc = on_change;
    let add_rule_group = group.clone();
    let add_group_oc = on_change;
    let add_group_group = group.clone();

    // Root group has a subtle card look; sub-groups use a left accent border.
    let container_class = if is_root {
        "rounded-lg border border-gray-200 bg-gray-50 p-3 space-y-2"
    } else {
        "rounded-lg border border-gray-200 bg-gray-50 border-l-4 border-l-indigo-400 pl-3 pr-3 pt-3 pb-3 space-y-2"
    };

    let select_class = "text-sm border border-gray-300 rounded px-2 py-1 bg-white focus:outline-none focus:ring-1 focus:ring-indigo-400";

    rsx! {
        div { class: "{container_class}",

            // ── Condition row ────────────────────────────────────────────────
            div { class: "flex items-center gap-2 flex-wrap",
                span { class: "text-sm font-medium text-gray-600", "Condition:" }
                select {
                    class: select_class,
                    value: if is_and { "and" } else { "or" },
                    onchange: move |e| {
                        let mut g = condition_group.clone();
                        g.condition = if e.value() == "and" { FilterCondition::And } else { FilterCondition::Or };
                        condition_oc.call(g);
                    },
                    option { value: "and", "AND — all rules must match" }
                    option { value: "or", "OR — any rule must match" }
                }
                button {
                    r#type: "button",
                    class: "text-xs font-medium text-indigo-600 hover:text-indigo-800 border border-indigo-300 rounded px-2 py-1 hover:bg-indigo-50",
                    onclick: move |_| {
                        let mut g = add_rule_group.clone();
                        g.items.push(BookFilter::Rule(FilterRule::TitleText {
                            op: TextOp::Contains,
                            value: String::new(),
                        }));
                        add_rule_oc.call(g);
                    },
                    "+ Add Rule"
                }
                button {
                    r#type: "button",
                    class: "text-xs font-medium text-gray-600 hover:text-gray-800 border border-gray-300 rounded px-2 py-1 hover:bg-gray-100",
                    onclick: move |_| {
                        let mut g = add_group_group.clone();
                        g.items.push(BookFilter::Group(FilterGroup {
                            condition: FilterCondition::And,
                            items: vec![BookFilter::Rule(FilterRule::TitleText {
                                op: TextOp::Contains,
                                value: String::new(),
                            })],
                        }));
                        add_group_oc.call(g);
                    },
                    "+ Add Group"
                }
            }

            // ── Items ────────────────────────────────────────────────────────
            for (i, item) in group.items.iter().cloned().enumerate() {
                {
                    let oc1 = on_change;
                    let oc2 = on_change;
                    let or2 = on_remove;
                    let gc1 = group.clone();
                    let gc2 = group.clone();
                    match item {
                        BookFilter::Rule(rule) => rsx! {
                            FilterRuleRow {
                                key: "{i}",
                                rule,
                                entity_options: entity_options.clone(),
                                on_change: move |new_rule: FilterRule| {
                                    let mut g = gc1.clone();
                                    g.items[i] = BookFilter::Rule(new_rule);
                                    oc1.call(g);
                                },
                                on_remove: move |()| {
                                    let mut g = gc2.clone();
                                    g.items.remove(i);
                                    if g.items.is_empty() && !is_root {
                                        or2.call(());
                                    } else {
                                        oc2.call(g);
                                    }
                                },
                            }
                        },
                        BookFilter::Group(sub) => rsx! {
                            FilterGroupEditor {
                                key: "{i}",
                                group: sub,
                                entity_options: entity_options.clone(),
                                is_root: false,
                                on_change: move |new_sub: FilterGroup| {
                                    let mut g = gc1.clone();
                                    g.items[i] = BookFilter::Group(new_sub);
                                    oc1.call(g);
                                },
                                on_remove: move |()| {
                                    let mut g = gc2.clone();
                                    g.items.remove(i);
                                    if g.items.is_empty() && !is_root {
                                        or2.call(());
                                    } else {
                                        oc2.call(g);
                                    }
                                },
                            }
                        },
                    }
                }
            }
        }
    }
}

/// A single filter rule row: [field ▼] [operator ▼] [value input] [× remove].
#[component]
fn FilterRuleRow(rule: FilterRule, entity_options: FilterEntityOptions, on_change: EventHandler<FilterRule>, on_remove: EventHandler<()>) -> Element {
    let field = field_key(&rule);

    let select_class = "text-sm border border-gray-300 rounded px-2 py-1 bg-white focus:outline-none focus:ring-1 focus:ring-indigo-400";
    let input_class = "flex-1 text-sm border border-gray-300 rounded px-2 py-1 focus:outline-none focus:ring-1 focus:ring-indigo-400";

    // Field selector
    let oc_field = on_change;
    let field_select = rsx! {
        select {
            class: select_class,
            value: field,
            onchange: move |e| oc_field.call(default_rule_for_field(&e.value())),
            option { value: "title_text", "Title" }
            option { value: "author_text", "Author (text)" }
            option { value: "author", "Author" }
            option { value: "series", "Series" }
            option { value: "genre", "Genre" }
            option { value: "tag", "Tag" }
            option { value: "publisher", "Publisher" }
            option { value: "language", "Language" }
            option { value: "read_status", "Read Status" }
            option { value: "rating", "Rating" }
            option { value: "date_added", "Date Added" }
        }
    };

    // Rule-specific operator + value
    let rule_ui = match rule.clone() {
        FilterRule::TitleText { op, value } | FilterRule::AuthorText { op, value } => {
            let is_text_field = matches!(rule, FilterRule::TitleText { .. });
            let op_key = text_op_key(&op);
            let needs_value = !matches!(op, TextOp::IsEmpty | TextOp::IsNotEmpty);
            let oc_op = on_change;
            let current_value_for_op = value.clone();
            let current_op_for_val = op.clone();
            let oc_val = on_change;
            rsx! {
                select {
                    class: select_class,
                    value: op_key,
                    onchange: move |e| {
                        let new_op = parse_text_op(&e.value());
                        let v = current_value_for_op.clone();
                        let new_rule = if is_text_field {
                            FilterRule::TitleText { op: new_op, value: v }
                        } else {
                            FilterRule::AuthorText { op: new_op, value: v }
                        };
                        oc_op.call(new_rule);
                    },
                    option { value: "contains", "contains" }
                    option { value: "doesnt_contain", "doesn't contain" }
                    option { value: "starts_with", "starts with" }
                    option { value: "ends_with", "ends with" }
                    option { value: "equals", "equals" }
                    option { value: "not_equals", "not equals" }
                    option { value: "is_empty", "is empty" }
                    option { value: "is_not_empty", "is not empty" }
                }
                if needs_value {
                    input {
                        class: input_class,
                        r#type: "text",
                        value: "{value}",
                        oninput: move |e| {
                            let v = e.value();
                            let op = current_op_for_val.clone();
                            let new_rule = if is_text_field {
                                FilterRule::TitleText { op, value: v }
                            } else {
                                FilterRule::AuthorText { op, value: v }
                            };
                            oc_val.call(new_rule);
                        },
                    }
                }
            }
        }

        FilterRule::Author { op, values } => {
            let options = entity_options.authors.clone();
            entity_set_rule_ui(select_class, op, values, options, on_change, |op, values| FilterRule::Author { op, values })
        }
        FilterRule::Series { op, values } => {
            let options = entity_options.series.clone();
            entity_set_rule_ui(select_class, op, values, options, on_change, |op, values| FilterRule::Series { op, values })
        }
        FilterRule::Genre { op, values } => {
            let options = entity_options.genres.clone();
            entity_set_rule_ui(select_class, op, values, options, on_change, |op, values| FilterRule::Genre { op, values })
        }
        FilterRule::Tag { op, values } => {
            let options = entity_options.tags.clone();
            entity_set_rule_ui(select_class, op, values, options, on_change, |op, values| FilterRule::Tag { op, values })
        }
        FilterRule::Publisher { op, values } => {
            let options = entity_options.publishers.clone();
            entity_set_rule_ui(select_class, op, values, options, on_change, |op, values| FilterRule::Publisher { op, values })
        }

        FilterRule::Language { op, values } => {
            let op_key = set_op_key(&op);
            let needs_value = !matches!(op, SetOp::IsEmpty | SetOp::IsNotEmpty);
            let oc_op = on_change;
            let op_for_val = op.clone();
            let values_for_val = values.clone();
            let oc_val = on_change;
            rsx! {
                select {
                    class: select_class,
                    value: op_key,
                    onchange: move |e| {
                        oc_op.call(FilterRule::Language { op: parse_set_op(&e.value()), values: values.clone() });
                    },
                    option { value: "includes_any", "includes any" }
                    option { value: "excludes_all", "excludes all" }
                    option { value: "is_empty", "is empty" }
                    option { value: "is_not_empty", "is not empty" }
                }
                if needs_value {
                    div { class: "flex-1",
                        LanguageChipInput {
                            values: values_for_val,
                            on_change: move |new_vals: Vec<String>| {
                                oc_val.call(FilterRule::Language { op: op_for_val.clone(), values: new_vals });
                            },
                        }
                    }
                }
            }
        }

        FilterRule::ReadStatus { op, values } => {
            let op_key = set_op_key(&op);
            let needs_value = !matches!(op, SetOp::IsEmpty | SetOp::IsNotEmpty);
            let oc_op = on_change;
            let op_for_val = op.clone();
            let oc_val = on_change;
            rsx! {
                select {
                    class: select_class,
                    value: op_key,
                    onchange: move |e| {
                        oc_op.call(FilterRule::ReadStatus { op: parse_set_op(&e.value()), values: values.clone() });
                    },
                    option { value: "includes_any", "is" }
                    option { value: "excludes_all", "is not" }
                    option { value: "is_empty", "is empty" }
                    option { value: "is_not_empty", "is not empty" }
                }
                if needs_value {
                    div { class: "flex-1 min-w-[200px]",
                        ReadStatusChipInput {
                            values: values.clone(),
                            on_change: move |new_vals: Vec<FilterReadStatus>| {
                                oc_val.call(FilterRule::ReadStatus { op: op_for_val.clone(), values: new_vals });
                            },
                        }
                    }
                }
            }
        }

        FilterRule::Rating { op, value } => {
            let op_key = numeric_op_key(&op);
            let oc_op = on_change;
            let op_for_val = op.clone();
            let oc_val = on_change;
            rsx! {
                select {
                    class: select_class,
                    value: op_key,
                    onchange: move |e| {
                        oc_op.call(FilterRule::Rating { op: parse_numeric_op(&e.value()), value });
                    },
                    option { value: "eq", "=" }
                    option { value: "not_eq", "≠" }
                    option { value: "lt", "<" }
                    option { value: "lte", "≤" }
                    option { value: "gt", ">" }
                    option { value: "gte", "≥" }
                }
                select {
                    class: select_class,
                    value: "{value}",
                    onchange: move |e| {
                        let v: u8 = e.value().parse().unwrap_or(1);
                        oc_val.call(FilterRule::Rating { op: op_for_val.clone(), value: v });
                    },
                    option { value: "1", "★" }
                    option { value: "2", "★★" }
                    option { value: "3", "★★★" }
                    option { value: "4", "★★★★" }
                    option { value: "5", "★★★★★" }
                }
            }
        }

        FilterRule::DateAdded { op, value } => {
            let op_key = date_op_key(&op);
            let needs_value = !matches!(op, DateOp::IsEmpty | DateOp::IsNotEmpty);
            let date_str = value.as_ref().map(datetime_to_date_str).unwrap_or_default();
            let oc_op = on_change;
            let op_for_val = op.clone();
            let oc_val = on_change;
            rsx! {
                select {
                    class: select_class,
                    value: op_key,
                    onchange: move |e| {
                        oc_op.call(FilterRule::DateAdded { op: parse_date_op(&e.value()), value });
                    },
                    option { value: "before", "before" }
                    option { value: "after", "after" }
                    option { value: "is_empty", "is empty" }
                    option { value: "is_not_empty", "is not empty" }
                }
                if needs_value {
                    input {
                        class: input_class,
                        r#type: "date",
                        value: "{date_str}",
                        oninput: move |e| {
                            let dt = date_str_to_datetime(&e.value());
                            oc_val.call(FilterRule::DateAdded { op: op_for_val.clone(), value: dt });
                        },
                    }
                }
            }
        }
    };

    let is_read_status = matches!(rule, FilterRule::ReadStatus { .. });

    rsx! {
        div { class: "flex flex-col gap-0.5",
            div { class: "flex items-center gap-2 flex-wrap",
                { field_select }
                { rule_ui }
                button {
                    r#type: "button",
                    class: "text-gray-400 hover:text-red-500 text-lg leading-none flex-shrink-0",
                    onclick: move |_| on_remove.call(()),
                    "×"
                }
            }
            if is_read_status {
                p { class: "text-xs text-gray-400 text-center leading-snug",
                    "Active (Unread · Reading · Rereading) · Paused · Read · Abandoned"
                }
            }
        }
    }
}

/// Shared helper to render an entity-picker set rule (Author, Series, Genre,
/// Tag, Publisher).
fn entity_set_rule_ui(
    select_class: &'static str,
    op: SetOp,
    values: Vec<EntityRef>,
    options: Vec<(i64, String)>,
    on_change: EventHandler<FilterRule>,
    make_rule: impl Fn(SetOp, Vec<EntityRef>) -> FilterRule + 'static + Clone,
) -> Element {
    let op_key = set_op_key(&op);
    let needs_value = !matches!(op, SetOp::IsEmpty | SetOp::IsNotEmpty);
    let oc_op = on_change;
    let make_rule_op = make_rule.clone();
    let values_for_op = values.clone();
    let op_for_val = op.clone();
    let make_rule_val = make_rule.clone();
    let oc_val = on_change;
    rsx! {
        select {
            class: select_class,
            value: op_key,
            onchange: move |e| {
                oc_op.call(make_rule_op(parse_set_op(&e.value()), values_for_op.clone()));
            },
            option { value: "includes_any", "includes any" }
            option { value: "includes_all", "includes all" }
            option { value: "excludes_all", "excludes all" }
            option { value: "is_empty", "is empty" }
            option { value: "is_not_empty", "is not empty" }
        }
        if needs_value {
            div { class: "flex-1 min-w-[200px]",
                EntityPicker {
                    values,
                    options,
                    on_change: move |new_vals: Vec<EntityRef>| {
                        oc_val.call(make_rule_val(op_for_val.clone(), new_vals));
                    },
                }
            }
        }
    }
}

/// Entity chip-picker for `Vec<EntityRef>` (id + label).
#[component]
fn EntityPicker(values: Vec<EntityRef>, options: Vec<(i64, String)>, on_change: EventHandler<Vec<EntityRef>>) -> Element {
    let mut input_text = use_signal(String::new);
    let mut show_dropdown = use_signal(|| false);

    let query = input_text.read().clone();
    let selected_ids: Vec<i64> = values.iter().map(|e| e.id).collect();

    let filtered: Vec<(i64, String)> = if query.is_empty() {
        vec![]
    } else {
        options
            .iter()
            .filter(|(id, label)| word_match(label, &query) && !selected_ids.contains(id))
            .take(8)
            .cloned()
            .collect()
    };

    rsx! {
        div { class: "relative",
            div { class: "flex flex-wrap gap-1 items-center border border-gray-300 rounded px-2 py-1 min-h-[34px] focus-within:ring-1 focus-within:ring-indigo-400",
                for (i, entity) in values.iter().enumerate() {
                    {
                        let label = entity.label.clone();
                        let new_values: Vec<EntityRef> = values.iter().cloned().enumerate().filter(|(j, _)| *j != i).map(|(_, e)| e).collect();
                        let oc = on_change;
                        rsx! {
                            span {
                                key: "{i}",
                                class: "inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-indigo-100 text-indigo-800 border border-indigo-300",
                                "{label}"
                                button {
                                    r#type: "button",
                                    class: "ml-0.5 text-indigo-400 hover:text-indigo-700 cursor-pointer leading-none",
                                    onclick: move |_| oc.call(new_values.clone()),
                                    "×"
                                }
                            }
                        }
                    }
                }
                input {
                    class: "flex-1 min-w-[120px] text-sm outline-none bg-transparent py-0.5",
                    value: "{input_text}",
                    placeholder: if values.is_empty() { "Search…" } else { "" },
                    oninput: move |e| {
                        let v = e.value();
                        let nonempty = !v.is_empty();
                        input_text.set(v);
                        show_dropdown.set(nonempty);
                    },
                    onkeydown: move |e| {
                        if e.key() == Key::Escape {
                            input_text.set(String::new());
                            show_dropdown.set(false);
                        }
                    },
                    onfocusout: move |_| show_dropdown.set(false),
                }
            }
            if *show_dropdown.read() && !filtered.is_empty() {
                div { class: "absolute left-0 right-0 top-full mt-1 bg-white border border-gray-200 rounded shadow-lg z-50 max-h-48 overflow-y-auto",
                    for (opt_id, opt_label) in &filtered {
                        {
                            let id = *opt_id;
                            let label = opt_label.clone();
                            let display = label.clone();
                            let mut new_values = values.clone();
                            new_values.push(EntityRef { id, label });
                            let oc = on_change;
                            rsx! {
                                div {
                                    key: "{id}",
                                    class: "px-3 py-1.5 text-sm text-gray-700 hover:bg-indigo-50 cursor-pointer",
                                    onmousedown: move |e| e.prevent_default(),
                                    onclick: move |_| {
                                        oc.call(new_values.clone());
                                        input_text.set(String::new());
                                        show_dropdown.set(false);
                                    },
                                    "{display}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Chip input for free-text `Vec<String>` (used for Language filter).
#[component]
fn LanguageChipInput(values: Vec<String>, on_change: EventHandler<Vec<String>>) -> Element {
    let mut input_text = use_signal(String::new);

    rsx! {
        div { class: "flex flex-wrap gap-1 items-center border border-gray-300 rounded px-2 py-1 min-h-[34px] focus-within:ring-1 focus-within:ring-indigo-400",
            for (i, chip) in values.iter().enumerate() {
                {
                    let label = chip.clone();
                    let new_values: Vec<String> = values.iter().cloned().enumerate().filter(|(j, _)| *j != i).map(|(_, v)| v).collect();
                    let oc = on_change;
                    rsx! {
                        span {
                            key: "{i}",
                            class: "inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-gray-100 text-gray-700 border border-gray-300",
                            "{label}"
                            button {
                                r#type: "button",
                                class: "ml-0.5 text-gray-400 hover:text-gray-700 cursor-pointer leading-none",
                                onclick: move |_| oc.call(new_values.clone()),
                                "×"
                            }
                        }
                    }
                }
            }
            input {
                class: "flex-1 min-w-[80px] text-sm outline-none bg-transparent py-0.5",
                value: "{input_text}",
                placeholder: if values.is_empty() { "e.g. en, fr" } else { "" },
                oninput: move |e| input_text.set(e.value()),
                onkeydown: move |e: KeyboardEvent| {
                    match e.key() {
                        Key::Enter => {
                            e.prevent_default();
                            let text = input_text.read().trim().to_string();
                            if !text.is_empty() {
                                let mut new_vals = values.clone();
                                if !new_vals.iter().any(|v| v.eq_ignore_ascii_case(&text)) {
                                    new_vals.push(text);
                                    on_change.call(new_vals);
                                }
                                input_text.set(String::new());
                            }
                        }
                        Key::Backspace if input_text.read().is_empty() => {
                            let mut new_vals = values.clone();
                            if !new_vals.is_empty() {
                                new_vals.pop();
                                on_change.call(new_vals);
                            }
                        }
                        Key::Escape => input_text.set(String::new()),
                        _ => {}
                    }
                },
            }
        }
    }
}

/// Chip-picker for `Vec<FilterReadStatus>`.
///
/// Opens a dropdown on focus showing all unselected statuses (filterable by
/// typing). A hint line below the input lists all available options with the
/// `Active` alias expanded.
#[component]
fn ReadStatusChipInput(values: Vec<FilterReadStatus>, on_change: EventHandler<Vec<FilterReadStatus>>) -> Element {
    let mut input_text = use_signal(String::new);
    let mut show_dropdown = use_signal(|| false);

    let query = input_text.read().to_lowercase();

    // Owned vec so it can be shared between the Enter handler and the dropdown.
    // Prefix match so "Read" finds "Read" and "Reading" but not "Rereading".
    // Sorted alphabetically so shorter/exact matches naturally rise to the top.
    let mut filtered: Vec<FilterReadStatus> = FilterReadStatus::all()
        .iter()
        .filter(|s| {
            let label = s.label().to_lowercase();
            (query.is_empty() || label.starts_with(&query)) && !values.iter().any(|v| v == *s)
        })
        .cloned()
        .collect();
    filtered.sort_by(|a, b| a.label().cmp(b.label()));

    // Pre-compute the top match so the Enter closure doesn't need to own
    // `filtered`.
    let first_filtered = filtered.first().cloned();

    rsx! {
        div { class: "relative",
            div { class: "flex flex-wrap gap-1 items-center border border-gray-300 rounded px-2 py-1 min-h-[34px] focus-within:ring-1 focus-within:ring-indigo-400",
                for (i, status) in values.iter().enumerate() {
                    {
                        let label = status.label();
                        let new_values: Vec<FilterReadStatus> = values
                            .iter()
                            .cloned()
                            .enumerate()
                            .filter(|(j, _)| *j != i)
                            .map(|(_, v)| v)
                            .collect();
                        let oc = on_change;
                        rsx! {
                            span {
                                key: "{i}",
                                class: "inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs bg-indigo-100 text-indigo-800 border border-indigo-300",
                                "{label}"
                                button {
                                    r#type: "button",
                                    class: "ml-0.5 text-indigo-400 hover:text-indigo-700 cursor-pointer leading-none",
                                    onclick: move |_| oc.call(new_values.clone()),
                                    "×"
                                }
                            }
                        }
                    }
                }
                input {
                    class: "flex-1 min-w-[80px] text-sm outline-none bg-transparent py-0.5",
                    value: "{input_text}",
                    placeholder: if values.is_empty() { "add status…" } else { "" },
                    onfocus: move |_| show_dropdown.set(true),
                    oninput: move |e| {
                        input_text.set(e.value());
                        show_dropdown.set(true);
                    },
                    onfocusout: move |_| show_dropdown.set(false),
                    onkeydown: move |e: KeyboardEvent| {
                        match e.key() {
                            Key::Enter => {
                                e.prevent_default();
                                if let Some(first) = first_filtered.clone() {
                                    let mut new_vals = values.clone();
                                    new_vals.push(first);
                                    on_change.call(new_vals);
                                    input_text.set(String::new());
                                }
                            }
                            Key::Escape => {
                                show_dropdown.set(false);
                                input_text.set(String::new());
                            }
                            Key::Backspace if input_text.read().is_empty() => {
                                let mut new_vals = values.clone();
                                if !new_vals.is_empty() {
                                    new_vals.pop();
                                    on_change.call(new_vals);
                                }
                            }
                            _ => {}
                        }
                    },
                }
            }
            if *show_dropdown.read() && !filtered.is_empty() {
                div { class: "absolute left-0 right-0 bottom-full mb-1 bg-white border border-gray-200 rounded shadow-lg z-50 max-h-48 overflow-y-auto",
                    for status in &filtered {
                        {
                            let label = status.label();
                            let mut new_values = values.clone();
                            new_values.push(status.clone());
                            let oc = on_change;
                            rsx! {
                                div {
                                    key: "{label}",
                                    class: "px-3 py-1.5 text-sm text-gray-700 hover:bg-indigo-50 cursor-pointer",
                                    onmousedown: move |e| e.prevent_default(),
                                    onclick: move |_| {
                                        oc.call(new_values.clone());
                                        input_text.set(String::new());
                                    },
                                    "{label}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
