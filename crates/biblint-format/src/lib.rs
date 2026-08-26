//! Canonical BibTeX formatting and cleanup transforms.

use biblint_syntax::{Comment, Document, Entry, Field, Item, Raw, Special, Value, ValuePart};
use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

mod formula;
mod key;

pub use formula::FormulaError;
pub use key::{
    GOOGLE_SCHOLAR_FORMULA, better_bibtex_key_base, better_bibtex_key_base_with_formula,
    better_bibtex_key_suggestions, better_bibtex_key_suggestions_with_formula,
};

pub const DEFAULT_SPACE: usize = 2;
pub const DEFAULT_WRAP: usize = 80;

pub const DEFAULT_FIELD_ORDER: &[&str] = &[
    "title",
    "shorttitle",
    "author",
    "year",
    "month",
    "day",
    "journal",
    "booktitle",
    "location",
    "on",
    "publisher",
    "address",
    "series",
    "volume",
    "number",
    "pages",
    "doi",
    "isbn",
    "issn",
    "url",
    "urldate",
    "copyright",
    "category",
    "note",
    "metadata",
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct KeyGenerationOptions {
    /// A supported Better BibTeX formula evaluated against each BibTeX entry.
    pub formula: String,
}

impl Default for KeyGenerationOptions {
    fn default() -> Self {
        Self {
            formula: GOOGLE_SCHOLAR_FORMULA.to_string(),
        }
    }
}

impl KeyGenerationOptions {
    /// Validate the configured formula before a formatting or linting run.
    pub fn validate(&self) -> Result<(), FormulaError> {
        formula::validate_formula(&self.formula)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Indent {
    Spaces(usize),
    Tab,
}

impl<'de> Deserialize<'de> for Indent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum IndentSetting {
            Spaces(usize),
            Named(String),
        }

        match IndentSetting::deserialize(deserializer)? {
            IndentSetting::Spaces(count) => Ok(Self::Spaces(count)),
            IndentSetting::Named(name) if name.eq_ignore_ascii_case("tab") => Ok(Self::Tab),
            IndentSetting::Named(name) => Err(serde::de::Error::custom(format!(
                "invalid indent setting '{name}'; use a space count or 'tab'"
            ))),
        }
    }
}

impl Indent {
    fn string(&self) -> String {
        match self {
            Self::Spaces(count) => " ".repeat(*count),
            Self::Tab => "\t".to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DuplicateKind {
    Key,
    Doi,
    Citation,
    Abstract,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MergeStrategy {
    First,
    Last,
    Combine,
    Overwrite,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
#[derive(Deserialize)]
#[serde(
    default = "default_format_options",
    deny_unknown_fields,
    rename_all = "kebab-case"
)]
pub struct FormatOptions {
    pub indent: Indent,
    #[serde(deserialize_with = "deserialize_align")]
    pub align: Option<usize>,
    pub blank_lines: bool,
    pub lowercase: bool,
    pub curly: bool,
    pub numeric: bool,
    pub months: bool,
    #[serde(deserialize_with = "deserialize_sort")]
    pub sort: Option<Vec<String>>,
    #[serde(deserialize_with = "deserialize_sort_fields")]
    pub sort_fields: Option<Vec<String>>,
    pub duplicate_kinds: Vec<DuplicateKind>,
    pub merge: Option<MergeStrategy>,
    pub omit: Vec<String>,
    pub strip_enclosing_braces: bool,
    pub drop_all_caps: bool,
    pub escape: bool,
    pub unescape: bool,
    pub strip_comments: bool,
    pub trailing_commas: bool,
    pub encode_urls: bool,
    pub tidy_comments: bool,
    pub remove_empty_fields: bool,
    pub remove_duplicate_fields: bool,
    pub generate_keys: bool,
    #[serde(rename = "key-generation")]
    pub key_generation: KeyGenerationOptions,
    pub max_authors: Option<usize>,
    #[serde(deserialize_with = "deserialize_field_list_option")]
    pub enclosing_braces: Option<Vec<String>>,
    #[serde(deserialize_with = "deserialize_field_list_option")]
    pub remove_braces: Option<Vec<String>>,
    #[serde(deserialize_with = "deserialize_wrap")]
    pub wrap: Option<usize>,
    pub format_page_ranges: bool,
}

fn default_format_options() -> FormatOptions {
    FormatOptions::default()
}

fn deserialize_sort<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_optional_string_list(deserializer, &["key"])
}

fn deserialize_sort_fields<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_optional_string_list(deserializer, DEFAULT_FIELD_ORDER)
}

fn deserialize_field_list_option<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_optional_string_list(deserializer, &["title"])
}

fn deserialize_optional_string_list<'de, D>(
    deserializer: D,
    enabled: &[&str],
) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Setting {
        Values(Vec<String>),
        Enabled(bool),
    }

    match Setting::deserialize(deserializer)? {
        Setting::Values(values) => Ok(Some(values)),
        Setting::Enabled(true) => Ok(Some(
            enabled.iter().map(|value| (*value).to_string()).collect(),
        )),
        Setting::Enabled(false) => Ok(None),
    }
}

fn deserialize_wrap<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Setting {
        Width(usize),
        Enabled(bool),
    }

    match Setting::deserialize(deserializer)? {
        Setting::Width(width) => Ok(Some(width)),
        Setting::Enabled(true) => Ok(Some(DEFAULT_WRAP)),
        Setting::Enabled(false) => Ok(None),
    }
}

fn deserialize_align<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum AlignSetting {
        Column(usize),
        Enabled(bool),
    }

    match AlignSetting::deserialize(deserializer)? {
        AlignSetting::Column(column) => Ok(Some(column)),
        AlignSetting::Enabled(false) => Ok(None),
        AlignSetting::Enabled(true) => Err(serde::de::Error::custom(
            "align must be a column number or false",
        )),
    }
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            indent: Indent::Spaces(DEFAULT_SPACE),
            align: None,
            blank_lines: false,
            lowercase: true,
            curly: false,
            numeric: false,
            months: false,
            sort: None,
            sort_fields: None,
            duplicate_kinds: Vec::new(),
            merge: None,
            omit: Vec::new(),
            strip_enclosing_braces: false,
            drop_all_caps: false,
            escape: false,
            unescape: false,
            strip_comments: false,
            trailing_commas: false,
            encode_urls: false,
            tidy_comments: false,
            remove_empty_fields: false,
            remove_duplicate_fields: false,
            generate_keys: false,
            key_generation: KeyGenerationOptions::default(),
            max_authors: None,
            enclosing_braces: None,
            remove_braces: None,
            wrap: None,
            format_page_ranges: false,
        }
    }
}

impl FormatOptions {
    /// Whether applying this option set can change bibliographic meaning or
    /// identity rather than only whitespace and spelling.
    #[must_use]
    pub const fn has_unsafe_transforms(&self) -> bool {
        self.generate_keys
            || self.merge.is_some()
            || !self.omit.is_empty()
            || self.strip_enclosing_braces
            || self.drop_all_caps
            || self.unescape
            || self.strip_comments
            || self.remove_empty_fields
            || self.remove_duplicate_fields
            || self.max_authors.is_some()
            || self.enclosing_braces.is_some()
            || self.remove_braces.is_some()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormatResult {
    pub output: String,
    pub changed: bool,
}

/// Return non-ASCII characters that the built-in escape table cannot convert
/// to a package-independent LaTeX sequence.
#[must_use]
pub fn unsupported_escape_characters(value: &str) -> Vec<char> {
    let escaped = escape_text(value, false);
    let mut unsupported = Vec::new();
    for character in value.chars().filter(|character| !character.is_ascii()) {
        if escaped.contains(character) && !unsupported.contains(&character) {
            unsupported.push(character);
        }
    }
    unsupported
}

/// Format a parsed BibTeX document.
#[must_use]
pub fn format_document(document: &Document, options: &FormatOptions) -> FormatResult {
    let mut document = document.clone();
    transform_document(&mut document, options);
    let output = render_document(&document, options);
    FormatResult {
        changed: true,
        output,
    }
}

fn transform_document(document: &mut Document, options: &FormatOptions) {
    for item in &mut document.items {
        match item {
            Item::Entry(entry) => transform_entry(entry, options),
            Item::Special(special) => {
                if options.lowercase {
                    special.command = special.command.to_ascii_lowercase();
                }
            }
            Item::Comment(_) | Item::Raw(_) => {}
        }
    }

    if options.generate_keys
        && let Ok(suggestions) =
            better_bibtex_key_suggestions_with_formula(document, &options.key_generation.formula)
    {
        for (item, suggestion) in document.items.iter_mut().zip(suggestions) {
            if let (Item::Entry(entry), Some(key)) = (item, suggestion) {
                entry.key = Some(key);
            }
        }
    }
    if options.merge.is_some() {
        merge_duplicate_entries(document, options);
    }
    if let Some(order) = &options.sort_fields {
        for item in &mut document.items {
            if let Item::Entry(entry) = item {
                sort_fields(&mut entry.fields, order);
            }
        }
    }
    if let Some(sort) = &options.sort {
        sort_entries(document, sort);
    }
}

fn transform_entry(entry: &mut Entry, options: &FormatOptions) {
    if options.lowercase {
        entry.command = entry.command.to_ascii_lowercase();
    }
    let omit: HashSet<String> = options
        .omit
        .iter()
        .map(|field| field.to_ascii_lowercase())
        .collect();
    let enclosing = lowercase_set(options.enclosing_braces.as_deref());
    let remove_braces = lowercase_set(options.remove_braces.as_deref());

    for field in &mut entry.fields {
        if options.lowercase {
            field.name = field.name.to_ascii_lowercase();
        }
        let field_name = field.name.to_ascii_lowercase();
        if let Some(value) = &mut field.value {
            transform_value(value, &field_name, options, &enclosing, &remove_braces);
        }
    }
    entry.fields.retain(|field| {
        let name = field.name.to_ascii_lowercase();
        if omit.contains(&name) {
            return false;
        }
        if options.remove_empty_fields && field.value.as_ref().is_some_and(value_is_empty) {
            return false;
        }
        true
    });
    if options.remove_duplicate_fields {
        let mut seen = HashSet::new();
        entry
            .fields
            .retain(|field| seen.insert(field.name.to_ascii_lowercase()));
    }
    if let Some(order) = &options.sort_fields {
        sort_fields(&mut entry.fields, order);
    }
}

fn transform_value(
    value: &mut Value,
    field_name: &str,
    options: &FormatOptions,
    enclosing: &HashSet<String>,
    remove_braces: &HashSet<String>,
) {
    let is_verbatim = matches!(
        field_name,
        "url" | "doi" | "eprint" | "file" | "pdf" | "verba" | "verbb" | "verbc"
    );

    let rendered = value.render();
    let visible = visible_latex_text(&rendered);
    let all_caps = options.drop_all_caps
        && visible.chars().any(char::is_alphabetic)
        && !visible.chars().any(char::is_lowercase);
    for part in &mut value.parts {
        let is_braced = part.is_braced();
        let is_quoted = part.is_quoted();
        let is_textual = is_braced || is_quoted;
        let original = part.render().to_string();
        if field_name == "month"
            && options.months
            && let Some(month) = abbreviated_month(&original)
        {
            *part = ValuePart::Literal(month);
            continue;
        }
        let mut text = normalize_value_whitespace(&original);
        if options.unescape && is_textual {
            text = unescape_text(&text, is_quoted);
        } else if options.escape && is_textual && !is_verbatim {
            text = escape_text(&text, is_quoted);
        }
        if options.format_page_ranges && field_name == "pages" && is_textual {
            text = format_page_ranges(&text);
        }
        if options.encode_urls && field_name == "url" && is_textual {
            text = encode_url(&text);
        }
        if all_caps && is_textual {
            text = title_case(&text);
        }
        if options.strip_enclosing_braces && is_braced {
            text = strip_one_outer_brace_group(&text);
        }
        if remove_braces.contains(field_name) && is_braced {
            text = remove_text_braces(&text);
        }
        *part = if is_braced {
            ValuePart::Braced(text)
        } else if is_quoted {
            ValuePart::Quoted(text)
        } else {
            ValuePart::Literal(text)
        };
    }

    if enclosing.contains(field_name) {
        for part in &mut value.parts {
            if let ValuePart::Braced(text) = part
                && !is_already_double_enclosed(text)
            {
                *text = format!("{{{text}}}");
            }
        }
    }
    if options.curly {
        let is_month_macro = field_name == "month" && abbreviated_month(&value.render()).is_some();
        if !is_month_macro {
            for part in &mut value.parts {
                let replacement = match part {
                    ValuePart::Braced(_) => None,
                    ValuePart::Quoted(text) | ValuePart::Literal(text) => {
                        Some(ValuePart::Braced(text.clone()))
                    }
                };
                if let Some(replacement) = replacement {
                    *part = replacement;
                }
            }
        }
    }
    if options.numeric {
        for part in &mut value.parts {
            let text = part.render().trim();
            if is_numeric(text) {
                *part = ValuePart::Literal(text.to_string());
            }
        }
    }
    if let Some(max_authors) = options.max_authors
        && field_name == "author"
    {
        limit_authors(value, max_authors);
    }
}

fn lowercase_set(values: Option<&[String]>) -> HashSet<String> {
    values
        .unwrap_or_default()
        .iter()
        .map(|value| value.to_ascii_lowercase())
        .collect()
}

fn sort_fields(fields: &mut [Field], order: &[String]) {
    let positions: HashMap<String, usize> = order
        .iter()
        .enumerate()
        .map(|(position, name)| (name.to_ascii_lowercase(), position))
        .collect();
    fields.sort_by_key(|field| {
        positions
            .get(&field.name.to_ascii_lowercase())
            .copied()
            .unwrap_or(usize::MAX)
    });
}

fn sort_entries(document: &mut Document, requested: &[String]) {
    let keys = if requested.is_empty() {
        vec!["key".to_string()]
    } else {
        requested.to_vec()
    };
    let sort_specials = keys
        .iter()
        .any(|key| key.trim_start_matches('-').eq_ignore_ascii_case("special"));
    let mut units: Vec<(Vec<Item>, Option<Item>)> = Vec::new();
    let mut pending = Vec::new();

    for item in std::mem::take(&mut document.items) {
        let is_comment = matches!(&item, Item::Comment(_))
            || matches!(&item, Item::Special(special) if special.command.eq_ignore_ascii_case("comment"));
        match item {
            Item::Comment(comment) => pending.push(Item::Comment(comment)),
            Item::Special(special) if is_comment => pending.push(Item::Special(special)),
            Item::Special(special) if sort_specials => {
                let item = Item::Special(special);
                pending.push(item.clone());
                units.push((std::mem::take(&mut pending), Some(item)));
            }
            Item::Special(special) => pending.push(Item::Special(special)),
            Item::Entry(entry) => {
                let item = Item::Entry(entry);
                pending.push(item.clone());
                units.push((std::mem::take(&mut pending), Some(item)));
            }
            Item::Raw(raw) if raw.text.trim().is_empty() => pending.push(Item::Raw(raw)),
            Item::Raw(raw) => {
                if !pending.is_empty() {
                    units.push((std::mem::take(&mut pending), None));
                }
                units.push((vec![Item::Raw(raw)], None));
            }
        }
    }
    if !pending.is_empty() {
        units.push((pending, None));
    }

    let mut sortable: Vec<(Item, Vec<Item>)> = units
        .iter()
        .filter_map(|(items, sort_item)| sort_item.clone().map(|item| (item, items.clone())))
        .collect();
    sortable.sort_by(|(left, _), (right, _)| compare_items(left, right, &keys));
    let mut next = sortable.into_iter();
    for (items, sort_item) in &mut units {
        if sort_item.is_some()
            && let Some((sorted_item, sorted_items)) = next.next()
        {
            *items = sorted_items;
            *sort_item = Some(sorted_item);
        }
    }
    document.items = units.into_iter().flat_map(|(items, _)| items).collect();
}

fn compare_items(left: &Item, right: &Item, keys: &[String]) -> Ordering {
    if let (Item::Entry(left), Item::Entry(right)) = (left, right) {
        return compare_entries(left, right, keys);
    }
    for requested in keys {
        let descending = requested.starts_with('-');
        let key = requested.trim_start_matches('-').to_ascii_lowercase();
        let left_value = item_sort_value(left, &key);
        let right_value = item_sort_value(right, &key);
        let ordering = compare_requested_values(&left_value, &right_value, descending);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn item_sort_value(item: &Item, key: &str) -> String {
    match item {
        Item::Entry(entry) => entry_sort_value(entry, key),
        Item::Special(special) if key == "special" => {
            if is_special_command(&special.command) {
                "0".to_string()
            } else {
                "1".to_string()
            }
        }
        Item::Special(special) if key == "type" => special.command.clone(),
        Item::Special(_) | Item::Comment(_) | Item::Raw(_) => String::new(),
    }
}

fn compare_entries(left: &Entry, right: &Entry, keys: &[String]) -> Ordering {
    for requested in keys {
        let descending = requested.starts_with('-');
        let key = requested.trim_start_matches('-').to_ascii_lowercase();
        let left_value = entry_sort_value(left, &key);
        let right_value = entry_sort_value(right, &key);
        let ordering = compare_requested_values(&left_value, &right_value, descending);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn compare_sort_values(left: &str, right: &str) -> Ordering {
    match (left.is_empty(), right.is_empty()) {
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        _ => natural_sort_value(left, right),
    }
}

fn compare_requested_values(left: &str, right: &str, descending: bool) -> Ordering {
    let empty_ordering = compare_sort_values(left, right);
    if left.is_empty() != right.is_empty() {
        empty_ordering
    } else if descending {
        empty_ordering.reverse()
    } else {
        empty_ordering
    }
}

fn entry_sort_value(entry: &Entry, key: &str) -> String {
    match key {
        "key" => entry.key.clone().unwrap_or_default(),
        "type" => entry.command.clone(),
        "special" => {
            if is_special_command(&entry.command) {
                "0".to_string()
            } else {
                "1".to_string()
            }
        }
        "month" => {
            field_value(entry, key).map_or_else(String::new, |value| month_sort_value(&value))
        }
        field => field_value(entry, field).unwrap_or_default(),
    }
}

fn is_special_command(command: &str) -> bool {
    matches!(
        command.to_ascii_lowercase().as_str(),
        "string" | "preamble" | "set" | "xdata"
    )
}

fn natural_sort_value(left: &str, right: &str) -> Ordering {
    let left_number = left.trim().parse::<u128>().ok();
    let right_number = right.trim().parse::<u128>().ok();
    match (left_number, right_number) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase()),
    }
}

fn month_sort_value(value: &str) -> String {
    if let Some(month) = abbreviated_month(value) {
        let rank = [
            "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
        ]
        .iter()
        .position(|candidate| *candidate == month)
        .unwrap_or(usize::MAX);
        return format!("{rank:02}");
    }
    value.trim().to_ascii_lowercase()
}

fn merge_duplicate_entries(document: &mut Document, options: &FormatOptions) {
    let duplicate_kinds = if options.duplicate_kinds.is_empty() {
        vec![
            DuplicateKind::Doi,
            DuplicateKind::Citation,
            DuplicateKind::Abstract,
        ]
    } else {
        options.duplicate_kinds.clone()
    };
    let merge_key = options.duplicate_kinds.contains(&DuplicateKind::Key);
    let mut representatives: HashMap<DuplicateKind, Vec<usize>> = HashMap::new();
    let mut removed = HashSet::new();
    for index in 0..document.items.len() {
        let Item::Entry(current) = &document.items[index] else {
            continue;
        };
        let current = current.clone();
        let mut kinds = vec![DuplicateKind::Key];
        kinds.extend(
            duplicate_kinds
                .iter()
                .copied()
                .filter(|kind| *kind != DuplicateKind::Key),
        );
        for kind in kinds {
            let previous_index = representatives.get(&kind).and_then(|indices| {
                indices.iter().copied().find(|previous_index| {
                    let Item::Entry(previous) = &document.items[*previous_index] else {
                        return false;
                    };
                    same_kind(previous, &current, kind)
                })
            });
            let Some(previous_index) = previous_index else {
                representatives.entry(kind).or_default().push(index);
                continue;
            };

            let should_merge = options.merge.is_some() && (kind != DuplicateKind::Key || merge_key);
            if should_merge {
                if let Some(strategy) = options.merge
                    && let Item::Entry(previous) = &mut document.items[previous_index]
                {
                    merge_entry(previous, &current, strategy);
                }
                removed.insert(index);
            }
        }
    }
    if !removed.is_empty() {
        let mut index = 0usize;
        document.items.retain(|_| {
            let keep = !removed.contains(&index);
            index += 1;
            keep
        });
    }
}

fn merge_entry(previous: &mut Entry, current: &Entry, strategy: MergeStrategy) {
    match strategy {
        MergeStrategy::First => {}
        MergeStrategy::Last => {
            previous.key.clone_from(&current.key);
            previous.key_range = current.key_range;
            previous.fields.clone_from(&current.fields);
        }
        MergeStrategy::Combine | MergeStrategy::Overwrite => {
            for field in &current.fields {
                if let Some(existing) = previous
                    .fields
                    .iter_mut()
                    .find(|existing| existing.name.eq_ignore_ascii_case(&field.name))
                {
                    if strategy == MergeStrategy::Overwrite {
                        *existing = field.clone();
                    }
                } else {
                    previous.fields.push(field.clone());
                }
            }
        }
    }
}

fn same_kind(left: &Entry, right: &Entry, kind: DuplicateKind) -> bool {
    match kind {
        DuplicateKind::Key => match (&left.key, &right.key) {
            (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
            _ => false,
        },
        DuplicateKind::Doi => normalized_fingerprint(field_value(left, "doi"))
            .zip(normalized_fingerprint(field_value(right, "doi")))
            .is_some_and(|(left, right)| left == right),
        DuplicateKind::Citation => {
            let left_title = field_value(left, "title");
            let right_title = field_value(right, "title");
            let left_author =
                field_value(left, "author").and_then(|value| citation_author_fingerprint(&value));
            let right_author =
                field_value(right, "author").and_then(|value| citation_author_fingerprint(&value));
            let left_number = field_value(left, "number").unwrap_or_default();
            let right_number = field_value(right, "number").unwrap_or_default();
            normalized_fingerprint(left_title)
                .zip(normalized_fingerprint(right_title))
                .zip(left_author.zip(right_author))
                .is_some_and(|((left_title, right_title), (left_author, right_author))| {
                    left_title == right_title
                        && left_author == right_author
                        && normalized_number_fingerprint(&left_number)
                            == normalized_number_fingerprint(&right_number)
                })
        }
        DuplicateKind::Abstract => normalized_fingerprint(field_value(left, "abstract"))
            .zip(normalized_fingerprint(field_value(right, "abstract")))
            .is_some_and(|(left, right)| left.chars().take(100).eq(right.chars().take(100))),
    }
}

fn normalized_fingerprint(value: Option<String>) -> Option<String> {
    let value = value?;
    let normalized: String = value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect();
    (!normalized.is_empty()).then_some(normalized)
}

fn normalized_number_fingerprint(value: &str) -> Option<String> {
    normalized_fingerprint(Some(if value.trim().is_empty() {
        "0".to_string()
    } else {
        value.to_string()
    }))
}

fn citation_author_fingerprint(value: &str) -> Option<String> {
    let first_author = value.split(" and ").next()?.trim();
    let family_name = first_author.split_once(',').map_or_else(
        || {
            first_author
                .split_whitespace()
                .last()
                .unwrap_or(first_author)
        },
        |(last, _)| last,
    );
    normalized_fingerprint(Some(family_name.to_string()))
}

fn field_value(entry: &Entry, field_name: &str) -> Option<String> {
    entry
        .fields
        .iter()
        .find(|field| field.name.eq_ignore_ascii_case(field_name))
        .and_then(|field| field.value.as_ref())
        .map(Value::render)
}

fn value_is_empty(value: &Value) -> bool {
    value.parts.is_empty()
        || value
            .parts
            .iter()
            .all(|part| part.render().trim().is_empty())
}

fn render_document(document: &Document, options: &FormatOptions) -> String {
    let mut rendered = Vec::new();
    for item in &document.items {
        let item_text = match item {
            Item::Entry(entry) => Some((format_entry(entry, options), false)),
            Item::Special(special)
                if options.strip_comments && special.command.eq_ignore_ascii_case("comment") =>
            {
                None
            }
            Item::Special(special) => Some((
                format_special(special, options),
                special.command.eq_ignore_ascii_case("comment"),
            )),
            Item::Comment(comment) if !options.strip_comments => {
                format_comment(comment, options).map(|text| (text, true))
            }
            Item::Comment(_) => None,
            Item::Raw(raw) => format_raw(raw).map(|text| (text, false)),
        };
        if let Some(item_text) = item_text.filter(|(text, _)| !text.trim().is_empty()) {
            rendered.push(item_text);
        }
    }
    let mut output = String::new();
    for (index, (item, _)) in rendered.iter().enumerate() {
        if index > 0 {
            let previous_is_comment = rendered[index - 1].1;
            let separator = if options.blank_lines && !previous_is_comment {
                "\n\n"
            } else {
                "\n"
            };
            output.push_str(separator);
        }
        output.push_str(item);
    }
    output = output.trim_end().to_string();
    output.push('\n');
    output
}

fn format_entry(entry: &Entry, options: &FormatOptions) -> String {
    let indent = options.indent.string();
    let open = entry.delimiter.open();
    let close = entry.delimiter.close();
    let mut output = format!("@{}{}", entry.command, open);
    if let Some(key) = &entry.key {
        output.push_str(key.trim());
        output.push(',');
    }
    if entry.fields.is_empty() {
        output.push('\n');
        output.push(close);
        return output;
    }
    output.push('\n');
    for (index, field) in entry.fields.iter().enumerate() {
        let comma = if index + 1 < entry.fields.len() || options.trailing_commas {
            ','
        } else {
            '\0'
        };
        let prefix = format_field_prefix(&indent, &field.name, options.align);
        let Some(value) = &field.value else {
            output.push_str(&prefix);
            if comma != '\0' {
                output.push(comma);
            }
            output.push('\n');
            continue;
        };
        let value_text = format_value(value);
        if should_wrap(value, &value_text, &prefix, options.wrap) {
            output.push_str(&format_wrapped_value(
                &prefix,
                value,
                &value_text,
                &indent,
                comma,
                options.wrap.unwrap_or(DEFAULT_WRAP),
            ));
        } else {
            output.push_str(&prefix);
            output.push_str("= ");
            output.push_str(&value_text);
            if comma != '\0' {
                output.push(comma);
            }
            output.push('\n');
        }
    }
    output.push(close);
    output
}

fn format_field_prefix(indent: &str, field_name: &str, align: Option<usize>) -> String {
    let padding = align.map_or(1, |column| column.saturating_sub(field_name.len()).max(1));
    format!("{indent}{field_name}{}", " ".repeat(padding))
}

fn format_value(value: &Value) -> String {
    value
        .parts
        .iter()
        .map(|part| match part {
            ValuePart::Braced(text) => format!("{{{text}}}"),
            ValuePart::Quoted(text) => format!("\"{text}\""),
            ValuePart::Literal(text) => text.clone(),
        })
        .collect::<Vec<_>>()
        .join(" # ")
}

fn should_wrap(value: &Value, text: &str, prefix: &str, width: Option<usize>) -> bool {
    width.is_some_and(|width| {
        value.parts.len() == 1
            && value.parts[0].is_braced()
            && (text.contains('\n') || prefix.len() + text.len() + 3 > width)
    })
}

fn format_wrapped_value(
    prefix: &str,
    value: &Value,
    text: &str,
    indent: &str,
    comma: char,
    width: usize,
) -> String {
    let ValuePart::Braced(inner) = &value.parts[0] else {
        let suffix = if comma == '\0' { "" } else { "," };
        return format!("{prefix}= {text}{suffix}\n");
    };
    let inner_width = width.saturating_sub(indent.len() * 2).max(1);
    let mut lines = Vec::new();
    for paragraph in inner.split("\n\n") {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if !current.is_empty() && current.len() + word.len() + 1 > inner_width {
                lines.push(current);
                current = String::new();
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        if !current.is_empty() {
            lines.push(current);
        }
        lines.push(String::new());
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    let mut output = format!("{prefix}= {{\n");
    for line in lines {
        if line.is_empty() {
            output.push('\n');
        } else {
            output.push_str(indent);
            output.push_str(indent);
            output.push_str(&line);
            output.push('\n');
        }
    }
    output.push_str(indent);
    output.push('}');
    if comma != '\0' {
        output.push(comma);
    }
    output.push('\n');
    output
}

fn format_special(special: &Special, options: &FormatOptions) -> String {
    let command = if options.lowercase {
        special.command.to_ascii_lowercase()
    } else {
        special.command.clone()
    };
    let body = if command.eq_ignore_ascii_case("comment") {
        special.body.trim().to_string()
    } else {
        normalize_special_body(&special.body)
    };
    let text = format!(
        "@{command}{}{}{}",
        special.delimiter.open(),
        body,
        special.delimiter.close()
    );
    text
}

fn format_comment(comment: &Comment, options: &FormatOptions) -> Option<String> {
    let lines: Vec<String> = comment
        .text
        .lines()
        .map(|line| {
            if options.tidy_comments {
                line.trim().to_string()
            } else {
                line.trim_end().to_string()
            }
        })
        .filter(|line| !line.is_empty())
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn format_raw(raw: &Raw) -> Option<String> {
    let lines: Vec<&str> = raw
        .text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn normalize_special_body(body: &str) -> String {
    let mut output = normalize_value_whitespace(body);
    output = join_top_level_parts(&output, '#', " # ");
    if let Some(position) = top_level_delimiter(&output, '=') {
        output = format!(
            "{} = {}",
            output[..position].trim(),
            output[position + '='.len_utf8()..].trim()
        );
    }
    output
}

fn join_top_level_parts(value: &str, delimiter: char, separator: &str) -> String {
    let mut parts = Vec::new();
    let mut start = 0usize;
    for position in top_level_delimiters(value, delimiter) {
        parts.push(value[start..position].trim());
        start = position + delimiter.len_utf8();
    }
    parts.push(value[start..].trim());
    parts.join(separator)
}

fn top_level_delimiter(value: &str, delimiter: char) -> Option<usize> {
    top_level_delimiters(value, delimiter).into_iter().next()
}

fn top_level_delimiters(value: &str, delimiter: char) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut braces = 0usize;
    let mut parentheses = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (position, character) in value.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if quoted {
            if character == '"' {
                quoted = false;
            }
            continue;
        }
        match character {
            '"' => quoted = true,
            '{' => braces += 1,
            '}' => braces = braces.saturating_sub(1),
            '(' => parentheses += 1,
            ')' => parentheses = parentheses.saturating_sub(1),
            _ if character == delimiter && braces == 0 && parentheses == 0 => {
                positions.push(position);
            }
            _ => {}
        }
    }
    positions
}

fn normalize_value_whitespace(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}

fn format_page_ranges(value: &str) -> String {
    let characters: Vec<char> = value.chars().collect();
    let mut output = String::new();
    let mut position = 0usize;
    while position < characters.len() {
        if characters[position] == '-' {
            let next = position + 1;
            let before_digit = output
                .chars()
                .rev()
                .find(|character| !character.is_whitespace())
                .is_some_and(|character| character.is_ascii_digit());
            let mut after = next;
            while after < characters.len() && characters[after].is_whitespace() {
                after += 1;
            }
            let after_digit = after < characters.len() && characters[after].is_ascii_digit();
            let is_single_dash = next >= characters.len() || characters[next] != '-';
            if is_single_dash && before_digit && after_digit {
                while output.chars().last().is_some_and(char::is_whitespace) {
                    output.pop();
                }
                output.push_str("--");
                position = after;
            } else {
                output.push('-');
                position = next;
            }
        } else {
            output.push(characters[position]);
            position += 1;
        }
    }
    output
}

fn encode_url(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if character == '_' {
            if output.ends_with('\\') {
                output.pop();
            }
            output.push_str("\\%5F");
        } else {
            output.push(character);
        }
    }
    output
}

fn abbreviated_month(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    let month = match value.as_str() {
        "1" | "jan" | "january" => "jan",
        "2" | "feb" | "february" => "feb",
        "3" | "mar" | "march" => "mar",
        "4" | "apr" | "april" => "apr",
        "5" | "may" => "may",
        "6" | "jun" | "june" => "jun",
        "7" | "jul" | "july" => "jul",
        "8" | "aug" | "august" => "aug",
        "9" | "sep" | "september" => "sep",
        "10" | "oct" | "october" => "oct",
        "11" | "nov" | "november" => "nov",
        "12" | "dec" | "december" => "dec",
        _ => return None,
    };
    Some(month.to_string())
}

fn is_numeric(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character.is_ascii_digit() && character != '0')
        && characters.all(|character| character.is_ascii_digit())
}

fn strip_one_outer_brace_group(value: &str) -> String {
    let value = value.trim();
    let inner = value.get(1..value.len().saturating_sub(1));
    if value.starts_with('{')
        && value.ends_with('}')
        && balanced_outer_braces(value)
        && inner.is_some_and(|inner| !inner.contains(['{', '}']))
    {
        inner.unwrap_or_default().to_string()
    } else {
        value.to_string()
    }
}

fn balanced_outer_braces(value: &str) -> bool {
    if !value.starts_with('{') || !value.ends_with('}') {
        return false;
    }
    let mut depth = 0usize;
    for (position, character) in value.char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
                if depth == 0 && position + character.len_utf8() != value.len() {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

fn is_already_double_enclosed(value: &str) -> bool {
    if !balanced_outer_braces(value) || value.len() < 4 {
        return false;
    }
    balanced_outer_braces(&value[1..value.len() - 1])
}

fn remove_text_braces(value: &str) -> String {
    let mut output = String::new();
    let mut position = 0usize;
    while position < value.len() {
        let character = value[position..].chars().next().unwrap_or_default();
        if character == '{'
            && !is_escaped_text(value, position)
            && let Some(end) = matching_brace(value, position)
        {
            let content = &value[position + 1..end];
            if is_command_argument(value, position) || has_direct_command(content) {
                output.push_str(&value[position..=end]);
            } else {
                output.push_str(&remove_text_braces(content));
            }
            position = end + 1;
            continue;
        }
        output.push(character);
        position += character.len_utf8();
    }
    output
}

fn matching_brace(value: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (position, character) in value[open..].char_indices() {
        let position = open + position;
        if is_escaped_text(value, position) {
            continue;
        }
        match character {
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(position);
                }
            }
            _ => {}
        }
    }
    None
}

fn is_command_argument(value: &str, open: usize) -> bool {
    let mut saw_letter = false;
    let mut position = open;
    while position > 0 {
        let character = value[..position].chars().next_back().unwrap_or_default();
        if !character.is_alphabetic() {
            return saw_letter && character == '\\';
        }
        saw_letter = true;
        position -= character.len_utf8();
    }
    false
}

fn has_direct_command(value: &str) -> bool {
    let mut depth = 0usize;
    for (position, character) in value.char_indices() {
        if is_escaped_text(value, position) {
            continue;
        }
        match character {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            '\\' if depth == 0 => return true,
            _ => {}
        }
    }
    false
}

fn is_escaped_text(value: &str, position: usize) -> bool {
    let bytes = value.as_bytes();
    let mut backslashes = 0usize;
    let mut previous = position;
    while previous > 0 && bytes[previous - 1] == b'\\' {
        backslashes += 1;
        previous -= 1;
    }
    backslashes % 2 == 1
}

fn title_case(value: &str) -> String {
    let characters: Vec<char> = value.chars().collect();
    let mut output = String::new();
    let mut word_start = true;
    let mut position = 0usize;
    while position < characters.len() {
        let character = characters[position];
        if character == '\\' {
            output.push(character);
            position += 1;
            while position < characters.len() && characters[position].is_alphabetic() {
                output.push(characters[position]);
                position += 1;
            }
            word_start = true;
            continue;
        }
        if character.is_alphabetic() {
            let start = position;
            while position < characters.len() && characters[position].is_alphabetic() {
                position += 1;
            }
            let word: String = characters[start..position].iter().collect();
            if is_roman_numeral(&word) {
                output.push_str(&word);
            } else if word_start {
                if let Some(first) = word.chars().next() {
                    output.extend(first.to_uppercase());
                    for character in word.chars().skip(1) {
                        output.extend(character.to_lowercase());
                    }
                }
            } else {
                output.extend(word.chars().flat_map(char::to_lowercase));
            }
            word_start = false;
        } else {
            output.push(character);
            word_start = !character.is_ascii_alphanumeric();
            position += 1;
        }
    }
    output
}

fn visible_latex_text(value: &str) -> String {
    let characters: Vec<char> = value.chars().collect();
    let mut output = String::new();
    let mut position = 0usize;
    while position < characters.len() {
        if characters[position] == '\\' {
            position += 1;
            while position < characters.len() && characters[position].is_alphabetic() {
                position += 1;
            }
        } else if !matches!(characters[position], '{' | '}') {
            output.push(characters[position]);
            position += 1;
        } else {
            position += 1;
        }
    }
    output
}

fn is_roman_numeral(value: &str) -> bool {
    if value.is_empty()
        || !value
            .chars()
            .all(|character| matches!(character, 'I' | 'V' | 'X' | 'L' | 'C' | 'D' | 'M'))
    {
        return false;
    }
    let values = |character| match character {
        'I' => 1,
        'V' => 5,
        'X' => 10,
        'L' => 50,
        'C' => 100,
        'D' => 500,
        'M' => 1000,
        _ => 0,
    };
    let mut total = 0;
    let mut previous = 0;
    for character in value.chars().rev() {
        let current = values(character);
        if current < previous {
            total -= current;
        } else {
            total += current;
            previous = current;
        }
    }
    (1..=4999).contains(&total) && canonical_roman(total) == value
}

fn canonical_roman(mut value: i32) -> String {
    let symbols = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut output = String::new();
    for (number, symbol) in symbols {
        while value >= number {
            output.push_str(symbol);
            value -= number;
        }
    }
    output
}

fn limit_authors(value: &mut Value, max_authors: usize) {
    if value.parts.len() != 1 || !value.parts[0].is_braced() && !value.parts[0].is_quoted() {
        return;
    }
    let rendered = value.render();
    let authors: Vec<&str> = rendered.split(" and ").collect();
    if authors.len() > max_authors {
        let kept = authors[..max_authors].join(" and ");
        let replacement = if kept.is_empty() {
            "others".to_string()
        } else {
            format!("{kept} and others")
        };
        let part = match value.parts.first() {
            Some(ValuePart::Braced(_)) => ValuePart::Braced(replacement),
            Some(ValuePart::Quoted(_)) => ValuePart::Quoted(replacement),
            Some(ValuePart::Literal(_)) | None => ValuePart::Literal(replacement),
        };
        value.parts = vec![part];
    }
}

fn escape_text(value: &str, protect_quotes: bool) -> String {
    let characters: Vec<char> = value.chars().collect();
    let mut output = String::new();
    let mut escaped = false;
    let mut in_math = false;
    let mut math_escaped = false;
    for (position, character) in characters.iter().copied().enumerate() {
        if in_math {
            output.push(character);
            if math_escaped {
                math_escaped = false;
            } else if character == '\\' {
                math_escaped = true;
            } else if character == '$' {
                in_math = false;
            }
            continue;
        }
        if escaped {
            output.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' {
            output.push(character);
            escaped = true;
            continue;
        }
        if character == '$' && has_unescaped_math_end(&characters, position + 1) {
            output.push(character);
            in_math = true;
            continue;
        }
        if let Some(replacement) = escape_character(character) {
            if protect_quotes && replacement.contains('"') {
                output.push('{');
                output.push_str(replacement);
                output.push('}');
            } else {
                output.push_str(replacement);
            }
        } else {
            output.push(character);
        }
    }
    output
}

fn has_unescaped_math_end(characters: &[char], start: usize) -> bool {
    let mut escaped = false;
    for character in characters.iter().copied().skip(start) {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '$' {
            return true;
        }
    }
    false
}

const CORE_REVERSIBLE_ESCAPE_MAPPINGS: &[(char, &str)] = &[
    ('#', "\\#"),
    ('$', "\\$"),
    ('%', "\\%"),
    ('&', "\\&"),
    ('@', "\\@"),
    ('_', "\\_"),
    ('À', "\\`{A}"),
    ('Á', "\\'{A}"),
    ('Â', "\\^{A}"),
    ('Ã', "\\~{A}"),
    ('Ä', "\\\"{A}"),
    ('Å', "\\AA{}"),
    ('Æ', "\\AE{}"),
    ('Ç', "\\c{C}"),
    ('È', "\\`{E}"),
    ('É', "\\'{E}"),
    ('Ê', "\\^{E}"),
    ('Ë', "\\\"{E}"),
    ('Ì', "\\`{I}"),
    ('Í', "\\'{I}"),
    ('Î', "\\^{I}"),
    ('Ï', "\\\"{I}"),
    ('Ñ', "\\~{N}"),
    ('Ò', "\\`{O}"),
    ('Ó', "\\'{O}"),
    ('Ô', "\\^{O}"),
    ('Õ', "\\~{O}"),
    ('Ö', "\\\"{O}"),
    ('Ø', "\\O{}"),
    ('Ù', "\\`{U}"),
    ('Ú', "\\'{U}"),
    ('Û', "\\^{U}"),
    ('Ü', "\\\"{U}"),
    ('Ý', "\\'{Y}"),
    ('ß', "\\ss{}"),
    ('à', "\\`{a}"),
    ('á', "\\'{a}"),
    ('â', "\\^{a}"),
    ('ã', "\\~{a}"),
    ('ä', "\\\"{a}"),
    ('å', "\\aa{}"),
    ('æ', "\\ae{}"),
    ('ç', "\\c{c}"),
    ('è', "\\`{e}"),
    ('é', "\\'{e}"),
    ('ê', "\\^{e}"),
    ('ë', "\\\"{e}"),
    ('ì', "\\`{\\i}"),
    ('í', "\\'{\\i}"),
    ('î', "\\^{\\i}"),
    ('ï', "\\\"{\\i}"),
    ('ñ', "\\~{n}"),
    ('ò', "\\`{o}"),
    ('ó', "\\'{o}"),
    ('ô', "\\^{o}"),
    ('õ', "\\~{o}"),
    ('ö', "\\\"{o}"),
    ('ø', "\\o{}"),
    ('ù', "\\`{u}"),
    ('ú', "\\'{u}"),
    ('û', "\\^{u}"),
    ('ü', "\\\"{u}"),
    ('ý', "\\'{y}"),
    ('ÿ', "\\\"{y}"),
    ('Ā', "\\={A}"),
    ('ā', "\\={a}"),
    ('Ă', "\\u{A}"),
    ('ă', "\\u{a}"),
    ('Ą', "\\k{A}"),
    ('ą', "\\k{a}"),
    ('Ć', "\\'{C}"),
    ('ć', "\\'{c}"),
    ('Ĉ', "\\^{C}"),
    ('ĉ', "\\^{c}"),
    ('Ċ', "\\.{C}"),
    ('ċ', "\\.{c}"),
    ('Č', "\\v{C}"),
    ('č', "\\v{c}"),
    ('Ď', "\\v{D}"),
    ('ď', "\\v{d}"),
    ('Ē', "\\={E}"),
    ('ē', "\\={e}"),
    ('Ĕ', "\\u{E}"),
    ('ĕ', "\\u{e}"),
    ('Ė', "\\.{E}"),
    ('ė', "\\.{e}"),
    ('Ę', "\\k{E}"),
    ('ę', "\\k{e}"),
    ('Ě', "\\v{E}"),
    ('ě', "\\v{e}"),
    ('Ĝ', "\\^{G}"),
    ('ĝ', "\\^{g}"),
    ('Ğ', "\\u{G}"),
    ('ğ', "\\u{g}"),
    ('Ġ', "\\.{G}"),
    ('ġ', "\\.{g}"),
    ('Ģ', "\\c{G}"),
    ('ģ', "\\c{g}"),
    ('Ĥ', "\\^{H}"),
    ('ĥ', "\\^{h}"),
    ('Ĩ', "\\~{I}"),
    ('ĩ', "\\~{\\i}"),
    ('Ī', "\\={I}"),
    ('ī', "\\={\\i}"),
    ('Ĭ', "\\u{I}"),
    ('ĭ', "\\u{\\i}"),
    ('Į', "\\k{I}"),
    ('į', "\\k{\\i}"),
    ('İ', "\\.{I}"),
    ('ı', "\\i{}"),
    ('Ĵ', "\\^{J}"),
    ('ĵ', "\\^{\\j}"),
    ('Ķ', "\\c{K}"),
    ('ķ', "\\c{k}"),
    ('Ĺ', "\\'{L}"),
    ('ĺ', "\\'{l}"),
    ('Ļ', "\\c{L}"),
    ('ļ', "\\c{l}"),
    ('Ľ', "\\v{L}"),
    ('ľ', "\\v{l}"),
    ('Ł', "\\L{}"),
    ('ł', "\\l{}"),
    ('Ń', "\\'{N}"),
    ('ń', "\\'{n}"),
    ('Ņ', "\\c{N}"),
    ('ņ', "\\c{n}"),
    ('Ň', "\\v{N}"),
    ('ň', "\\v{n}"),
    ('Ō', "\\={O}"),
    ('ō', "\\={o}"),
    ('Ŏ', "\\u{O}"),
    ('ŏ', "\\u{o}"),
    ('Ő', "\\H{O}"),
    ('ő', "\\H{o}"),
    ('Œ', "\\OE{}"),
    ('œ', "\\oe{}"),
    ('Ŕ', "\\'{R}"),
    ('ŕ', "\\'{r}"),
    ('Ŗ', "\\c{R}"),
    ('ŗ', "\\c{r}"),
    ('Ř', "\\v{R}"),
    ('ř', "\\v{r}"),
    ('Ś', "\\'{S}"),
    ('ś', "\\'{s}"),
    ('Ŝ', "\\^{S}"),
    ('ŝ', "\\^{s}"),
    ('Ş', "\\c{S}"),
    ('ş', "\\c{s}"),
    ('Š', "\\v{S}"),
    ('š', "\\v{s}"),
    ('Ţ', "\\c{T}"),
    ('ţ', "\\c{t}"),
    ('Ť', "\\v{T}"),
    ('ť', "\\v{t}"),
    ('Ũ', "\\~{U}"),
    ('ũ', "\\~{u}"),
    ('Ū', "\\={U}"),
    ('ū', "\\={u}"),
    ('Ŭ', "\\u{U}"),
    ('ŭ', "\\u{u}"),
    ('Ů', "\\r{U}"),
    ('ů', "\\r{u}"),
    ('Ű', "\\H{U}"),
    ('ű', "\\H{u}"),
    ('Ų', "\\k{U}"),
    ('ų', "\\k{u}"),
    ('Ŵ', "\\^{W}"),
    ('ŵ', "\\^{w}"),
    ('Ŷ', "\\^{Y}"),
    ('ŷ', "\\^{y}"),
    ('Ź', "\\'{Z}"),
    ('ź', "\\'{z}"),
    ('Ż', "\\.{Z}"),
    ('ż', "\\.{z}"),
    ('Ž', "\\v{Z}"),
    ('ž', "\\v{z}"),
    ('ǵ', "\\'{g}"),
    ('–', "--"),
    ('—', "---"),
    ('“', "``"),
    ('”', "''"),
    ('…', "\\ldots{}"),
    ('\u{212B}', "\\AA{}"),
];

const CORE_NON_REVERSIBLE_ESCAPE_MAPPINGS: &[(char, &str)] = &[
    (' ', "~"),
    ('Ĳ', "IJ"),
    ('ĳ', "ij"),
    ('‐', "-"),
    ('‑', "-"),
    ('‘', "`"),
    ('’', "'"),
    (' ', "~"),
];

fn escape_character(character: char) -> Option<&'static str> {
    CORE_REVERSIBLE_ESCAPE_MAPPINGS
        .iter()
        .chain(CORE_NON_REVERSIBLE_ESCAPE_MAPPINGS.iter())
        .find_map(|(source, replacement)| (*source == character).then_some(*replacement))
}

#[allow(clippy::too_many_lines)]
fn unescape_text(value: &str, quoted: bool) -> String {
    let value = if quoted {
        replace_core_escapes(value, true)
    } else {
        value.to_string()
    };
    let mut generic = String::new();
    let characters: Vec<char> = value.chars().collect();
    let mut position = 0usize;
    while position < characters.len() {
        if characters[position] == '\\' && position + 1 < characters.len() {
            let accent = characters[position + 1];
            if matches!(
                accent,
                '`' | '\'' | '^' | '"' | '~' | '=' | 'v' | 'c' | 'u' | 'r' | 'k' | 'H'
            ) {
                let raw_start = position;
                position += 2;
                let base = if characters.get(position) == Some(&'{') {
                    position += 1;
                    let base = characters.get(position).copied();
                    while position < characters.len() && characters[position] != '}' {
                        position += 1;
                    }
                    if position < characters.len() {
                        position += 1;
                    }
                    base
                } else {
                    let base = characters.get(position).copied();
                    position += usize::from(base.is_some());
                    base
                };
                if let Some(base) = base.and_then(|base| unescaped_accent(accent, base)) {
                    generic.push(base);
                } else {
                    generic.extend(characters[raw_start..position].iter());
                }
                continue;
            }
        }
        generic.push(characters[position]);
        position += 1;
    }
    replace_core_escapes(&generic, false)
}

fn replace_core_escapes(value: &str, quoted: bool) -> String {
    let mut output = String::new();
    let mut position = 0usize;
    while position < value.len() {
        let remaining = &value[position..];
        let mut matching = None;
        for (character, escaped) in CORE_REVERSIBLE_ESCAPE_MAPPINGS {
            let match_length = if quoted && escaped.contains('"') {
                let wrapped = remaining
                    .strip_prefix('{')
                    .and_then(|remaining| remaining.strip_prefix(*escaped))
                    .is_some_and(|remaining| remaining.starts_with('}'));
                wrapped.then_some(escaped.len() + 2)
            } else {
                remaining.starts_with(*escaped).then_some(escaped.len())
            };
            if let Some(match_length) = match_length
                && matching.is_none_or(|(_, current_length)| match_length > current_length)
            {
                matching = Some((*character, match_length));
            }
        }
        if let Some((character, match_length)) = matching {
            output.push(character);
            position += match_length;
        } else if let Some(character) = remaining.chars().next() {
            output.push(character);
            position += character.len_utf8();
        } else {
            break;
        }
    }
    output
}

fn unescaped_accent(accent: char, base: char) -> Option<char> {
    let lower = base.to_ascii_lowercase();
    let mapped = match (accent, lower) {
        ('v', 'c') => 'č',
        ('v', 'd') => 'ď',
        ('v', 'e') => 'ě',
        ('v', 'n') => 'ň',
        ('v', 'r') => 'ř',
        ('v', 's') => 'š',
        ('v', 't') => 'ť',
        ('v', 'z') => 'ž',
        ('=', 'a') => 'ā',
        ('=', 'e') => 'ē',
        ('=', 'i') => 'ī',
        ('=', 'o') => 'ō',
        ('=', 'u') => 'ū',
        ('u', 'a') => 'ă',
        ('u', 'g') => 'ğ',
        ('u', 'i') => 'ĭ',
        ('u', 'o') => 'ŏ',
        ('u', 'u') => 'ŭ',
        ('r', 'a') => 'å',
        ('r', 'u') => 'ů',
        ('c', 'c') => 'ç',
        ('c', 's') => 'ş',
        ('k', 'a') => 'ą',
        ('k', 'e') => 'ę',
        ('H', 'o') => 'ő',
        ('H', 'u') => 'ű',
        ('`', 'a') => 'à',
        ('`', 'e') => 'è',
        ('`', 'i') => 'ì',
        ('`', 'o') => 'ò',
        ('`', 'u') => 'ù',
        ('\'', 'a') => 'á',
        ('\'', 'e') => 'é',
        ('\'', 'i') => 'í',
        ('\'', 'o') => 'ó',
        ('\'', 'u') => 'ú',
        ('^', 'a') => 'â',
        ('^', 'e') => 'ê',
        ('^', 'i') => 'î',
        ('^', 'o') => 'ô',
        ('^', 'u') => 'û',
        ('"', 'a') => 'ä',
        ('"', 'e') => 'ë',
        ('"', 'i') => 'ï',
        ('"', 'o') => 'ö',
        ('"', 'u') => 'ü',
        ('~', 'a') => 'ã',
        ('~', 'n') => 'ñ',
        ('~', 'o') => 'õ',
        _ => return None,
    };
    if base.is_uppercase() {
        mapped.to_uppercase().next()
    } else {
        Some(mapped)
    }
}

#[cfg(test)]
mod tests {
    use super::{DuplicateKind, FormatOptions, Indent, MergeStrategy, format_document};
    use biblint_syntax::parse;

    #[test]
    fn formats_the_basic_canonical_shape() {
        let source = "@ARTICLE {key, title={A title},author=\"Smith\",year={2024},}";
        let result = format_document(&parse(source), &FormatOptions::default());
        assert_eq!(
            result.output,
            "@article{key,\n  title = {A title},\n  author = \"Smith\",\n  year = {2024}\n}\n"
        );
    }

    #[test]
    fn supports_common_cleanup_options() {
        let source = "@article{b, title=\"TITLE\", month={3}, pages={4-9}, year={2024},}";
        let mut options = FormatOptions {
            curly: true,
            months: true,
            drop_all_caps: true,
            format_page_ranges: true,
            ..FormatOptions::default()
        };
        options.indent = Indent::Spaces(4);
        let result = format_document(&parse(source), &options);
        assert!(result.output.contains("title = {Title}"));
        assert!(result.output.contains("month = mar"));
        assert!(result.output.contains("pages = {4--9}"));
        assert!(result.output.starts_with("@article{b,\n    "));
    }

    #[test]
    fn keeps_unparsed_text_visible() {
        let result = format_document(&parse("not bibtex\n"), &FormatOptions::default());
        assert_eq!(result.output, "not bibtex\n");
    }

    #[test]
    fn formatting_is_idempotent() {
        let source = "@ARTICLE {key, title=\"Café\", pages={4-9}, year={2024},}\n";
        let options = FormatOptions::default();
        let once = format_document(&parse(source), &options);
        let twice = format_document(&parse(&once.output), &options);
        assert_eq!(once.output, twice.output);
    }

    #[test]
    fn formats_entries_with_an_empty_key() {
        let result = format_document(
            &parse("@article{\n  title = \"A year of trees\",\n  volume = {5}\n}"),
            &FormatOptions::default(),
        );
        assert_eq!(
            result.output,
            "@article{\n  title = \"A year of trees\",\n  volume = {5}\n}\n"
        );
    }

    #[test]
    fn trims_value_edges_without_collapsing_internal_whitespace() {
        let source = r#"@article{key,title={  A   B  },note="  C\t\tD  "}"#;
        let result = format_document(&parse(source), &FormatOptions::default());
        assert!(result.output.contains("title = {A   B}"));
        assert!(result.output.contains("note = \"C\\t\\tD\""));
    }

    #[test]
    fn applies_page_ranges_only_to_pages_and_preserves_existing_double_dashes() {
        let source = "@article{key,title={4-9},year={4 - 9},pages={4 -- 9; 10 - 12}}";
        let options = FormatOptions {
            format_page_ranges: true,
            ..FormatOptions::default()
        };
        let result = format_document(&parse(source), &options);
        assert!(result.output.contains("title = {4-9}"));
        assert!(result.output.contains("year = {4 - 9}"));
        assert!(result.output.contains("pages = {4 -- 9; 10--12}"));
    }

    #[test]
    fn encodes_url_underscores_without_touching_other_fields() {
        let source = r"@article{key,url={https://example.test/a_b},note={a_b}}";
        let options = FormatOptions {
            encode_urls: true,
            ..FormatOptions::default()
        };
        let result = format_document(&parse(source), &options);
        assert!(
            result
                .output
                .contains(r"url = {https://example.test/a\%5Fb}")
        );
        assert!(result.output.contains("note = {a_b}"));
    }

    #[test]
    fn keeps_protected_months_and_zero_or_padded_numeric_values() {
        let source = "@article{key,month={{march}},year={0},volume={001},number={12}}";
        let options = FormatOptions {
            months: true,
            numeric: true,
            ..FormatOptions::default()
        };
        let result = format_document(&parse(source), &options);
        assert!(result.output.contains("month = {{march}}"));
        assert!(result.output.contains("year = {0}"));
        assert!(result.output.contains("volume = {001}"));
        assert!(result.output.contains("number = 12"));
    }

    #[test]
    fn brace_transforms_respect_nested_groups() {
        let source = "@article{key,title={{A {B}}},journal={{{C}}},note={{D}}}";
        let options = FormatOptions {
            strip_enclosing_braces: true,
            enclosing_braces: Some(vec!["journal".to_string()]),
            ..FormatOptions::default()
        };
        let result = format_document(&parse(source), &options);
        assert!(result.output.contains("title = {{A {B}}}"));
        assert!(result.output.contains("journal = {{{C}}}"));
        assert!(result.output.contains("note = {D}"));
    }

    #[test]
    fn removes_empty_concatenations_and_keeps_first_duplicate_field() {
        let source = "@article{key,title={first},title={second},abstract={ } # {  },year=2024}";
        let options = FormatOptions {
            remove_empty_fields: true,
            remove_duplicate_fields: true,
            ..FormatOptions::default()
        };
        let result = format_document(&parse(source), &options);
        assert_eq!(result.output.matches("title =").count(), 1);
        assert!(result.output.contains("title = {first}"));
        assert!(!result.output.contains("abstract"));
    }

    #[test]
    fn normalizes_special_bodies_without_splitting_nested_delimiters() {
        let source = r#"@string{label = "A#B" # "C=D"}"#;
        let result = format_document(&parse(source), &FormatOptions::default());
        assert_eq!(result.output, "@string{label = \"A#B\" # \"C=D\"}\n");
    }

    #[test]
    fn keeps_comments_with_the_entry_they_precede_when_sorting() {
        let source = "% for b\n@article{b,title={B}}\n@article{a,title={A}}";
        let options = FormatOptions {
            sort: Some(vec!["key".to_string()]),
            ..FormatOptions::default()
        };
        let result = format_document(&parse(source), &options);
        let a_position = result.output.find("@article{a").unwrap();
        let comment_position = result.output.find("% for b").unwrap();
        let b_position = result.output.find("@article{b").unwrap();
        assert!(a_position < comment_position && comment_position < b_position);
    }

    #[test]
    fn sorts_months_chronologically() {
        let source = "@article{dec,month=dec}\n@article{jan,month=jan}";
        let options = FormatOptions {
            sort: Some(vec!["month".to_string()]),
            ..FormatOptions::default()
        };
        let result = format_document(&parse(source), &options);
        assert!(
            result.output.find("@article{jan").unwrap()
                < result.output.find("@article{dec").unwrap()
        );
    }

    #[test]
    fn preserves_key_duplicates_by_default_but_merges_explicit_key_duplicates() {
        let source = "@article{same,title={A}}\n@article{same,title={B}}";
        let default_merge = FormatOptions {
            merge: Some(MergeStrategy::First),
            ..FormatOptions::default()
        };
        let preserved = format_document(&parse(source), &default_merge);
        assert_eq!(preserved.output.matches("@article").count(), 2);

        let explicit_key_merge = FormatOptions {
            duplicate_kinds: vec![DuplicateKind::Key],
            merge: Some(MergeStrategy::First),
            ..FormatOptions::default()
        };
        let merged = format_document(&parse(source), &explicit_key_merge);
        assert_eq!(merged.output.matches("@article").count(), 1);
    }

    #[test]
    fn evaluates_duplicate_kinds_independently() {
        let source = "@article{same,author={A},title={A},doi={10/one}}\n@article{same,author={B},title={B},doi={10/two}}\n@article{third,author={C},title={C},doi={10/two}}";
        let options = FormatOptions {
            merge: Some(MergeStrategy::First),
            ..FormatOptions::default()
        };
        let result = format_document(&parse(source), &options);
        assert_eq!(result.output.matches("@article").count(), 2);
        assert!(result.output.contains("@article{same"));
        assert!(result.output.contains("title = {B}"));
        assert!(!result.output.contains("@article{third"));
    }

    #[test]
    fn protects_math_when_escaping_and_unescapes_common_latin_commands() {
        let source = r"@article{key,title={100% & Café $x_y$ and \AA{}},note={\^{A} \%}}";
        let escape_options = FormatOptions {
            escape: true,
            ..FormatOptions::default()
        };
        let escaped = format_document(&parse(source), &escape_options);
        assert!(escaped.output.contains(r"100\% \& Caf\'{e} $x_y$"));

        let unescape_options = FormatOptions {
            unescape: true,
            ..FormatOptions::default()
        };
        let unescaped = format_document(&parse(&escaped.output), &unescape_options);
        assert!(unescaped.output.contains("100% & Café $x_y$ and Å"));
        assert!(unescaped.output.contains("note = {Â %}"));
    }

    #[test]
    fn escapes_better_bibtex_core_characters() {
        let source = "@article{key,title={Ą Ć Ġ ĩ Ű ı ǵ \u{212b} \u{00a0} Ĳ ‐ ‘ ’ \u{202f}tail}}";
        let options = FormatOptions {
            escape: true,
            ..FormatOptions::default()
        };
        let escaped = format_document(&parse(source), &options);
        assert!(
            escaped
                .output
                .contains(r"\k{A} \'{C} \.{G} \~{\i} \H{U} \i{} \'{g} \AA{} ~ IJ - ` ' ~tail")
        );
        assert!(super::unsupported_escape_characters(source).is_empty());
        assert_eq!(super::unsupported_escape_characters("λ"), vec!['λ']);
    }

    #[test]
    fn unescapes_better_bibtex_core_characters_and_protected_quotes() {
        let source =
            r"@article{key,title={\k{A} \'{C} \.{G} \~{\i} \H{U} \i{} \'{g} \AA{} ~ IJ - ` ' ~}}";
        let options = FormatOptions {
            unescape: true,
            ..FormatOptions::default()
        };
        let unescaped = format_document(&parse(source), &options);
        assert!(
            unescaped
                .output
                .contains("title = {Ą Ć Ġ ĩ Ű ı ǵ Å ~ IJ - ` ' ~}")
        );

        let quoted = r#"@article{key,title="{\"{A}}"}"#;
        let unescaped_quoted = format_document(&parse(quoted), &options);
        assert!(unescaped_quoted.output.contains(r#"title = "Ä""#));
    }

    #[test]
    fn does_not_drop_concatenated_values_when_wrapping() {
        let source = "@article{key,title={first} # \"second value\"}";
        let options = FormatOptions {
            wrap: Some(20),
            ..FormatOptions::default()
        };
        let result = format_document(&parse(source), &options);
        assert!(result.output.contains("title = {first} # \"second value\""));
    }

    #[test]
    fn strips_at_comment_blocks_when_comments_are_removed() {
        let source = "% line\n@comment{hidden}\n@article{key,title={A}}";
        let options = FormatOptions {
            strip_comments: true,
            ..FormatOptions::default()
        };
        let result = format_document(&parse(source), &options);
        assert!(!result.output.contains("% line"));
        assert!(!result.output.contains("hidden"));
        assert!(result.output.contains("@article{key"));
    }

    #[test]
    fn accepts_reference_boolean_option_shorthands() {
        use serde::de::value::{BoolDeserializer, Error, UsizeDeserializer};

        let sort = super::deserialize_sort(BoolDeserializer::<Error>::new(true)).unwrap();
        assert_eq!(sort, Some(vec!["key".to_string()]));
        let fields = super::deserialize_sort_fields(BoolDeserializer::<Error>::new(true)).unwrap();
        assert_eq!(
            fields.as_ref().unwrap().first().map(String::as_str),
            Some("title")
        );
        let disabled =
            super::deserialize_field_list_option(BoolDeserializer::<Error>::new(false)).unwrap();
        assert_eq!(disabled, None);
        let wrap = super::deserialize_wrap(BoolDeserializer::<Error>::new(true)).unwrap();
        assert_eq!(wrap, Some(super::DEFAULT_WRAP));
        let custom_wrap = super::deserialize_wrap(UsizeDeserializer::<Error>::new(72)).unwrap();
        assert_eq!(custom_wrap, Some(72));
    }

    #[test]
    fn alignment_is_opt_in() {
        let options = FormatOptions {
            align: Some(14),
            ..FormatOptions::default()
        };
        let result = format_document(&parse("@article{key,title={A},author={B}}"), &options);
        assert!(result.output.contains("title         = {A}"));
    }

    #[test]
    fn sorts_special_entries_before_regular_entries() {
        let source =
            "@article{b,year=2008}\n@string{mar=\"march\"}\n@article{a,year=2001}\n@preamble{foo}";
        let options = FormatOptions {
            sort: Some(vec!["special".to_string()]),
            ..FormatOptions::default()
        };
        let result = format_document(&parse(source), &options);
        let string_position = result.output.find("@string").unwrap();
        let article_position = result.output.find("@article").unwrap();
        assert!(string_position < article_position);
        assert!(result.output.find("@preamble").unwrap() < article_position);
    }
}
