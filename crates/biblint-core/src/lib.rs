//! Lint rules, diagnostics, and formatting orchestration for biblint.

use biblint_format::{better_bibtex_key_suggestions_with_formula_and_suffix, format_document};
use biblint_syntax::{Document, Item, ParseError, TextRange, Value, parse};
use globset::GlobBuilder;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub use biblint_format::{
    CollisionSuffixStyle, DEFAULT_FIELD_ORDER, DEFAULT_SPACE, DEFAULT_WRAP, DuplicateKind,
    FormatOptions, FormatResult, Indent, KeyGenerationOptions, MergeStrategy,
    unsupported_escape_characters,
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
    NonstandardNameSeparator,
    InvalidSuppression,
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
        Self::NonstandardNameSeparator,
        Self::InvalidSuppression,
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
            Self::NonstandardNameSeparator => "nonstandard_name_separator",
            Self::InvalidSuppression => "invalid_suppression",
            Self::UnsupportedEscape => "unsupported_escape",
            Self::UnsupportedConstruct => "unsupported_construct",
        }
    }

    #[must_use]
    pub const fn summary(self) -> &'static str {
        match self {
            Self::SyntaxError => "BibTeX syntax is incomplete or unbalanced",
            Self::Formatting => "the file would be reformatted by biblint",
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
            Self::NonstandardNameSeparator => {
                "a name-list field uses a non-BibTeX creator separator"
            }
            Self::InvalidSuppression => "a suppression comment is invalid or obsolete",
            Self::UnsupportedEscape => "a character has no built-in LaTeX escape",
            Self::UnsupportedConstruct => "the formatter preserved source it does not understand",
        }
    }

    #[must_use]
    pub const fn default_enabled(self) -> bool {
        !matches!(
            self,
            Self::KeyFormat
                | Self::Formatting
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
            | Self::NonstandardNameSeparator
            | Self::InvalidSuppression
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
    suppressions: Vec<Suppression>,
}

#[derive(Clone, Debug)]
struct Suppression {
    rule: Rule,
    target: TextRange,
    directive: TextRange,
}

#[derive(Clone, Debug)]
struct SuppressionIssue {
    range: TextRange,
    message: String,
}

#[derive(Clone, Debug, Default)]
struct SuppressionParse {
    entries: Vec<Suppression>,
    issues: Vec<SuppressionIssue>,
}

/// Check one source string. Formatting is represented as one whole-file fix,
/// while semantic observations remain diagnostics that require a human
/// decision.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn check_source(source: &str, path: &Path, settings: &Settings) -> CheckedDocument {
    let parsed = parse(source);
    let SuppressionParse {
        entries: suppressions,
        issues: suppression_issues,
    } = parse_suppressions(&parsed, source);
    let mut diagnostics = parsed
        .errors
        .iter()
        .map(|error| syntax_diagnostic(error, path))
        .collect::<Vec<_>>();

    let key_suggestions = if settings.rule_enabled_for_path(Rule::KeyFormat, path) {
        match settings.lint.key_format.style {
            KeyFormatStyle::BetterBibtex => better_bibtex_key_suggestions_with_formula_and_suffix(
                &parsed,
                &settings.format.key_generation.formula,
                settings.format.key_generation.collision_suffix,
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
            if is_name_list_field(&field.name) {
                for separator in detect_name_separators(value) {
                    diagnostics.push(nonstandard_name_separator_diagnostic(
                        field, path, separator,
                    ));
                }
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
                severity: Severity::Note,
                message: "file would be reformatted".to_string(),
                path: path.to_path_buf(),
                range: TextRange::new(0, source.len()),
                help: Some(
                    "run `biblint format <path> --diff` or use `biblint check --format --fix`"
                        .to_string(),
                ),
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
    for issue in suppression_issues {
        diagnostics.push(invalid_suppression_diagnostic(&issue, path));
    }
    for suppression in &suppressions {
        let used = diagnostics.iter().any(|diagnostic| {
            diagnostic.rule == suppression.rule
                && settings.rule_enabled_for_path(diagnostic.rule, path)
                && ranges_intersect(diagnostic.range, suppression.target)
        });
        if !used {
            diagnostics.push(outdated_suppression_diagnostic(suppression, path));
        }
    }
    diagnostics.retain(|diagnostic| {
        settings.rule_enabled_for_path(diagnostic.rule, path)
            && !suppression_applies(&suppressions, diagnostic.rule, diagnostic.range)
    });
    diagnostics.sort_by_key(|diagnostic| (diagnostic.range.start, diagnostic.range.end));
    CheckedDocument {
        parse: parsed,
        diagnostics,
        suppressions,
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
                        if suppression_applies(&document.suppressions, rule, location) {
                            for suppression in document.suppressions.iter().filter(|suppression| {
                                suppression.rule == rule
                                    && ranges_intersect(suppression.target, location)
                            }) {
                                remove_outdated_suppression_diagnostic(
                                    &mut diagnostics,
                                    path,
                                    suppression.directive,
                                );
                            }
                            continue;
                        }
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

fn parse_suppressions(document: &Document, source: &str) -> SuppressionParse {
    let mut result = SuppressionParse::default();
    for (index, item) in document.items.iter().enumerate() {
        let Item::Comment(comment) = item else {
            continue;
        };
        let Some(parsed) = parse_suppression_comment(&comment.text) else {
            continue;
        };
        let rules = match parsed {
            Ok(rules) => rules,
            Err(message) => {
                result.issues.push(SuppressionIssue {
                    range: comment.range,
                    message,
                });
                continue;
            }
        };
        if !is_standalone_comment(source, comment.range) {
            result.issues.push(SuppressionIssue {
                range: comment.range,
                message: "suppression comments must be on their own line before an entry"
                    .to_string(),
            });
            continue;
        }
        let Some(target) = next_entry_range(document, index) else {
            result.issues.push(SuppressionIssue {
                range: comment.range,
                message: "suppression comment is not followed by a BibTeX entry".to_string(),
            });
            continue;
        };
        result
            .entries
            .extend(rules.into_iter().map(|rule| Suppression {
                rule,
                target,
                directive: comment.range,
            }));
    }
    result
}

fn parse_suppression_comment(text: &str) -> Option<Result<Vec<Rule>, String>> {
    let text = text.trim_start();
    let text = text.strip_prefix('%')?.trim_start();
    let remainder = text.strip_prefix("biblint-ignore")?;
    if let Some(character) = remainder.chars().next()
        && !character.is_whitespace()
    {
        if matches!(character, ':' | '-') {
            return Some(Err(
                "only next-entry suppressions are supported; use `% biblint-ignore <rule> [<rule> ...][: <reason>]`"
                    .to_string(),
            ));
        }
        return None;
    }
    let payload = remainder.trim();
    if payload.is_empty() {
        return Some(Err("suppression must name at least one rule".to_string()));
    }

    let rules_text = payload
        .split_once(':')
        .map_or(payload, |(rules, _reason)| rules)
        .trim();
    if rules_text.is_empty() {
        return Some(Err("suppression must name at least one rule".to_string()));
    }

    let mut rules = Vec::new();
    for rule_name in rules_text.split_whitespace() {
        let Some(rule) = Rule::from_name(rule_name) else {
            return Some(Err(format!("unknown biblint rule '{rule_name}'")));
        };
        if matches!(
            rule,
            Rule::SyntaxError | Rule::Formatting | Rule::InvalidSuppression
        ) {
            return Some(Err(format!(
                "rule '{}' cannot be suppressed with an entry comment",
                rule.name()
            )));
        }
        rules.push(rule);
    }
    Some(Ok(rules))
}

fn is_standalone_comment(source: &str, range: TextRange) -> bool {
    let line_start = source[..range.start]
        .rfind('\n')
        .map_or(0, |position| position + 1);
    source[line_start..range.start].trim().is_empty()
}

fn next_entry_range(document: &Document, comment_index: usize) -> Option<TextRange> {
    for item in document.items.iter().skip(comment_index + 1) {
        match item {
            Item::Entry(entry) => return Some(entry.range),
            Item::Comment(_) => {}
            Item::Raw(raw) if raw.text.trim().is_empty() => {}
            Item::Raw(_) | Item::Special(_) => return None,
        }
    }
    None
}

fn invalid_suppression_diagnostic(issue: &SuppressionIssue, path: &Path) -> Diagnostic {
    Diagnostic {
        rule: Rule::InvalidSuppression,
        severity: Severity::Warning,
        message: issue.message.clone(),
        path: path.to_path_buf(),
        range: issue.range,
        help: Some(
            "use `% biblint-ignore <rule> [<rule> ...][: <reason>]` before the intended entry, or remove the directive"
                .to_string(),
        ),
        related: Vec::new(),
        fix: None,
    }
}

fn outdated_suppression_diagnostic(suppression: &Suppression, path: &Path) -> Diagnostic {
    invalid_suppression_diagnostic(
        &SuppressionIssue {
            range: suppression.directive,
            message: format!(
                "suppression for '{}' is obsolete; it matches no diagnostic",
                suppression.rule.name()
            ),
        },
        path,
    )
}

fn remove_outdated_suppression_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    path: &Path,
    directive: TextRange,
) {
    diagnostics.retain(|diagnostic| {
        !(diagnostic.rule == Rule::InvalidSuppression
            && diagnostic.path.as_path() == path
            && diagnostic.range == directive
            && diagnostic.message.contains("matches no diagnostic"))
    });
}

fn ranges_intersect(left: TextRange, right: TextRange) -> bool {
    left.start < right.end && right.start < left.end
}

fn suppression_applies(suppressions: &[Suppression], rule: Rule, range: TextRange) -> bool {
    suppressions
        .iter()
        .any(|suppression| suppression.rule == rule && ranges_intersect(suppression.target, range))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NameSeparator {
    With,
    OxfordComma,
}

fn is_name_list_field(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "author" | "editor" | "translator" | "collaborator"
    )
}

fn detect_name_separators(value: &Value) -> Vec<NameSeparator> {
    let mut has_with = false;
    let mut has_oxford_comma = false;
    for part in &value.parts {
        let text = part.render();
        let mut depth = 0usize;
        for (position, character) in text.char_indices() {
            match character {
                '{' if !is_escaped(text, position) => {
                    depth += 1;
                    continue;
                }
                '}' if !is_escaped(text, position) => {
                    depth = depth.saturating_sub(1);
                    continue;
                }
                _ => {}
            }
            if depth != 0 {
                continue;
            }
            if matches!(character, 'w' | 'W') && has_word_at(text, position, "with") {
                has_with = true;
            }
            if character == ','
                && !is_escaped(text, position)
                && has_word_after_comma(text, position, "and")
            {
                has_oxford_comma = true;
            }
        }
    }
    let mut separators = Vec::new();
    if has_with {
        separators.push(NameSeparator::With);
    }
    if has_oxford_comma {
        separators.push(NameSeparator::OxfordComma);
    }
    separators
}

fn has_word_at(text: &str, position: usize, word: &str) -> bool {
    let Some(candidate) = text.get(position..position + word.len()) else {
        return false;
    };
    let previous = text[..position].chars().next_back();
    let following = text[position + word.len()..].chars().next();
    candidate.eq_ignore_ascii_case(word)
        && previous.is_some_and(char::is_whitespace)
        && following.is_some_and(char::is_whitespace)
}

fn has_word_after_comma(text: &str, position: usize, word: &str) -> bool {
    let after_comma = &text[position + 1..];
    let trimmed = after_comma.trim_start();
    let Some(candidate) = trimmed.get(..word.len()) else {
        return false;
    };
    let following = trimmed[word.len()..].chars().next();
    candidate.eq_ignore_ascii_case(word) && following.is_some_and(char::is_whitespace)
}

fn is_escaped(text: &str, position: usize) -> bool {
    let backslashes = text[..position]
        .chars()
        .rev()
        .take_while(|character| *character == '\\')
        .count();
    backslashes % 2 == 1
}

fn nonstandard_name_separator_diagnostic(
    field: &biblint_syntax::Field,
    path: &Path,
    separator: NameSeparator,
) -> Diagnostic {
    let (message, help) = match separator {
        NameSeparator::With => (
            format!("field '{}' uses 'with' as a creator separator", field.name),
            "BibTeX separates creators with `and`; use `and` if these are separate creators, or brace an intended literal name".to_string(),
        ),
        NameSeparator::OxfordComma => (
            format!(
                "field '{}' has a comma before 'and' in a creator list",
                field.name
            ),
            "BibTeX treats commas as part of a name; separate creators with `and` and brace an intended literal name".to_string(),
        ),
    };
    Diagnostic {
        rule: Rule::NonstandardNameSeparator,
        severity: Severity::Warning,
        message,
        path: path.to_path_buf(),
        range: field.range,
        help: Some(help),
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
        KeyFormatSettings, KeyFormatStyle, LintSettings, Rule, Settings, Severity, check_source,
        check_sources,
    };
    use biblint_format::{CollisionSuffixStyle, FormatOptions, Indent};
    use std::path::Path;

    #[test]
    fn reports_duplicate_keys_and_offers_formatting_fix() {
        let settings = Settings {
            lint: LintSettings {
                extend_select: vec!["formatting".to_string()],
                ..LintSettings::default()
            },
            ..Settings::default()
        };
        let checked = check_source(
            "@article{A,title={x}}\n@article{a,title={y}}\n",
            Path::new("refs.bib"),
            &settings,
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
        assert!(checked.diagnostics.iter().any(|diagnostic| {
            diagnostic.rule == Rule::Formatting && diagnostic.severity == Severity::Note
        }));
    }

    #[test]
    fn formatting_is_not_enabled_by_default() {
        let checked = check_source(
            "@ARTICLE{key,title=\"A title\"}\n",
            Path::new("refs.bib"),
            &Settings::default(),
        );
        assert!(
            !checked
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
    fn detects_nonstandard_name_separators_but_respects_fields_and_braces() {
        let source = "@article{with,author={Simson Garfinkel with Gene Spafford},title={A with B}}\n\
            @article{comma,editor={Gretchen Stevens, Gary King, and Kenji Shibuya},title={A}}\n\
            @article{protected,author={{Smith, Jones, and Associates}},translator={John Withers and Jane Doe}}\n";
        let checked = check_source(source, Path::new("refs.bib"), &Settings::default());
        let diagnostics = checked
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.rule == Rule::NonstandardNameSeparator)
            .collect::<Vec<_>>();
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.message.contains("uses 'with'")
                && diagnostic.severity == Severity::Warning
                && diagnostic.fix.is_none()
        }));
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.message.contains("comma before 'and'") })
        );
    }

    #[test]
    fn inline_suppressions_apply_to_the_next_entry() {
        let source = "% biblint-ignore nonstandard_name_separator\n\
            @article{ignored,author={Simson Garfinkel with Gene Spafford},title={A}}\n\
            @article{reported,author={Simson Garfinkel with Gene Spafford},title={B}}\n";
        let checked = check_source(source, Path::new("refs.bib"), &Settings::default());
        let diagnostics = checked
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.rule == Rule::NonstandardNameSeparator)
            .collect::<Vec<_>>();
        assert_eq!(diagnostics.len(), 1);
        assert!(
            !checked
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule == Rule::InvalidSuppression)
        );
    }

    #[test]
    fn inline_suppressions_can_name_multiple_rules() {
        let source = "% biblint-ignore nonstandard_name_separator duplicate_field\n\
            @article{ignored,author={A with B},title={A},title={B}}\n\
            % biblint-ignore nonstandard_name_separator\n\
            % biblint-ignore duplicate_field\n\
            @article{also_ignored,author={A with B},title={A},title={B}}\n";
        let checked = check_source(source, Path::new("refs.bib"), &Settings::default());
        assert!(!checked.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.rule,
                Rule::NonstandardNameSeparator | Rule::DuplicateField
            )
        }));
        assert!(
            !checked
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule == Rule::InvalidSuppression)
        );
    }

    #[test]
    fn invalid_inline_suppressions_are_reported() {
        let source = "% biblint-ignore not_a_rule: typo\n\
            @article{unknown,author={A with B},title={A}}\n\
            % biblint-ignore nonstandard_name_separator: stale\n\
            @article{stale,author={A and B},title={C}}\n\
            % biblint-ignore nonstandard_name_separator: no following entry\n";
        let checked = check_source(source, Path::new("refs.bib"), &Settings::default());
        let invalid = checked
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.rule == Rule::InvalidSuppression)
            .collect::<Vec<_>>();
        assert_eq!(invalid.len(), 3);
        assert!(
            invalid
                .iter()
                .any(|diagnostic| { diagnostic.message.contains("unknown biblint rule") })
        );
        assert!(
            invalid
                .iter()
                .any(|diagnostic| { diagnostic.message.contains("matches no diagnostic") })
        );
        assert!(invalid.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("not followed by a BibTeX entry")
        }));
        assert!(checked.diagnostics.iter().any(|diagnostic| {
            diagnostic.rule == Rule::NonstandardNameSeparator
                && diagnostic.message.contains("uses 'with'")
        }));
    }

    #[test]
    fn inline_suppressions_apply_to_cross_file_duplicates() {
        let sources = vec![
            (
                std::path::PathBuf::from("one.bib"),
                "@article{shared,title={A}}\n".to_string(),
            ),
            (
                std::path::PathBuf::from("two.bib"),
                "% biblint-ignore duplicate_key: intentionally shared key\n@article{shared,title={B}}\n"
                    .to_string(),
            ),
        ];
        let diagnostics = check_sources(&sources, &Settings::default());
        assert!(!diagnostics.iter().any(|diagnostic| {
            diagnostic.rule == Rule::DuplicateKey && diagnostic.path == Path::new("two.bib")
        }));
        assert!(!diagnostics.iter().any(|diagnostic| {
            diagnostic.rule == Rule::InvalidSuppression
                && diagnostic.message.contains("matches no diagnostic")
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
            collision-suffix = "skip-a"
            "#,
        )
        .expect("valid biblint configuration");
        assert_eq!(settings.format.indent, Indent::Tab);
        assert_eq!(settings.format.align, None);
        assert_eq!(settings.format.sort, Some(vec!["key".to_string()]));
        assert_eq!(settings.format.key_generation.formula, "auth.lower + year");
        assert_eq!(
            settings.format.key_generation.collision_suffix,
            CollisionSuffixStyle::SkipA
        );
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
