//! A small, deliberately forgiving BibTeX parser.
//!
//! BibTeX in the wild contains extensions, comments, and partially written
//! entries.  The parser therefore keeps unknown material as [`Item::Raw`]
//! instead of trying to invent a meaning for it.  Known entries retain byte
//! ranges so the linter can point at the original source and the formatter can
//! safely offer a whole-document fix.

use std::fmt;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct TextRange {
    pub start: usize,
    pub end: usize,
}

impl TextRange {
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Delimiter {
    Braces,
    Parentheses,
}

impl Delimiter {
    #[must_use]
    pub const fn open(self) -> char {
        match self {
            Self::Braces => '{',
            Self::Parentheses => '(',
        }
    }

    #[must_use]
    pub const fn close(self) -> char {
        match self {
            Self::Braces => '}',
            Self::Parentheses => ')',
        }
    }
}

#[derive(Clone, Debug)]
pub struct Document {
    pub items: Vec<Item>,
    pub errors: Vec<ParseError>,
    pub source_len: usize,
}

#[derive(Clone, Debug)]
pub enum Item {
    Entry(Entry),
    Special(Special),
    Comment(Comment),
    Raw(Raw),
}

impl Item {
    #[must_use]
    pub const fn range(&self) -> TextRange {
        match self {
            Self::Entry(entry) => entry.range,
            Self::Special(special) => special.range,
            Self::Comment(comment) => comment.range,
            Self::Raw(raw) => raw.range,
        }
    }

    #[must_use]
    pub fn command_name(&self) -> Option<&str> {
        match self {
            Self::Entry(entry) => Some(&entry.command),
            Self::Special(special) => Some(&special.command),
            Self::Comment(_) | Self::Raw(_) => None,
        }
    }

    #[must_use]
    pub const fn is_entry(&self) -> bool {
        matches!(self, Self::Entry(_))
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub command: String,
    pub key: Option<String>,
    pub key_range: Option<TextRange>,
    pub fields: Vec<Field>,
    pub delimiter: Delimiter,
    pub range: TextRange,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub value: Option<Value>,
    pub range: TextRange,
}

#[derive(Clone, Debug)]
pub struct Value {
    pub parts: Vec<ValuePart>,
    pub range: TextRange,
}

impl Value {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty() || self.render().trim().is_empty()
    }

    #[must_use]
    pub fn render(&self) -> String {
        self.parts
            .iter()
            .map(ValuePart::render)
            .collect::<Vec<_>>()
            .join(" # ")
    }
}

#[derive(Clone, Debug)]
pub enum ValuePart {
    Braced(String),
    Quoted(String),
    Literal(String),
}

impl ValuePart {
    #[must_use]
    pub fn render(&self) -> &str {
        match self {
            Self::Braced(value) | Self::Quoted(value) | Self::Literal(value) => value,
        }
    }

    #[must_use]
    pub const fn is_braced(&self) -> bool {
        matches!(self, Self::Braced(_))
    }

    #[must_use]
    pub const fn is_quoted(&self) -> bool {
        matches!(self, Self::Quoted(_))
    }
}

#[derive(Clone, Debug)]
pub struct Special {
    pub command: String,
    pub body: String,
    pub delimiter: Delimiter,
    pub range: TextRange,
}

#[derive(Clone, Debug)]
pub struct Comment {
    pub text: String,
    pub range: TextRange,
}

#[derive(Clone, Debug)]
pub struct Raw {
    pub text: String,
    pub range: TextRange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub range: TextRange,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

struct Parser<'a> {
    source: &'a str,
    position: usize,
    errors: Vec<ParseError>,
}

/// Parse a BibTeX source string.
#[must_use]
pub fn parse(source: &str) -> Document {
    let mut parser = Parser {
        source,
        position: 0,
        errors: Vec::new(),
    };
    let mut items = Vec::new();

    while parser.position < source.len() {
        if source.as_bytes()[parser.position] == b'%' {
            items.push(Item::Comment(parser.parse_comment()));
        } else if parser.is_command_start(parser.position) {
            items.push(parser.parse_command());
        } else {
            items.push(Item::Raw(parser.parse_raw()));
        }
    }

    Document {
        items,
        errors: parser.errors,
        source_len: source.len(),
    }
}

impl Parser<'_> {
    fn parse_comment(&mut self) -> Comment {
        let start = self.position;
        while self.position < self.source.len() {
            let byte = self.source.as_bytes()[self.position];
            self.position += 1;
            if byte == b'\n' {
                break;
            }
        }
        let text = self.source[start..self.position]
            .trim_end_matches(['\r', '\n'])
            .to_string();
        Comment {
            text,
            range: TextRange::new(start, self.position),
        }
    }

    fn parse_raw(&mut self) -> Raw {
        let start = self.position;
        while self.position < self.source.len() {
            if self.source.as_bytes()[self.position] == b'%' || self.is_command_start(self.position)
            {
                break;
            }
            self.position = next_char_boundary(self.source, self.position);
        }
        Raw {
            text: self.source[start..self.position].to_string(),
            range: TextRange::new(start, self.position),
        }
    }

    fn is_command_start(&self, position: usize) -> bool {
        if self.source.as_bytes().get(position) != Some(&b'@') {
            return false;
        }
        self.source
            .as_bytes()
            .get(position + 1)
            .is_some_and(u8::is_ascii_alphabetic)
    }

    fn parse_command(&mut self) -> Item {
        let start = self.position;
        self.position += 1;
        let name_start = self.position;
        while self.position < self.source.len() {
            let byte = self.source.as_bytes()[self.position];
            if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' {
                self.position += 1;
            } else {
                break;
            }
        }
        let command = self.source[name_start..self.position].to_string();
        skip_ascii_whitespace(self.source, &mut self.position);
        let Some(&open_byte) = self.source.as_bytes().get(self.position) else {
            self.errors.push(ParseError {
                range: TextRange::new(start, self.source.len()),
                message: format!("@{command} command has no body"),
            });
            self.position = start + 1;
            return Item::Raw(Raw {
                text: "@".to_string(),
                range: TextRange::new(start, start + 1),
            });
        };
        let delimiter = match open_byte {
            b'{' => Delimiter::Braces,
            b'(' => Delimiter::Parentheses,
            _ => {
                self.errors.push(ParseError {
                    range: TextRange::new(start, self.position),
                    message: format!("@{command} command is not followed by '{{' or '('"),
                });
                self.position = start + 1;
                return Item::Raw(Raw {
                    text: "@".to_string(),
                    range: TextRange::new(start, start + 1),
                });
            }
        };
        let open = self.position;
        let Some(close) = scan_balanced(self.source, open, delimiter) else {
            self.errors.push(ParseError {
                range: TextRange::new(start, self.source.len()),
                message: format!("unclosed @{command} block"),
            });
            self.position = self.source.len();
            return Item::Raw(Raw {
                text: self.source[start..].to_string(),
                range: TextRange::new(start, self.source.len()),
            });
        };

        let body_start = open + 1;
        let body = &self.source[body_start..close];
        self.position = close + 1;
        if matches!(
            command.to_ascii_lowercase().as_str(),
            "comment" | "preamble" | "string"
        ) {
            return Item::Special(Special {
                command,
                body: body.to_string(),
                delimiter,
                range: TextRange::new(start, self.position),
            });
        }

        Item::Entry(parse_entry(
            &mut self.errors,
            command,
            delimiter,
            body,
            body_start,
            TextRange::new(start, self.position),
        ))
    }
}

fn parse_entry(
    errors: &mut Vec<ParseError>,
    command: String,
    delimiter: Delimiter,
    body: &str,
    body_start: usize,
    range: TextRange,
) -> Entry {
    let comma = find_top_level_byte(body, 0, b',');
    let equals = find_top_level_byte(body, 0, b'=');
    let has_key = match (comma, equals) {
        (Some(comma), Some(equals)) => comma < equals,
        (Some(_) | None, None) => !body.trim().is_empty(),
        (None, Some(_)) => false,
    };
    let key_separator = if has_key { comma } else { None };
    let key_slice = if has_key {
        key_separator.map_or(body, |separator| &body[..separator])
    } else {
        ""
    };
    let key_trimmed = key_slice.trim();
    let key = (!key_trimmed.is_empty()).then(|| key_trimmed.to_string());
    let key_range = key.as_ref().map(|_| {
        let left_trim = key_slice.len() - key_slice.trim_start().len();
        let right_trim = key_slice.trim_end().len();
        TextRange::new(body_start + left_trim, body_start + right_trim)
    });
    let mut fields = Vec::new();
    let fields_start = if has_key {
        key_separator.map_or(body.len(), |separator| separator + 1)
    } else {
        0
    };
    if fields_start < body.len() {
        parse_fields(errors, body, body_start, fields_start, &mut fields);
    }
    Entry {
        command,
        key,
        key_range,
        fields,
        delimiter,
        range,
    }
}

fn parse_fields(
    errors: &mut Vec<ParseError>,
    body: &str,
    body_start: usize,
    mut position: usize,
    fields: &mut Vec<Field>,
) {
    while position < body.len() {
        skip_ascii_whitespace(body, &mut position);
        while body.as_bytes().get(position) == Some(&b',') {
            position += 1;
            skip_ascii_whitespace(body, &mut position);
        }
        if position >= body.len() {
            break;
        }
        let field_start = position;
        let segment_end = find_top_level_byte(body, position, b',').unwrap_or(body.len());
        let equals = find_top_level_byte(body, position, b'=');
        let field_end = segment_end;
        let (name_end, value_start) = match equals {
            Some(equals) if equals < segment_end => (equals, Some(equals + 1)),
            _ => (segment_end, None),
        };
        let name_slice = &body[field_start..name_end];
        let name = name_slice.trim().to_string();
        if name.is_empty() {
            errors.push(ParseError {
                range: TextRange::new(body_start + field_start, body_start + field_end),
                message: "field has no name".to_string(),
            });
        } else {
            let value =
                value_start.map(|start| parse_value(errors, body, body_start, start, segment_end));
            fields.push(Field {
                name,
                value,
                range: TextRange::new(body_start + field_start, body_start + field_end),
            });
        }
        position = segment_end.saturating_add(1);
    }
}

fn parse_value(
    errors: &mut Vec<ParseError>,
    body: &str,
    body_start: usize,
    mut position: usize,
    end: usize,
) -> Value {
    let range_start = position;
    let mut parts = Vec::new();
    while position < end {
        skip_ascii_whitespace_until(body, &mut position, end);
        if position >= end {
            break;
        }
        if body.as_bytes()[position] == b'#' {
            position += 1;
            continue;
        }
        match body.as_bytes()[position] {
            b'{' => {
                let group_start = position;
                if let Some(group_end) = scan_group(body, position, b'{', b'}', end) {
                    parts.push(ValuePart::Braced(body[position + 1..group_end].to_string()));
                    position = group_end + 1;
                } else {
                    errors.push(ParseError {
                        range: TextRange::new(body_start + group_start, body_start + end),
                        message: "unclosed braced value".to_string(),
                    });
                    parts.push(ValuePart::Braced(body[position + 1..end].to_string()));
                    position = end;
                }
            }
            b'"' => {
                let quote_start = position;
                if let Some(quote_end) = scan_quote(body, position, end) {
                    parts.push(ValuePart::Quoted(body[position + 1..quote_end].to_string()));
                    position = quote_end + 1;
                } else {
                    errors.push(ParseError {
                        range: TextRange::new(body_start + quote_start, body_start + end),
                        message: "unclosed quoted value".to_string(),
                    });
                    parts.push(ValuePart::Quoted(body[position + 1..end].to_string()));
                    position = end;
                }
            }
            _ => {
                let literal_start = position;
                while position < end {
                    let byte = body.as_bytes()[position];
                    if byte == b'#' {
                        break;
                    }
                    position = next_char_boundary(body, position);
                }
                if literal_start < position {
                    parts.push(ValuePart::Literal(
                        body[literal_start..position].trim().to_string(),
                    ));
                } else {
                    position += 1;
                }
            }
        }
    }
    Value {
        parts,
        range: TextRange::new(body_start + range_start, body_start + end),
    }
}

fn find_top_level_byte(source: &str, start: usize, wanted: u8) -> Option<usize> {
    let mut position = start;
    let mut braces = 0usize;
    let mut parentheses = 0usize;
    let mut quoted = false;
    let mut quoted_braces = 0usize;
    while position < source.len() {
        let byte = source.as_bytes()[position];
        if quoted {
            if byte == b'{' && !is_escaped(source, position) {
                quoted_braces += 1;
            } else if byte == b'}' && !is_escaped(source, position) {
                quoted_braces = quoted_braces.saturating_sub(1);
            } else if byte == b'"' && quoted_braces == 0 && !is_escaped(source, position) {
                quoted = false;
            }
        } else if braces > 0 {
            if byte == b'{' && !is_escaped(source, position) {
                braces += 1;
            } else if byte == b'}' && !is_escaped(source, position) {
                braces = braces.saturating_sub(1);
            }
        } else {
            match byte {
                b'"' if !is_escaped(source, position) => quoted = true,
                b'{' if !is_escaped(source, position) => braces = 1,
                b'(' if !is_escaped(source, position) => parentheses += 1,
                b')' if !is_escaped(source, position) => {
                    parentheses = parentheses.saturating_sub(1);
                }
                _ if byte == wanted && parentheses == 0 => return Some(position),
                _ => {}
            }
        }
        position = next_char_boundary(source, position);
    }
    None
}

fn scan_balanced(source: &str, open: usize, delimiter: Delimiter) -> Option<usize> {
    let outer_braces = usize::from(matches!(delimiter, Delimiter::Braces));
    let mut braces = outer_braces;
    let mut parentheses = usize::from(matches!(delimiter, Delimiter::Parentheses));
    let mut quoted = false;
    let mut quoted_braces = 0usize;
    let mut position = next_char_boundary(source, open);
    while position < source.len() {
        let character = source[position..].chars().next()?;
        if quoted {
            if character == '{' && !is_escaped(source, position) {
                quoted_braces += 1;
            } else if character == '}' && !is_escaped(source, position) {
                quoted_braces = quoted_braces.saturating_sub(1);
            } else if character == '"' && quoted_braces == 0 && !is_escaped(source, position) {
                quoted = false;
            }
        } else if braces > outer_braces {
            if character == '{' && !is_escaped(source, position) {
                braces += 1;
            } else if character == '}' && !is_escaped(source, position) {
                braces = braces.saturating_sub(1);
            }
        } else {
            match character {
                '"' if !is_escaped(source, position) => quoted = true,
                '{' if !is_escaped(source, position) => braces += 1,
                '}' if matches!(delimiter, Delimiter::Braces) && !is_escaped(source, position) => {
                    braces = braces.saturating_sub(1);
                    if braces == 0 {
                        return Some(position);
                    }
                }
                '(' if matches!(delimiter, Delimiter::Parentheses)
                    && !is_escaped(source, position) =>
                {
                    parentheses += 1;
                }
                ')' if matches!(delimiter, Delimiter::Parentheses)
                    && !is_escaped(source, position) =>
                {
                    parentheses = parentheses.saturating_sub(1);
                    if parentheses == 0 {
                        return Some(position);
                    }
                }
                _ => {}
            }
        }
        position += character.len_utf8();
    }
    None
}

fn scan_group(
    source: &str,
    open: usize,
    open_char: u8,
    close_char: u8,
    end: usize,
) -> Option<usize> {
    let mut depth = 1usize;
    let mut position = open + 1;
    while position < end {
        let byte = source.as_bytes()[position];
        if byte == open_char && !is_escaped(source, position) {
            depth += 1;
        } else if byte == close_char && !is_escaped(source, position) {
            depth -= 1;
            if depth == 0 {
                return Some(position);
            }
        }
        position = next_char_boundary(source, position);
    }
    None
}

fn scan_quote(source: &str, open: usize, end: usize) -> Option<usize> {
    let mut braces = 0usize;
    let mut position = open + 1;
    while position < end {
        let byte = source.as_bytes()[position];
        if byte == b'{' && !is_escaped(source, position) {
            braces += 1;
        } else if byte == b'}' && !is_escaped(source, position) {
            if braces == 0 {
                return None;
            }
            braces -= 1;
        } else if byte == b'"' && braces == 0 && !is_escaped(source, position) {
            return Some(position);
        }
        position = next_char_boundary(source, position);
    }
    None
}

fn is_escaped(source: &str, position: usize) -> bool {
    let bytes = source.as_bytes();
    let mut backslashes = 0usize;
    let mut previous = position;
    while previous > 0 && bytes[previous - 1] == b'\\' {
        backslashes += 1;
        previous -= 1;
    }
    backslashes % 2 == 1
}

fn skip_ascii_whitespace(source: &str, position: &mut usize) {
    skip_ascii_whitespace_until(source, position, source.len());
}

fn skip_ascii_whitespace_until(source: &str, position: &mut usize, end: usize) {
    while *position < end && source.as_bytes()[*position].is_ascii_whitespace() {
        *position += 1;
    }
}

fn next_char_boundary(source: &str, position: usize) -> usize {
    position + source[position..].chars().next().map_or(1, char::len_utf8)
}

use serde::Serialize;

#[cfg(test)]
mod tests {
    use super::{Item, ValuePart, parse};

    #[test]
    fn parses_entries_and_nested_values() {
        let document = parse(
            r#"% a comment
@ARTICLE {smith,
  title = {A {nested} title},
  author = "Smith, A" # andrew,
  year = 2024,
}
"#,
        );
        assert!(document.errors.is_empty());
        assert!(matches!(document.items[0], Item::Comment(_)));
        let Item::Entry(entry) = &document.items[1] else {
            panic!("expected entry");
        };
        assert_eq!(entry.key.as_deref(), Some("smith"));
        assert_eq!(entry.fields.len(), 3);
        assert!(matches!(
            entry.fields[0].value.as_ref().unwrap().parts[0],
            ValuePart::Braced(_)
        ));
    }

    #[test]
    fn preserves_a_key_when_an_entry_has_no_fields() {
        let document = parse("@article{key}");
        assert!(document.errors.is_empty());
        let Item::Entry(entry) = &document.items[0] else {
            panic!("expected entry");
        };
        assert_eq!(entry.key.as_deref(), Some("key"));
        assert!(entry.fields.is_empty());
    }

    #[test]
    fn balances_quotes_and_braces_using_bibtex_value_rules() {
        let document = parse(
            r#"@article{key,
              title = "A {quoted {nested}} title",
              note = {A quoted "brace {group}"},
              year = 2024
            }"#,
        );
        assert!(document.errors.is_empty());
        let Item::Entry(entry) = &document.items[0] else {
            panic!("expected entry");
        };
        assert_eq!(entry.fields.len(), 3);
        assert_eq!(
            entry.fields[0].value.as_ref().unwrap().render(),
            "A {quoted {nested}} title"
        );
        assert_eq!(
            entry.fields[1].value.as_ref().unwrap().render(),
            "A quoted \"brace {group}\""
        );
    }

    #[test]
    fn balances_parenthesized_entries_around_nested_value_delimiters() {
        let document = parse(
            r#"@article(key,
              title = {A (parenthetical, title)},
              note = "A (quoted, note)",
              year = 2024
            )"#,
        );
        assert!(document.errors.is_empty());
        let Item::Entry(entry) = &document.items[0] else {
            panic!("expected entry");
        };
        assert_eq!(entry.key.as_deref(), Some("key"));
        assert_eq!(entry.fields.len(), 3);
    }

    #[test]
    fn reports_a_command_without_a_delimited_body() {
        let document = parse("@article");
        assert_eq!(document.errors.len(), 1);
        assert!(matches!(document.items[0], Item::Raw(_)));
    }

    #[test]
    fn reports_an_unbalanced_brace_inside_a_quoted_value() {
        let document = parse(r#"@article{key,title = "unbalanced } value",year=2024}"#);
        assert!(
            document
                .errors
                .iter()
                .any(|error| error.message == "unclosed quoted value")
        );
    }

    #[test]
    fn keeps_unknown_text_and_reports_unclosed_commands() {
        let document = parse("some text\n@article{broken, title = {oops\n");
        assert_eq!(document.errors.len(), 1);
        assert!(
            document
                .items
                .iter()
                .any(|item| matches!(item, Item::Raw(_)))
        );
    }

    #[test]
    fn parses_special_commands_without_treating_nested_at_as_entries() {
        let document = parse("@Comment{@article{hidden, title = {x}}}\n@string{mar = \"march\"}");
        assert_eq!(document.errors.len(), 0);
        assert!(matches!(document.items[0], Item::Special(_)));
        assert!(
            document
                .items
                .iter()
                .skip(1)
                .any(|item| matches!(item, Item::Special(_)))
        );
    }

    #[test]
    fn keeps_whitespace_inside_unbraced_values_without_inventing_concatenation() {
        let document = parse("@article{key, title = foo bar}");
        let Item::Entry(entry) = &document.items[0] else {
            panic!("expected entry");
        };
        let value = entry.fields[0].value.as_ref().unwrap();
        assert_eq!(value.render(), "foo bar");
        assert_eq!(value.parts.len(), 1);
    }

    #[test]
    fn escaped_delimiters_do_not_close_an_entry() {
        let document = parse(r#"@article{key,title={A \} B and \"quoted\"},year=2024}"#);
        assert!(document.errors.is_empty());
        let Item::Entry(entry) = &document.items[0] else {
            panic!("expected entry");
        };
        assert_eq!(entry.fields.len(), 2);
    }
}
