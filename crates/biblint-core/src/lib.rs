//! Lint rules, diagnostics, and formatting orchestration for biblint.

use biblint_format::{better_bibtex_key_suggestions_with_formula, format_document};
use biblint_syntax::{Document, Item, ParseError, TextRange, parse};
use globset::GlobBuilder;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub use biblint_format::{
    DEFAULT_FIELD_ORDER, DEFAULT_SPACE, DEFAULT_WRAP, DuplicateKind, FormatOptions, FormatResult,
    Indent, KeyGenerationOptions, MergeStrategy, unsupported_escape_characters,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Note,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FixSafety {
    Safe,
    Unsafe,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TextEdit {
    pub range: TextRange,
    pub replacement: String,
    pub safety: FixSafety,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RelatedLocation {
    pub path: PathBuf,
    pub range: TextRange,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Diagnostic {
    pub rule: Rule,
    pub severity: Severity,
    pub message: String,
    pub path: PathBuf,
    pub range: TextRange,
    pub help: Option<String>,
    pub related: Vec<RelatedLocation>,
    pub fix: Option<TextEdit>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    SyntaxError,
    Formatting,
    MissingKey,
    KeyFormat,
    DuplicateKey,
    DuplicateDoi,
    DuplicateCitation,
    DuplicateAbstract,
    DuplicateField,
    EmptyField,
    UnsupportedEscape,
    UnsupportedConstruct,
}

impl Rule {
    pub const ALL: &'static [Self] = &[
        Self::SyntaxError,
        Self::Formatting,
        Self::MissingKey,
        Self::KeyFormat,
        Self::DuplicateKey,
        Self::DuplicateDoi,
        Self::DuplicateCitation,
        Self::DuplicateAbstract,
        Self::DuplicateField,
        Self::EmptyField,
        Self::UnsupportedEscape,
        Self::UnsupportedConstruct,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::SyntaxError => "syntax_error",
            Self::Formatting => "formatting",
            Self::MissingKey => "missing_key",
            Self::KeyFormat => "key_format",
            Self::DuplicateKey => "duplicate_key",
            Self::DuplicateDoi => "duplicate_doi",
            Self::DuplicateCitation => "duplicate_citation",
            Self::DuplicateAbstract => "duplicate_abstract",
            Self::DuplicateField => "duplicate_field",
            Self::EmptyField => "empty_field",
            Self::UnsupportedEscape => "unsupported_escape",
            Self::UnsupportedConstruct => "unsupported_construct",
        }
    }

    #[must_use]
    pub const fn summary(self) -> &'static str {
        match self {
            Self::SyntaxError => "BibTeX syntax is incomplete or unbalanced",
            Self::Formatting => "the document is not in biblint's canonical format",
            Self::MissingKey => "an entry does not have a citation key",
            Self::KeyFormat => "a citation key does not match the configured format",
            Self::DuplicateKey => "a citation key is used more than once",
            Self::DuplicateDoi => "two entries have the same DOI",
            Self::DuplicateCitation => {
                "two entries have the same author/title citation fingerprint"
            }
            Self::DuplicateAbstract => "two entries have the same abstract prefix",
            Self::DuplicateField => "an entry repeats a field name",
            Self::EmptyField => "a field has no value",
            Self::UnsupportedEscape => "a character has no built-in LaTeX escape",
            Self::UnsupportedConstruct => "the formatter preserved source it does not understand",
        }
    }

    #[must_use]
    pub const fn default_enabled(self) -> bool {
        !matches!(
            self,
            Self::KeyFormat
                | Self::DuplicateDoi
                | Self::DuplicateCitation
                | Self::DuplicateAbstract
        )
    }

    #[must_use]
    pub const fn fix_safety(self) -> Option<FixSafety> {
        match self {
            Self::Formatting => Some(FixSafety::Safe),
            Self::SyntaxError
            | Self::MissingKey
            | Self::KeyFormat
            | Self::DuplicateKey
            | Self::DuplicateDoi
            | Self::DuplicateCitation
            | Self::DuplicateAbstract
            | Self::DuplicateField
            | Self::EmptyField
            | Self::UnsupportedEscape
            | Self::UnsupportedConstruct => None,
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|rule| rule.name() == name)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    #[serde(rename = "default-exclude")]
    pub default_exclude: bool,
    pub lint: LintSettings,
    pub format: FormatOptions,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            include: Vec::new(),
            exclude: Vec::new(),
            default_exclude: true,
            lint: LintSettings::default(),
            format: FormatOptions::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LintSettings {
    pub select: Vec<String>,
    #[serde(rename = "extend-select")]
    pub extend_select: Vec<String>,
    pub ignore: Vec<String>,
    #[serde(rename = "per-file-ignores")]
    pub per_file_ignores: HashMap<String, Vec<String>>,
    #[serde(rename = "key-format")]
    pub key_format: KeyFormatSettings,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum KeyFormatStyle {
    #[default]
    BetterBibtex,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct KeyFormatSettings {
    pub style: KeyFormatStyle,
}

pub const DEFAULT_EXCLUDE: &[&str] = &[".git/**", "target/**"];

impl Settings {
    #[must_use]
    pub fn rule_enabled(&self, rule: Rule) -> bool {
        let matches = |selection: &str| selection == "all" || selection == rule.name();
        if self.lint.ignore.iter().any(|ignored| matches(ignored)) {
            return false;
        }
        let extended = self
            .lint
            .extend_select
            .iter()
            .any(|selected| matches(selected));
        if self.lint.select.is_empty() {
            rule.default_enabled() || extended
        } else {
            self.lint.select.iter().any(|selected| matches(selected)) || extended
        }
    }

    #[must_use]
    pub fn rule_enabled_for_path(&self, rule: Rule, path: &Path) -> bool {
        if !self.rule_enabled(rule) {
            return false;
        }
        let normalized = path.to_string_lossy().replace('\\', "/");
        !self.lint.per_file_ignores.iter().any(|(pattern, rules)| {
            path_matches(pattern, &normalized)
                && rules
                    .iter()
                    .any(|ignored| ignored == "all" || ignored == rule.name())
        })
    }
}

pub fn load_settings(
    start: &Path,
    explicit: Option<&Path>,
) -> Result<(Settings, Option<PathBuf>), String> {
    let path = if let Some(path) = explicit {
        Some(path.to_path_buf())
    } else {
        let mut directory = if start.is_dir() {
            start.to_path_buf()
        } else {
            start
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        };
        loop {
            let candidate = directory.join("biblint.toml");
            if candidate.is_file() {
                break Some(candidate);
            }
            if !directory.pop() {
                break None;
            }
        }
    };
    let Some(path) = path else {
        return Ok((Settings::default(), None));
    };
    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let settings: Settings = toml::from_str(&contents)
        .map_err(|error| format!("invalid {}: {error}", path.display()))?;
    validate_rule_selections(&settings, &path)?;
    validate_globs(&settings, &path)?;
    settings.format.key_generation.validate().map_err(|error| {
        format!(
            "invalid {}: invalid [format.key-generation].formula: {error}",
            path.display()
        )
    })?;
    Ok((settings, Some(path)))
}

fn validate_rule_selections(settings: &Settings, path: &Path) -> Result<(), String> {
    for selection in settings
        .lint
        .select
        .iter()
        .chain(&settings.lint.extend_select)
        .chain(&settings.lint.ignore)
        .chain(settings.lint.per_file_ignores.values().flatten())
    {
        if selection != "all" && Rule::from_name(selection).is_none() {
            return Err(format!(
                "invalid {}: unknown rule '{selection}'",
                path.display()
            ));
        }
    }
    Ok(())
}

fn validate_globs(settings: &Settings, path: &Path) -> Result<(), String> {
    for pattern in settings
        .include
        .iter()
        .chain(&settings.exclude)
        .chain(settings.lint.per_file_ignores.keys())
    {
        GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .map_err(|error| {
                format!(
                    "invalid {}: invalid glob '{pattern}': {error}",
                    path.display()
                )
            })?;
    }
    Ok(())
}

fn glob_matches(pattern: &str, text: &str) -> bool {
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .is_ok_and(|glob| glob.compile_matcher().is_match(text))
}

fn path_matches(pattern: &str, path: &str) -> bool {
    if glob_matches(pattern, path) {
        return true;
    }
    path.match_indices('/')
        .map(|(index, _)| &path[index + 1..])
        .any(|suffix| glob_matches(pattern, suffix))
}

#[derive(Clone, Debug)]
pub struct CheckedDocument {
    pub parse: Document,
    pub diagnostics: Vec<Diagnostic>,
}

/// Check one source string.  Formatting is represented as one safe whole-file
/// fix, while semantic observations remain diagnostics that require a human
/// decision.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn check_source(source: &str, path: &Path, settings: &Settings) -> CheckedDocument {
    let parsed = parse(source);
    let mut diagnostics = parsed
        .errors
        .iter()
        .map(|error| syntax_diagnostic(error, path))
        .collect::<Vec<_>>();

    let key_suggestions = if settings.rule_enabled_for_path(Rule::KeyFormat, path) {
        match settings.lint.key_format.style {
            KeyFormatStyle::BetterBibtex => better_bibtex_key_suggestions_with_formula(
                &parsed,
                &settings.format.key_generation.formula,
            )
            .ok(),
        }
    } else {
        None
    };
    let mut keys: HashMap<String, (TextRange, String)> = HashMap::new();
    let mut values: HashMap<DuplicateKind, HashMap<String, (TextRange, String)>> = HashMap::new();
    for (item_index, item) in parsed.items.iter().enumerate() {
        let Item::Entry(entry) = item else {
            if let Item::Raw(raw) = item
                && !raw.text.trim().is_empty()
            {
                diagnostics.push(Diagnostic {
                    rule: Rule::UnsupportedConstruct,
                    severity: Severity::Note,
                    message: "preserved source is not a structured BibTeX construct".to_string(),
                    path: path.to_path_buf(),
                    range: raw.range,
                    help: Some("biblint leaves this text unchanged".to_string()),
                    related: Vec::new(),
                    fix: None,
                });
            }
            continue;
        };

        let key = entry.key.as_ref();
        if let Some(key) = key {
            if let Some(Some(expected)) = key_suggestions
                .as_ref()
                .and_then(|suggestions| suggestions.get(item_index))
                && key != expected
            {
                diagnostics.push(Diagnostic {
                    rule: Rule::KeyFormat,
                    severity: Severity::Warning,
                    message: format!(
                        "citation key '{key}' does not match Better BibTeX suggestion '{expected}'"
                    ),
                    path: path.to_path_buf(),
                    range: entry.key_range.unwrap_or(entry.range),
                    help: Some(format!(
                        "rename it to '{expected}' if this key is generated; biblint does not rename citation keys automatically"
                    )),
                    related: Vec::new(),
                    fix: None,
                });
            }
            let key_id = key.to_ascii_lowercase();
            if let Some((previous_range, previous_key)) = keys.get(&key_id) {
                diagnostics.push(Diagnostic {
                    rule: Rule::DuplicateKey,
                    severity: Severity::Warning,
                    message: format!("citation key '{key}' has already been used"),
                    path: path.to_path_buf(),
                    range: entry.key_range.unwrap_or(entry.range),
                    help: Some("rename one of the entries".to_string()),
                    related: vec![RelatedLocation {
                        path: path.to_path_buf(),
                        range: *previous_range,
                        message: format!("first use of '{previous_key}'"),
                    }],
                    fix: None,
                });
            } else {
                keys.insert(
                    key_id,
                    (entry.key_range.unwrap_or(entry.range), key.clone()),
                );
            }
        } else {
            diagnostics.push(Diagnostic {
                rule: Rule::MissingKey,
                severity: Severity::Warning,
                message: format!("@{} entry does not have a citation key", entry.command),
                path: path.to_path_buf(),
                range: entry.range,
                help: Some("add a key after the opening delimiter".to_string()),
                related: Vec::new(),
                fix: None,
            });
        }

        for field in &entry.fields {
            let Some(value) = &field.value else {
                diagnostics.push(empty_field_diagnostic(field, path));
                continue;
            };
            if value.is_empty() {
                diagnostics.push(empty_field_diagnostic(field, path));
            }
            if settings.format.escape
                && !settings.format.unescape
                && !matches!(
                    field.name.to_ascii_lowercase().as_str(),
                    "url" | "doi" | "file" | "pdf" | "verba" | "verbb" | "verbc"
                )
            {
                for character in unsupported_escape_characters(&value.render()) {
                    diagnostics.push(Diagnostic {
                        rule: Rule::UnsupportedEscape,
                        severity: Severity::Note,
                        message: format!(
                            "cannot escape character {character} (U+{:04X}) in field '{}'",
                            u32::from(character),
                            field.name
                        ),
                        path: path.to_path_buf(),
                        range: field.range,
                        help: Some(
                            "biblint preserves the character; use a package or set `escape = false` in the format configuration if that is intentional"
                                .to_string(),
                        ),
                        related: Vec::new(),
                        fix: None,
                    });
                }
            }
        }
        let mut field_names = HashMap::<String, TextRange>::new();
        for field in &entry.fields {
            let normalized = field.name.to_ascii_lowercase();
            if let Some(previous_range) = field_names.get(&normalized) {
                diagnostics.push(Diagnostic {
                    rule: Rule::DuplicateField,
                    severity: Severity::Warning,
                    message: format!(
                        "field '{}' is repeated in '{}'",
                        field.name,
                        key.map_or("<missing key>", String::as_str)
                    ),
                    path: path.to_path_buf(),
                    range: field.range,
                    help: Some("remove the duplicate or choose which value should win".to_string()),
                    related: vec![RelatedLocation {
                        path: path.to_path_buf(),
                        range: *previous_range,
                        message: "first field with this name".to_string(),
                    }],
                    fix: None,
                });
            } else {
                field_names.insert(normalized, field.range);
            }
        }

        check_optional_duplicate(
            &mut diagnostics,
            &mut values,
            entry,
            path,
            DuplicateKind::Doi,
            Rule::DuplicateDoi,
            "DOI",
            settings.rule_enabled_for_path(Rule::DuplicateDoi, path),
        );
        check_optional_duplicate(
            &mut diagnostics,
            &mut values,
            entry,
            path,
            DuplicateKind::Citation,
            Rule::DuplicateCitation,
            "citation",
            settings.rule_enabled_for_path(Rule::DuplicateCitation, path),
        );
        check_optional_duplicate(
            &mut diagnostics,
            &mut values,
            entry,
            path,
            DuplicateKind::Abstract,
            Rule::DuplicateAbstract,
            "abstract",
            settings.rule_enabled_for_path(Rule::DuplicateAbstract, path),
        );
    }

    if parsed.errors.is_empty() {
        let formatted = format_document(&parsed, &settings.format);
        if formatted.output != source {
            diagnostics.push(Diagnostic {
                rule: Rule::Formatting,
                severity: Severity::Warning,
                message: "file is not canonically formatted".to_string(),
                path: path.to_path_buf(),
                range: TextRange::new(0, source.len()),
                help: Some("run `biblint format` or use `biblint check --fix`".to_string()),
                related: Vec::new(),
                fix: Some(TextEdit {
                    range: TextRange::new(0, source.len()),
                    replacement: formatted.output,
                    safety: if settings.format.has_unsafe_transforms() {
                        FixSafety::Unsafe
                    } else {
                        FixSafety::Safe
                    },
                }),
            });
        }
    }
    diagnostics.retain(|diagnostic| settings.rule_enabled_for_path(diagnostic.rule, path));
    diagnostics.sort_by_key(|diagnostic| (diagnostic.range.start, diagnostic.range.end));
    CheckedDocument {
        parse: parsed,
        diagnostics,
    }
}

/// Check a collection of sources as one bibliography project.
///
/// `check_source` remains useful for editor integrations and single-file
/// checks. This project-level entry point adds cross-file duplicate checks so
/// a directory invocation does not silently miss collisions between files.
#[must_use]
pub fn check_sources(sources: &[(PathBuf, String)], settings: &Settings) -> Vec<Diagnostic> {
    let checked = sources
        .iter()
        .map(|(path, source)| check_source(source, path, settings))
        .collect::<Vec<_>>();
    let mut diagnostics = checked
        .iter()
        .flat_map(|document| document.diagnostics.iter().cloned())
        .collect::<Vec<_>>();

    let duplicate_rules = [
        (DuplicateKind::Key, Rule::DuplicateKey, "citation key"),
        (DuplicateKind::Doi, Rule::DuplicateDoi, "DOI"),
        (DuplicateKind::Citation, Rule::DuplicateCitation, "citation"),
        (DuplicateKind::Abstract, Rule::DuplicateAbstract, "abstract"),
    ];
    let mut seen: HashMap<(Rule, String), (PathBuf, TextRange, String)> = HashMap::new();

    for ((path, _), document) in sources.iter().zip(&checked) {
        let mut seen_in_file = HashSet::new();
        for item in &document.parse.items {
            let Item::Entry(entry) = item else {
                continue;
            };
            let location = entry.key_range.unwrap_or(entry.range);
            let key = entry
                .key
                .clone()
                .unwrap_or_else(|| "<missing key>".to_string());
            for (kind, rule, label) in duplicate_rules {
                if !settings.rule_enabled_for_path(rule, path) {
                    continue;
                }
                let Some(fingerprint) = duplicate_fingerprint(entry, kind) else {
                    continue;
                };
                if !seen_in_file.insert((rule, fingerprint.clone())) {
                    continue;
                }
                let id = (rule, fingerprint);
                if let Some((previous_path, previous_range, previous_key)) = seen.get(&id) {
                    if previous_path != path {
                        let message = if rule == Rule::DuplicateKey {
                            format!("citation key '{key}' has already been used")
                        } else {
                            format!("entry '{key}' has a duplicate {label} with '{previous_key}'")
                        };
                        diagnostics.push(Diagnostic {
                            rule,
                            severity: Severity::Warning,
                            message,
                            path: path.clone(),
                            range: location,
                            help: Some(if rule == Rule::DuplicateKey {
                                "rename one of the entries".to_string()
                            } else {
                                "inspect the entries and keep only the intended record".to_string()
                            }),
                            related: vec![RelatedLocation {
                                path: previous_path.clone(),
                                range: *previous_range,
                                message: format!("first use of '{label}'"),
                            }],
                            fix: None,
                        });
                    }
                } else {
                    seen.insert(id, (path.clone(), location, key.clone()));
                }
            }
        }
    }

    diagnostics.sort_by_key(|diagnostic| {
        (
            diagnostic.path.clone(),
            diagnostic.range.start,
            diagnostic.range.end,
        )
    });
    diagnostics
}

fn syntax_diagnostic(error: &ParseError, path: &Path) -> Diagnostic {
    Diagnostic {
        rule: Rule::SyntaxError,
        severity: Severity::Error,
        message: error.message.clone(),
        path: path.to_path_buf(),
        range: error.range,
        help: Some("close the BibTeX block or quoted value".to_string()),
        related: Vec::new(),
        fix: None,
    }
}

fn empty_field_diagnostic(field: &biblint_syntax::Field, path: &Path) -> Diagnostic {
    Diagnostic {
        rule: Rule::EmptyField,
        severity: Severity::Warning,
        message: format!("field '{}' has no value", field.name),
        path: path.to_path_buf(),
        range: field.range,
        help: Some("remove the field or provide a value".to_string()),
        related: Vec::new(),
        fix: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn check_optional_duplicate(
    diagnostics: &mut Vec<Diagnostic>,
    values: &mut HashMap<DuplicateKind, HashMap<String, (TextRange, String)>>,
    entry: &biblint_syntax::Entry,
    path: &Path,
    kind: DuplicateKind,
    rule: Rule,
    label: &str,
    enabled: bool,
) {
    if !enabled
        || !matches!(
            kind,
            DuplicateKind::Doi | DuplicateKind::Citation | DuplicateKind::Abstract
        )
    {
        return;
    }
    let Some(fingerprint) = duplicate_fingerprint(entry, kind) else {
        return;
    };
    let location = entry.key_range.unwrap_or(entry.range);
    let key = entry
        .key
        .clone()
        .unwrap_or_else(|| "<missing key>".to_string());
    let index = values.entry(kind).or_default();
    if let Some((previous_range, previous_key)) = index.get(&fingerprint) {
        diagnostics.push(Diagnostic {
            rule,
            severity: Severity::Warning,
            message: format!("entry '{key}' has a duplicate {label} with '{previous_key}'"),
            path: path.to_path_buf(),
            range: location,
            help: Some("inspect the entries and keep only the intended record".to_string()),
            related: vec![RelatedLocation {
                path: path.to_path_buf(),
                range: *previous_range,
                message: format!("first entry with this {label}"),
            }],
            fix: None,
        });
    } else {
        index.insert(fingerprint, (location, key));
    }
}

fn duplicate_fingerprint(entry: &biblint_syntax::Entry, kind: DuplicateKind) -> Option<String> {
    let value = |name: &str| {
        entry
            .fields
            .iter()
            .find(|field| field.name.eq_ignore_ascii_case(name))
            .and_then(|field| field.value.as_ref())
            .map(biblint_syntax::Value::render)
    };
    let normalized = |value: String| {
        let result: String = value
            .chars()
            .filter(|character| character.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        (!result.is_empty()).then_some(result)
    };
    match kind {
        DuplicateKind::Doi => value("doi").and_then(normalized),
        DuplicateKind::Abstract => value("abstract")
            .and_then(normalized)
            .map(|value| value.chars().take(100).collect()),
        DuplicateKind::Citation => {
            let title = normalized(value("title")?)?;
            let author = normalized(first_author_surname(&value("author")?))?;
            let number = normalized(value("number").unwrap_or_else(|| "0".to_string()))
                .unwrap_or_else(|| "0".to_string());
            Some(format!("{author}:{title}:{number}"))
        }
        DuplicateKind::Key => entry.key.as_ref().map(|key| key.to_ascii_lowercase()),
    }
}

fn first_author_surname(value: &str) -> String {
    let first = split_first_creator(value);
    let first = first.trim().trim_matches(['{', '}']).trim();
    if let Some((surname, _)) = first.split_once(',') {
        surname.trim().to_string()
    } else {
        first
            .split_whitespace()
            .next_back()
            .unwrap_or_default()
            .to_string()
    }
}

fn split_first_creator(value: &str) -> &str {
    let mut depth = 0usize;
    for (position, character) in value.char_indices() {
        match character {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            'a' | 'A' if depth == 0 => {
                let Some(candidate) = value.get(position..position + 3) else {
                    continue;
                };
                let before = position
                    .checked_sub(1)
                    .and_then(|index| value.as_bytes().get(index))
                    .copied();
                let after = value.as_bytes().get(position + 3).copied();
                if candidate.eq_ignore_ascii_case("and")
                    && before.is_some_and(|character| character.is_ascii_whitespace())
                    && after.is_some_and(|character| character.is_ascii_whitespace())
                {
                    return value[..position].trim();
                }
            }
            _ => {}
        }
    }
    value.trim()
}

/// Format a source string.
#[must_use]
pub fn format_source(source: &str, options: &FormatOptions) -> (Document, FormatResult) {
    let parsed = parse(source);
    let result = format_document(&parsed, options);
    (parsed, result)
}

/// Apply non-overlapping safe fixes in source order.
#[must_use]
pub fn apply_fixes(source: &str, diagnostics: &[Diagnostic], unsafe_fixes: bool) -> String {
    let mut edits: Vec<&TextEdit> = diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.fix.as_ref())
        .filter(|edit| unsafe_fixes || edit.safety == FixSafety::Safe)
        .collect();
    edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
    if edits
        .windows(2)
        .any(|pair| pair[0].range.end > pair[1].range.start)
    {
        return source.to_string();
    }
    let mut output = source.to_string();
    for edit in edits.into_iter().rev() {
        if edit.range.end <= output.len() && edit.range.start <= edit.range.end {
            output.replace_range(edit.range.start..edit.range.end, &edit.replacement);
        }
    }
    output
}

/// Discover BibTeX files under paths. Explicit files are accepted regardless
/// of extension; directories contribute `.bib` files recursively.
pub fn discover_files(paths: &[PathBuf], settings: &Settings) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for path in paths {
        if path == Path::new("-") {
            continue;
        }
        if path.is_file() {
            if configured(path, path.parent(), settings) {
                files.push(path.clone());
            }
        } else if path.is_dir() {
            visit_directory(path, path, settings, &mut files)?;
        } else {
            return Err(format!("path does not exist: {}", path.display()));
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn visit_directory(
    path: &Path,
    root: &Path,
    settings: &Settings,
    files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let entries = fs::read_dir(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not read directory entry: {error}"))?;
        let child = entry.path();
        if child.is_dir() {
            if settings.default_exclude
                && child
                    .file_name()
                    .is_some_and(|name| name == ".git" || name == "target")
            {
                continue;
            }
            visit_directory(&child, root, settings, files)?;
        } else if child
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("bib"))
            && configured(&child, Some(root), settings)
        {
            files.push(child);
        }
    }
    Ok(())
}

fn configured(path: &Path, root: Option<&Path>, settings: &Settings) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let relative = root
        .and_then(|base| path.strip_prefix(base).ok())
        .map(|value| value.to_string_lossy().replace('\\', "/"));
    let matches = |pattern: &str| {
        path_matches(pattern, &normalized)
            || relative
                .as_deref()
                .is_some_and(|candidate| path_matches(pattern, candidate))
            || path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| glob_matches(pattern, name))
    };
    let excluded = settings.exclude.iter().any(|pattern| matches(pattern))
        || (settings.default_exclude && DEFAULT_EXCLUDE.iter().any(|pattern| matches(pattern)));
    !excluded
        && (settings.include.is_empty() || settings.include.iter().any(|pattern| matches(pattern)))
}

#[cfg(test)]
mod tests {
    use super::{
        KeyFormatSettings, KeyFormatStyle, LintSettings, Rule, Settings, check_source,
        check_sources,
    };
    use biblint_format::{FormatOptions, Indent};
    use std::path::Path;

    #[test]
    fn reports_duplicate_keys_and_offers_formatting_fix() {
        let checked = check_source(
            "@article{A,title={x}}\n@article{a,title={y}}\n",
            Path::new("refs.bib"),
            &Settings::default(),
        );
        assert!(
            checked
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule == Rule::DuplicateKey)
        );
        assert!(
            checked
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule == Rule::Formatting)
        );
    }

    #[test]
    fn missing_keys_do_not_skip_field_diagnostics() {
        let checked = check_source(
            "@article{title={x}, title={y}, empty=}",
            Path::new("refs.bib"),
            &Settings::default(),
        );
        assert!(
            checked
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule == Rule::MissingKey)
        );
        assert!(
            checked
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule == Rule::DuplicateField)
        );
        assert!(
            checked
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule == Rule::EmptyField)
        );
    }

    #[test]
    fn project_checks_find_cross_file_duplicate_keys_and_dois() {
        let sources = vec![
            (
                std::path::PathBuf::from("one.bib"),
                "@article{shared,doi={10/x}}\n".to_string(),
            ),
            (
                std::path::PathBuf::from("two.bib"),
                "@article{SHARED,doi={10/x}}\n".to_string(),
            ),
        ];
        let settings = Settings {
            lint: LintSettings {
                extend_select: vec!["duplicate_doi".to_string()],
                ..LintSettings::default()
            },
            ..Settings::default()
        };
        let diagnostics = check_sources(&sources, &settings);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.rule == Rule::DuplicateKey
                && diagnostic.path == Path::new("two.bib")
                && diagnostic
                    .related
                    .first()
                    .is_some_and(|related| related.path == Path::new("one.bib"))
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.rule == Rule::DuplicateDoi
                && diagnostic.path == Path::new("two.bib")
                && diagnostic
                    .related
                    .first()
                    .is_some_and(|related| related.path == Path::new("one.bib"))
        }));
    }

    #[test]
    fn optional_duplicate_rules_are_explicit() {
        let lint = LintSettings {
            extend_select: vec!["duplicate_doi".to_string()],
            ..LintSettings::default()
        };
        let checked = check_source(
            "@article{a,doi={10/x}}\n@article{b,doi={10/x}}\n",
            Path::new("refs.bib"),
            &Settings {
                lint,
                ..Settings::default()
            },
        );
        assert!(
            checked
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule == Rule::DuplicateDoi)
        );
    }

    #[test]
    fn better_bibtex_key_format_is_opt_in_and_diagnostic_only() {
        let source = "@article{wrong,author={Smith, John},title={A Study of Things},year={2024}}\n";
        let default_checked = check_source(source, Path::new("refs.bib"), &Settings::default());
        assert!(
            !default_checked
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule == Rule::KeyFormat)
        );

        let settings = Settings {
            lint: LintSettings {
                extend_select: vec!["key_format".to_string()],
                key_format: KeyFormatSettings {
                    style: KeyFormatStyle::BetterBibtex,
                },
                ..LintSettings::default()
            },
            ..Settings::default()
        };
        let checked = check_source(source, Path::new("refs.bib"), &settings);
        let diagnostic = checked
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.rule == Rule::KeyFormat)
            .expect("key format diagnostic");
        assert!(diagnostic.message.contains("smith2024Study"));
        assert!(diagnostic.fix.is_none());

        let correct = check_source(
            "@article{smith2024Study,author={Smith, John},title={A Study of Things},year={2024}}\n",
            Path::new("refs.bib"),
            &settings,
        );
        assert!(
            !correct
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule == Rule::KeyFormat)
        );
    }

    #[test]
    fn notes_unicode_that_the_escape_table_cannot_handle() {
        let checked = check_source(
            "@article{a,title={A ↳ B}}\n",
            Path::new("refs.bib"),
            &Settings {
                format: FormatOptions {
                    escape: true,
                    ..FormatOptions::default()
                },
                ..Settings::default()
            },
        );
        assert!(checked.diagnostics.iter().any(|diagnostic| {
            diagnostic.rule == Rule::UnsupportedEscape && diagnostic.message.contains("U+21B3")
        }));
    }

    #[test]
    fn toml_controls_rules_and_formatting_policy() {
        let settings: Settings = toml::from_str(
            r#"
            [lint]
            select = ["formatting"]

            [lint.key-format]
            style = "better-bibtex"

            [format]
            indent = "tab"
            align = false
            sort = ["key"]

            [format.key-generation]
            formula = "auth.lower + year"
            "#,
        )
        .expect("valid biblint configuration");
        assert_eq!(settings.format.indent, Indent::Tab);
        assert_eq!(settings.format.align, None);
        assert_eq!(settings.format.sort, Some(vec!["key".to_string()]));
        assert_eq!(settings.format.key_generation.formula, "auth.lower + year");
        assert_eq!(settings.lint.key_format.style, KeyFormatStyle::BetterBibtex);

        let checked = check_source(
            "@article{A,title={x}}\n@article{a,title={y}}\n",
            Path::new("refs.bib"),
            &settings,
        );
        assert!(
            checked
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule == Rule::Formatting)
        );
        assert!(
            !checked
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule == Rule::DuplicateKey)
        );
    }
}
