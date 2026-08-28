//! A deterministic, BibTeX-backed subset of Better BibTeX's citekey formula
//! language.
//!
//! Better BibTeX evaluates formulas against Zotero objects. biblint only has
//! the BibTeX record, so this module deliberately implements the parts whose
//! inputs can be recovered from a BibTeX entry. Zotero-only functions are
//! rejected during compilation instead of producing a plausible but wrong key.

use crate::key::{
    capitalize_first, clean_key_segment, creator_surname, field_value, fold_character,
    is_single_field_name, is_skip_word, split_creators, visible_text,
};
use biblint_syntax::{Entry, Value, ValuePart};
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormulaError {
    pub position: usize,
    pub message: String,
}

impl FormulaError {
    fn new(position: usize, message: impl Into<String>) -> Self {
        Self {
            position,
            message: message.into(),
        }
    }
}

impl fmt::Display for FormulaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} at character {}", self.message, self.position)
    }
}

impl std::error::Error for FormulaError {}

#[derive(Clone, Debug)]
pub(crate) struct Formula {
    patterns: Vec<Expr>,
}

#[derive(Clone, Debug)]
enum Expr {
    Atom(Atom),
    Concat(Vec<Self>),
    Alternate(Vec<Self>),
    Condition(Box<Self>, Box<Self>),
    Ternary(Box<Self>, Box<Self>, Box<Self>),
    Compare(Box<Self>, Relation, Box<Self>),
    Filter(Box<Self>, FilterCall),
}

#[derive(Clone, Debug)]
enum Atom {
    Literal(String),
    Number(i64),
    Identifier(String),
    Call {
        name: String,
        arguments: Vec<Argument>,
        position: usize,
    },
}

#[derive(Clone, Debug)]
struct FilterCall {
    name: String,
    arguments: Vec<Argument>,
    position: usize,
}

#[derive(Clone, Debug)]
struct Argument {
    name: Option<String>,
    value: ArgumentValue,
}

#[derive(Clone, Debug)]
enum ArgumentValue {
    String(String),
    Number(i64),
    Boolean(bool),
    Name(String),
}

#[derive(Clone, Copy, Debug)]
enum Relation {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Clone, Debug)]
struct EvalValue {
    text: String,
    valid: bool,
}

impl EvalValue {
    fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            valid: true,
        }
    }

    fn empty() -> Self {
        Self::text("")
    }

    fn invalid() -> Self {
        Self {
            text: String::new(),
            valid: false,
        }
    }
}

#[derive(Clone, Debug)]
struct Creator {
    surname: String,
    given: String,
}

#[derive(Clone, Debug)]
enum TokenKind {
    Identifier(String),
    Number(i64),
    String(String),
    Plus,
    Or,
    OrOr,
    AndAnd,
    Question,
    Colon,
    Dot,
    Comma,
    Semicolon,
    LParen,
    RParen,
    Equal,
    EqualEqual,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    End,
}

#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    position: usize,
}

/// Parse and validate a formula without evaluating it.
pub(crate) fn compile_formula(source: &str) -> Result<Formula, FormulaError> {
    let tokens = Lexer::new(source).lex()?;
    let mut parser = Parser::new(tokens);
    let formula = parser.parse_formula()?;
    if !matches!(parser.peek(), TokenKind::End) {
        return Err(parser.error("unexpected token after formula"));
    }
    for pattern in &formula.patterns {
        validate_expression(pattern)?;
    }
    Ok(formula)
}

pub(crate) fn validate_formula(source: &str) -> Result<(), FormulaError> {
    compile_formula(source).map(|_| ())
}

pub(crate) fn evaluate_formula(
    formula: &Formula,
    entry: &Entry,
) -> Result<Option<String>, FormulaError> {
    for pattern in &formula.patterns {
        let result = evaluate_expression(pattern, entry)?;
        if result.valid && !result.text.is_empty() {
            return Ok(Some(result.text));
        }
    }
    Ok(None)
}

struct Lexer<'a> {
    source: &'a str,
    position: usize,
}

impl<'a> Lexer<'a> {
    const fn new(source: &'a str) -> Self {
        Self {
            source,
            position: 0,
        }
    }

    fn lex(mut self) -> Result<Vec<Token>, FormulaError> {
        let mut tokens = Vec::new();
        while self.position < self.source.len() {
            self.skip_whitespace();
            if self.position >= self.source.len() {
                break;
            }
            let start = self.position;
            let character = self
                .next_character()
                .ok_or_else(|| FormulaError::new(start, "could not read formula character"))?;
            let kind = match character {
                '+' => TokenKind::Plus,
                '?' => TokenKind::Question,
                ':' => TokenKind::Colon,
                '.' => TokenKind::Dot,
                ',' => TokenKind::Comma,
                ';' => TokenKind::Semicolon,
                '(' => TokenKind::LParen,
                ')' => TokenKind::RParen,
                '|' if self.consume_if('|') => TokenKind::OrOr,
                '|' => TokenKind::Or,
                '&' if self.consume_if('&') => TokenKind::AndAnd,
                '&' => {
                    return Err(FormulaError::new(
                        start,
                        "single '&' is not valid; use '&&'",
                    ));
                }
                '=' if self.consume_if('=') => TokenKind::EqualEqual,
                '=' => TokenKind::Equal,
                '!' if self.consume_if('=') => TokenKind::NotEqual,
                '!' => {
                    return Err(FormulaError::new(
                        start,
                        "unexpected '!'; use '!=' for comparison",
                    ));
                }
                '<' if self.consume_if('=') => TokenKind::LessEqual,
                '<' => TokenKind::Less,
                '>' if self.consume_if('=') => TokenKind::GreaterEqual,
                '>' => TokenKind::Greater,
                '\'' | '"' => TokenKind::String(self.lex_string(character, start)?),
                '-' if self
                    .source
                    .get(self.position..)
                    .and_then(|rest| rest.chars().next())
                    .is_some_and(|next| next.is_ascii_digit()) =>
                {
                    TokenKind::Number(-self.lex_number(start)?)
                }
                digit if digit.is_ascii_digit() => {
                    self.position -= digit.len_utf8();
                    TokenKind::Number(self.lex_number(start)?)
                }
                identifier if is_identifier_start(identifier) => {
                    self.position -= identifier.len_utf8();
                    TokenKind::Identifier(self.lex_identifier())
                }
                _ => {
                    return Err(FormulaError::new(
                        start,
                        format!("unexpected character '{character}'"),
                    ));
                }
            };
            tokens.push(Token {
                kind,
                position: start,
            });
        }
        tokens.push(Token {
            kind: TokenKind::End,
            position: self.source.len(),
        });
        Ok(tokens)
    }

    fn skip_whitespace(&mut self) {
        while self
            .source
            .get(self.position..)
            .and_then(|rest| rest.chars().next())
            .is_some_and(char::is_whitespace)
        {
            self.position += self
                .source
                .get(self.position..)
                .and_then(|rest| rest.chars().next())
                .map_or(1, char::len_utf8);
        }
    }

    fn next_character(&mut self) -> Option<char> {
        let character = self.source.get(self.position..)?.chars().next()?;
        self.position += character.len_utf8();
        Some(character)
    }

    fn consume_if(&mut self, expected: char) -> bool {
        if self
            .source
            .get(self.position..)
            .and_then(|rest| rest.chars().next())
            == Some(expected)
        {
            self.position += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn lex_number(&mut self, start: usize) -> Result<i64, FormulaError> {
        let number_start = self.position;
        while self
            .source
            .get(self.position..)
            .and_then(|rest| rest.chars().next())
            .is_some_and(|character| character.is_ascii_digit())
        {
            self.position += 1;
        }
        self.source[number_start..self.position]
            .parse()
            .map_err(|_| FormulaError::new(start, "invalid numeric formula argument"))
    }

    fn lex_identifier(&mut self) -> String {
        let start = self.position;
        while self
            .source
            .get(self.position..)
            .and_then(|rest| rest.chars().next())
            .is_some_and(is_identifier_continue)
        {
            self.position += self
                .source
                .get(self.position..)
                .and_then(|rest| rest.chars().next())
                .map_or(1, char::len_utf8);
        }
        self.source[start..self.position].to_string()
    }

    fn lex_string(&mut self, quote: char, start: usize) -> Result<String, FormulaError> {
        let mut value = String::new();
        while let Some(character) = self.next_character() {
            if character == quote {
                return Ok(value);
            }
            if character == '\\' {
                let escaped = self.next_character().ok_or_else(|| {
                    FormulaError::new(start, "unterminated quoted formula string")
                })?;
                value.push(match escaped {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    other => other,
                });
            } else {
                value.push(character);
            }
        }
        Err(FormulaError::new(
            start,
            "unterminated quoted formula string",
        ))
    }
}

fn is_identifier_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_'
}

fn is_identifier_continue(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    const fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    fn parse_formula(&mut self) -> Result<Formula, FormulaError> {
        if matches!(self.peek(), TokenKind::End) {
            return Err(self.error("formula cannot be empty"));
        }
        let mut patterns = Vec::new();
        loop {
            patterns.push(self.parse_expression()?);
            if self.consume_if(|kind| matches!(kind, TokenKind::Or | TokenKind::Semicolon)) {
                if matches!(self.peek(), TokenKind::End) {
                    return Err(self.error("formula cannot end with an alternate separator"));
                }
            } else {
                break;
            }
        }
        Ok(Formula { patterns })
    }

    fn parse_expression(&mut self) -> Result<Expr, FormulaError> {
        self.parse_ternary()
    }

    fn parse_ternary(&mut self) -> Result<Expr, FormulaError> {
        let condition = self.parse_or()?;
        if self.consume_if(|kind| matches!(kind, TokenKind::Question)) {
            let if_true = self.parse_expression()?;
            self.expect(
                |kind| matches!(kind, TokenKind::Colon),
                "expected ':' in ternary",
            )?;
            let if_false = self.parse_ternary()?;
            Ok(Expr::Ternary(
                Box::new(condition),
                Box::new(if_true),
                Box::new(if_false),
            ))
        } else {
            Ok(condition)
        }
    }

    fn parse_or(&mut self) -> Result<Expr, FormulaError> {
        let mut expressions = vec![self.parse_and()?];
        while self.consume_if(|kind| matches!(kind, TokenKind::OrOr)) {
            expressions.push(self.parse_and()?);
        }
        if expressions.len() == 1 {
            Ok(expressions.remove(0))
        } else {
            Ok(Expr::Alternate(expressions))
        }
    }

    fn parse_and(&mut self) -> Result<Expr, FormulaError> {
        let mut expression = self.parse_compare()?;
        while self.consume_if(|kind| matches!(kind, TokenKind::AndAnd)) {
            let right = self.parse_compare()?;
            expression = Expr::Condition(Box::new(expression), Box::new(right));
        }
        Ok(expression)
    }

    fn parse_compare(&mut self) -> Result<Expr, FormulaError> {
        let left = self.parse_concat()?;
        let relation = match self.peek() {
            TokenKind::EqualEqual => Some(Relation::Equal),
            TokenKind::NotEqual => Some(Relation::NotEqual),
            TokenKind::Less => Some(Relation::Less),
            TokenKind::LessEqual => Some(Relation::LessEqual),
            TokenKind::Greater => Some(Relation::Greater),
            TokenKind::GreaterEqual => Some(Relation::GreaterEqual),
            _ => None,
        };
        let Some(relation) = relation else {
            return Ok(left);
        };
        self.position += 1;
        let right = self.parse_concat()?;
        Ok(Expr::Compare(Box::new(left), relation, Box::new(right)))
    }

    fn parse_concat(&mut self) -> Result<Expr, FormulaError> {
        let mut expressions = vec![self.parse_postfix()?];
        while self.consume_if(|kind| matches!(kind, TokenKind::Plus)) {
            expressions.push(self.parse_postfix()?);
        }
        if expressions.len() == 1 {
            Ok(expressions.remove(0))
        } else {
            Ok(Expr::Concat(expressions))
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, FormulaError> {
        let mut expression = self.parse_primary()?;
        while self.consume_if(|kind| matches!(kind, TokenKind::Dot)) {
            let token = self.take();
            let TokenKind::Identifier(name) = token.kind else {
                return Err(FormulaError::new(token.position, "expected a filter name"));
            };
            let arguments = if self.consume_if(|kind| matches!(kind, TokenKind::LParen)) {
                self.parse_arguments()?
            } else {
                Vec::new()
            };
            expression = Expr::Filter(
                Box::new(expression),
                FilterCall {
                    name,
                    arguments,
                    position: token.position,
                },
            );
        }
        Ok(expression)
    }

    fn parse_primary(&mut self) -> Result<Expr, FormulaError> {
        let token = self.take();
        match token.kind {
            TokenKind::String(value) => Ok(Expr::Atom(Atom::Literal(value))),
            TokenKind::Number(value) => Ok(Expr::Atom(Atom::Number(value))),
            TokenKind::Identifier(name) => {
                if self.consume_if(|kind| matches!(kind, TokenKind::LParen)) {
                    Ok(Expr::Atom(Atom::Call {
                        name,
                        arguments: self.parse_arguments()?,
                        position: token.position,
                    }))
                } else {
                    Ok(Expr::Atom(Atom::Identifier(name)))
                }
            }
            TokenKind::LParen => {
                let expression = self.parse_expression()?;
                self.expect(
                    |kind| matches!(kind, TokenKind::RParen),
                    "expected ')' to close formula group",
                )?;
                Ok(expression)
            }
            _ => Err(FormulaError::new(
                token.position,
                "expected a formula value",
            )),
        }
    }

    fn parse_arguments(&mut self) -> Result<Vec<Argument>, FormulaError> {
        let mut arguments = Vec::new();
        if self.consume_if(|kind| matches!(kind, TokenKind::RParen)) {
            return Ok(arguments);
        }
        loop {
            let name = if let TokenKind::Identifier(name) = self.peek().clone() {
                if self
                    .tokens
                    .get(self.position + 1)
                    .is_some_and(|token| matches!(token.kind, TokenKind::Equal))
                {
                    self.position += 2;
                    Some(name)
                } else {
                    None
                }
            } else {
                None
            };
            let token = self.take();
            let value = match token.kind {
                TokenKind::String(value) => ArgumentValue::String(value),
                TokenKind::Number(value) => ArgumentValue::Number(value),
                TokenKind::Identifier(value) if value.eq_ignore_ascii_case("true") => {
                    ArgumentValue::Boolean(true)
                }
                TokenKind::Identifier(value) if value.eq_ignore_ascii_case("false") => {
                    ArgumentValue::Boolean(false)
                }
                TokenKind::Identifier(value) => ArgumentValue::Name(value),
                _ => {
                    return Err(FormulaError::new(
                        token.position,
                        "formula arguments must be strings, numbers, booleans, or names",
                    ));
                }
            };
            arguments.push(Argument { name, value });
            if self.consume_if(|kind| matches!(kind, TokenKind::RParen)) {
                break;
            }
            self.expect(
                |kind| matches!(kind, TokenKind::Comma),
                "expected ',' between arguments",
            )?;
        }
        Ok(arguments)
    }

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.position].kind
    }

    fn take(&mut self) -> Token {
        let token = self.tokens[self.position].clone();
        self.position += 1;
        token
    }

    fn consume_if(&mut self, predicate: impl FnOnce(&TokenKind) -> bool) -> bool {
        if predicate(self.peek()) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expect(
        &mut self,
        predicate: impl FnOnce(&TokenKind) -> bool,
        message: &str,
    ) -> Result<(), FormulaError> {
        if self.consume_if(predicate) {
            Ok(())
        } else {
            Err(self.error(message))
        }
    }

    fn error(&self, message: &str) -> FormulaError {
        FormulaError::new(self.tokens[self.position].position, message)
    }
}

fn validate_expression(expression: &Expr) -> Result<(), FormulaError> {
    match expression {
        Expr::Atom(Atom::Call {
            name,
            position,
            arguments,
        }) => {
            if !supported_function(name) {
                return Err(FormulaError::new(
                    *position,
                    format!(
                        "Better BibTeX function '{name}' needs Zotero metadata or is not supported by biblint"
                    ),
                ));
            }
            validate_arguments(name, arguments, *position)?;
        }
        Expr::Atom(Atom::Identifier(_) | Atom::Literal(_) | Atom::Number(_)) => {}
        Expr::Concat(expressions) | Expr::Alternate(expressions) => {
            for expression in expressions {
                validate_expression(expression)?;
            }
        }
        Expr::Condition(left, right) | Expr::Compare(left, _, right) => {
            validate_expression(left)?;
            validate_expression(right)?;
        }
        Expr::Ternary(condition, if_true, if_false) => {
            validate_expression(condition)?;
            validate_expression(if_true)?;
            validate_expression(if_false)?;
        }
        Expr::Filter(expression, filter) => {
            validate_expression(expression)?;
            if !supported_filter(&filter.name) {
                return Err(FormulaError::new(
                    filter.position,
                    format!(
                        "Better BibTeX filter '.{}' is not supported by biblint",
                        filter.name
                    ),
                ));
            }
            validate_arguments(&filter.name, &filter.arguments, filter.position)?;
        }
    }
    Ok(())
}

fn supported_function(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "auth"
            | "authauthea"
            | "authetal"
            | "authetal2"
            | "authforeini"
            | "authini"
            | "authorini"
            | "authorlast"
            | "authors"
            | "authorsalpha"
            | "authorsn"
            | "authshort"
            | "date"
            | "extra"
            | "firstpage"
            | "journal"
            | "language"
            | "lastpage"
            | "month"
            | "origdate"
            | "origyear"
            | "shorttitle"
            | "shortyear"
            | "title"
            | "transliterate"
            | "type"
            | "veryshorttitle"
            | "year"
    )
}

fn supported_filter(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "abbr"
            | "alphanum"
            | "ascii"
            | "capitalize"
            | "clean"
            | "condense"
            | "default"
            | "discard"
            | "len"
            | "lower"
            | "nopunct"
            | "nopunctordash"
            | "numeric"
            | "postfix"
            | "prefix"
            | "replace"
            | "select"
            | "skipwords"
            | "substring"
            | "transliterate"
            | "upper"
    )
}

fn validate_arguments(
    name: &str,
    arguments: &[Argument],
    position: usize,
) -> Result<(), FormulaError> {
    let lower = name.to_ascii_lowercase();
    let (max, allowed_names): (usize, &[&str]) = match lower.as_str() {
        "auth" => (4, &["n", "m", "creator", "initials"]),
        "authauthea" | "authetal" | "authetal2" | "authorini" | "authorsalpha" | "authshort" => {
            (3, &["creator", "initials", "sep"])
        }
        "authforeini" => (1, &["creator"]),
        "authini" | "authors" | "authorsn" => (4, &["n", "creator", "initials", "sep"]),
        "authorlast" => (2, &["creator", "initials"]),
        "extra" => (1, &["variable"]),
        "language" | "type" => (8, &[]),
        "shorttitle" | "veryshorttitle" => (2, &["n", "m"]),
        "abbr" => (1, &["chars"]),
        "condense" => (1, &["sep"]),
        "default" => (1, &["text"]),
        "len" => (2, &["relation", "length"]),
        "nopunct" => (1, &["dash"]),
        "postfix" => (1, &["postfix"]),
        "prefix" => (1, &["prefix"]),
        "replace" => (2, &["find", "replace"]),
        "select" | "substring" => (2, &["start", "n"]),
        "skipwords" => (1, &["nopunct"]),
        _ => (0, &[]),
    };
    if arguments.len() > max {
        return Err(FormulaError::new(
            position,
            format!("formula item '{name}' has too many arguments"),
        ));
    }
    for argument in arguments {
        if let Some(argument_name) = &argument.name
            && !allowed_names
                .iter()
                .any(|allowed| argument_name.eq_ignore_ascii_case(allowed))
        {
            return Err(FormulaError::new(
                position,
                format!("formula item '{name}' has an unknown named argument '{argument_name}'"),
            ));
        }
    }
    Ok(())
}

fn evaluate_expression(expression: &Expr, entry: &Entry) -> Result<EvalValue, FormulaError> {
    match expression {
        Expr::Atom(atom) => evaluate_atom(atom, entry),
        Expr::Concat(expressions) => {
            let mut output = String::new();
            for expression in expressions {
                let value = evaluate_expression(expression, entry)?;
                if !value.valid {
                    return Ok(EvalValue::invalid());
                }
                output.push_str(&value.text);
            }
            Ok(EvalValue::text(output))
        }
        Expr::Alternate(expressions) => {
            for expression in expressions {
                let value = evaluate_expression(expression, entry)?;
                if value.valid && !value.text.is_empty() {
                    return Ok(value);
                }
            }
            Ok(EvalValue::empty())
        }
        Expr::Condition(left, right) => {
            let condition = evaluate_expression(left, entry)?;
            if condition.valid && !condition.text.is_empty() {
                evaluate_expression(right, entry)
            } else {
                Ok(EvalValue::empty())
            }
        }
        Expr::Ternary(condition, if_true, if_false) => {
            let condition = evaluate_expression(condition, entry)?;
            if condition.valid && !condition.text.is_empty() {
                evaluate_expression(if_true, entry)
            } else {
                evaluate_expression(if_false, entry)
            }
        }
        Expr::Compare(left, relation, right) => {
            let left = evaluate_expression(left, entry)?;
            let right = evaluate_expression(right, entry)?;
            if !left.valid || !right.valid {
                return Ok(EvalValue::invalid());
            }
            let length = i64::try_from(left.text.chars().count()).unwrap_or(i64::MAX);
            let comparison = right.text.trim().parse::<i64>().map_err(|_| {
                FormulaError::new(
                    0,
                    "formula length comparisons require a numeric right-hand side",
                )
            })?;
            if relation_matches(*relation, length, comparison) {
                Ok(left)
            } else {
                Ok(EvalValue::invalid())
            }
        }
        Expr::Filter(expression, filter) => {
            let value = evaluate_expression(expression, entry)?;
            if !value.valid {
                return Ok(value);
            }
            evaluate_filter(value, filter)
        }
    }
}

fn relation_matches(relation: Relation, left: i64, right: i64) -> bool {
    match relation {
        Relation::Equal => left == right,
        Relation::NotEqual => left != right,
        Relation::Less => left < right,
        Relation::LessEqual => left <= right,
        Relation::Greater => left > right,
        Relation::GreaterEqual => left >= right,
    }
}

fn evaluate_atom(atom: &Atom, entry: &Entry) -> Result<EvalValue, FormulaError> {
    match atom {
        Atom::Literal(value) => Ok(EvalValue::text(value.clone())),
        Atom::Number(value) => Ok(EvalValue::text(value.to_string())),
        Atom::Identifier(name) => evaluate_name(name, &[], entry),
        Atom::Call {
            name, arguments, ..
        } => evaluate_name(name, arguments, entry),
    }
}

fn evaluate_name(
    name: &str,
    arguments: &[Argument],
    entry: &Entry,
) -> Result<EvalValue, FormulaError> {
    let lower = name.to_ascii_lowercase();
    if !arguments.is_empty() || supported_function(&lower) {
        return evaluate_function(&lower, arguments, entry);
    }
    Ok(EvalValue::text(field_text(entry, name)))
}

#[allow(clippy::too_many_lines)]
fn evaluate_function(
    name: &str,
    arguments: &[Argument],
    entry: &Entry,
) -> Result<EvalValue, FormulaError> {
    match name {
        "auth" => evaluate_auth(arguments, entry),
        "authauthea" => evaluate_auth_multi(arguments, entry, AuthMulti::AuthAuthEa),
        "authetal" => evaluate_auth_multi(arguments, entry, AuthMulti::AuthEtAl),
        "authetal2" => evaluate_auth_multi(arguments, entry, AuthMulti::AuthEtal2),
        "authforeini" => {
            let creator = string_argument(arguments, 0, "creator")?.unwrap_or_else(|| "*".into());
            let creators = creators(entry, &creator);
            Ok(EvalValue::text(
                creators
                    .first()
                    .map_or_else(String::new, |creator| first_initial(&creator.given)),
            ))
        }
        "authini" => evaluate_auth_ini(arguments, entry),
        "authorini" => evaluate_author_ini(arguments, entry),
        "authorlast" => {
            let creator = string_argument(arguments, 0, "creator")?.unwrap_or_else(|| "*".into());
            let initials = boolean_argument(arguments, 1, "initials")?.unwrap_or(false);
            let output = creators(entry, &creator)
                .last()
                .map_or_else(String::new, |creator| format_creator(creator, initials));
            Ok(EvalValue::text(output))
        }
        "authors" | "authorsn" => evaluate_authors(arguments, entry),
        "authorsalpha" => evaluate_authors_alpha(arguments, entry),
        "authshort" => evaluate_auth_short(arguments, entry),
        "date" => Ok(EvalValue::text(field_text(entry, "date"))),
        "extra" => {
            let key = string_argument(arguments, 0, "variable")?.ok_or_else(|| {
                FormulaError::new(0, "extra() requires the name of an extra-field variable")
            })?;
            Ok(EvalValue::text(extra_value(entry, &key)))
        }
        "firstpage" => Ok(EvalValue::text(page_value(entry, true))),
        "journal" => Ok(EvalValue::text(field_text(entry, "journal"))),
        "language" => {
            let language = field_text(entry, "language");
            if arguments.is_empty() {
                Ok(EvalValue::text(language))
            } else {
                let matches = arguments.iter().any(|argument| {
                    argument_string(argument)
                        .is_some_and(|wanted| language.eq_ignore_ascii_case(wanted))
                });
                Ok(if matches {
                    EvalValue::text("")
                } else {
                    EvalValue::invalid()
                })
            }
        }
        "lastpage" => Ok(EvalValue::text(page_value(entry, false))),
        "month" => Ok(EvalValue::text(field_text(entry, "month"))),
        "origdate" => Ok(EvalValue::text(field_text(entry, "origdate"))),
        "origyear" => Ok(EvalValue::text(field_text(entry, "origyear"))),
        "shorttitle" => evaluate_title(arguments, entry, 3),
        "shortyear" => {
            let year = year_value(entry);
            Ok(EvalValue::text(
                year.get(year.len().saturating_sub(2)..).unwrap_or(""),
            ))
        }
        "title" => Ok(EvalValue::text(title_value(entry, None, usize::MAX))),
        "transliterate" => Ok(EvalValue::empty()),
        "type" => {
            let item_type = entry.command.to_ascii_lowercase();
            if arguments.is_empty() {
                Ok(EvalValue::text(item_type))
            } else {
                let matches = arguments.iter().any(|argument| {
                    argument_string(argument)
                        .is_some_and(|allowed| bibtex_type_matches(&item_type, allowed))
                });
                Ok(if matches {
                    EvalValue::text("")
                } else {
                    EvalValue::invalid()
                })
            }
        }
        "veryshorttitle" => evaluate_title(arguments, entry, 1),
        "year" => Ok(EvalValue::text(year_value(entry))),
        _ => Err(FormulaError::new(
            0,
            format!("unsupported Better BibTeX function '{name}'"),
        )),
    }
}

fn evaluate_auth(arguments: &[Argument], entry: &Entry) -> Result<EvalValue, FormulaError> {
    let n = number_argument(arguments, 0, "n")?.unwrap_or(0);
    let m = number_argument(arguments, 1, "m")?.unwrap_or(1);
    let creator = string_argument(arguments, 2, "creator")?.unwrap_or_else(|| "*".into());
    let initials = boolean_argument(arguments, 3, "initials")?.unwrap_or(false);
    if n < 0 || m < 1 {
        return Err(FormulaError::new(
            0,
            "auth() character and creator indexes must be positive",
        ));
    }
    let index = usize::try_from(m - 1)
        .map_err(|_| FormulaError::new(0, "auth() creator index is too large"))?;
    let Some(creator) = creators(entry, &creator).get(index).cloned() else {
        return Ok(EvalValue::empty());
    };
    let surname = clean_key_segment(&creator.surname);
    let surname = take_characters(&surname, n);
    let suffix = if initials {
        clean_key_segment(&creator.given_initials())
    } else {
        String::new()
    };
    Ok(EvalValue::text(format!("{surname}{suffix}")))
}

#[derive(Clone, Copy)]
enum AuthMulti {
    AuthAuthEa,
    AuthEtAl,
    AuthEtal2,
}

fn evaluate_auth_multi(
    arguments: &[Argument],
    entry: &Entry,
    mode: AuthMulti,
) -> Result<EvalValue, FormulaError> {
    let creator = string_argument(arguments, 0, "creator")?.unwrap_or_else(|| "*".into());
    let initials = boolean_argument(arguments, 1, "initials")?.unwrap_or(false);
    let default_separator = match mode {
        AuthMulti::AuthEtAl => " ",
        AuthMulti::AuthAuthEa | AuthMulti::AuthEtal2 => ".",
    };
    let separator =
        string_argument(arguments, 2, "sep")?.unwrap_or_else(|| default_separator.to_string());
    let people = creators(entry, &creator);
    let names = people
        .iter()
        .take(2)
        .map(|person| format_creator(person, initials))
        .collect::<Vec<_>>();
    if names.is_empty() {
        return Ok(EvalValue::empty());
    }
    let mut output = names.join(&separator);
    if people.len() > 2 {
        output.push_str(&separator);
        output.push_str(match mode {
            AuthMulti::AuthAuthEa => "ea",
            AuthMulti::AuthEtAl => "EtAl",
            AuthMulti::AuthEtal2 => "etal",
        });
    }
    Ok(EvalValue::text(output))
}

fn evaluate_auth_ini(arguments: &[Argument], entry: &Entry) -> Result<EvalValue, FormulaError> {
    let n = number_argument(arguments, 0, "n")?.unwrap_or(0);
    let creator = string_argument(arguments, 1, "creator")?.unwrap_or_else(|| "*".into());
    let initials = boolean_argument(arguments, 2, "initials")?.unwrap_or(false);
    let separator = string_argument(arguments, 3, "sep")?.unwrap_or_else(|| ".".into());
    if n < 0 {
        return Err(FormulaError::new(
            0,
            "authIni() character count cannot be negative",
        ));
    }
    let output = creators(entry, &creator)
        .iter()
        .map(|person| {
            let mut name = take_characters(&clean_key_segment(&person.surname), n);
            if initials {
                name.push_str(&clean_key_segment(&person.given_initials()));
            }
            name
        })
        .collect::<Vec<_>>()
        .join(&separator);
    Ok(EvalValue::text(output))
}

fn evaluate_author_ini(arguments: &[Argument], entry: &Entry) -> Result<EvalValue, FormulaError> {
    let creator = string_argument(arguments, 0, "creator")?.unwrap_or_else(|| "*".into());
    let initials = boolean_argument(arguments, 1, "initials")?.unwrap_or(false);
    let separator = string_argument(arguments, 2, "sep")?.unwrap_or_else(|| ".".into());
    let people = creators(entry, &creator);
    let output = people
        .iter()
        .enumerate()
        .map(|(index, person)| {
            let length = if index == 0 { 5 } else { 1 };
            let mut name = take_characters(&clean_key_segment(&person.surname), length);
            if initials {
                name.push_str(&clean_key_segment(&person.given_initials()));
            }
            name
        })
        .collect::<Vec<_>>()
        .join(&separator);
    Ok(EvalValue::text(output))
}

fn evaluate_authors(arguments: &[Argument], entry: &Entry) -> Result<EvalValue, FormulaError> {
    let n = number_argument(arguments, 0, "n")?.unwrap_or(0);
    let creator = string_argument(arguments, 1, "creator")?.unwrap_or_else(|| "*".into());
    let initials = boolean_argument(arguments, 2, "initials")?.unwrap_or(false);
    let separator = string_argument(arguments, 3, "sep")?.unwrap_or_else(|| " ".into());
    if n < 0 {
        return Err(FormulaError::new(0, "authorsn() count cannot be negative"));
    }
    let people = creators(entry, &creator);
    let limit = if n == 0 {
        people.len()
    } else {
        usize::try_from(n).map_err(|_| FormulaError::new(0, "authorsn() count is too large"))?
    };
    Ok(EvalValue::text(
        people
            .iter()
            .take(limit)
            .map(|person| format_creator(person, initials))
            .collect::<Vec<_>>()
            .join(&separator),
    ))
}

fn evaluate_authors_alpha(
    arguments: &[Argument],
    entry: &Entry,
) -> Result<EvalValue, FormulaError> {
    let creator = string_argument(arguments, 0, "creator")?.unwrap_or_else(|| "*".into());
    let initials = boolean_argument(arguments, 1, "initials")?.unwrap_or(false);
    let separator = string_argument(arguments, 2, "sep")?.unwrap_or_else(|| " ".into());
    let people = creators(entry, &creator);
    let output = match people.len() {
        0 => String::new(),
        1 => take_characters(&clean_key_segment(&people[0].surname), 3),
        2..=4 => people
            .iter()
            .map(|person| {
                if initials {
                    format_creator(person, true)
                } else {
                    first_initial(&person.surname)
                }
            })
            .collect::<Vec<_>>()
            .join(&separator),
        _ => {
            let mut result = people
                .iter()
                .take(3)
                .map(|person| first_initial(&person.surname))
                .collect::<Vec<_>>()
                .join(&separator);
            result.push('+');
            result
        }
    };
    Ok(EvalValue::text(output))
}

fn evaluate_auth_short(arguments: &[Argument], entry: &Entry) -> Result<EvalValue, FormulaError> {
    let creator = string_argument(arguments, 0, "creator")?.unwrap_or_else(|| "*".into());
    let initials = boolean_argument(arguments, 1, "initials")?.unwrap_or(false);
    let separator = string_argument(arguments, 2, "sep")?.unwrap_or_else(|| ".".into());
    let people = creators(entry, &creator);
    let output = if people.len() <= 1 {
        people
            .first()
            .map_or_else(String::new, |person| format_creator(person, initials))
    } else {
        let mut result = people
            .iter()
            .take(3)
            .map(|person| first_initial(&person.surname))
            .collect::<Vec<_>>()
            .join(&separator);
        if people.len() > 3 {
            result.push('+');
        }
        result
    };
    Ok(EvalValue::text(output))
}

fn evaluate_title(
    arguments: &[Argument],
    entry: &Entry,
    default_words: i64,
) -> Result<EvalValue, FormulaError> {
    let words = number_argument(arguments, 0, "n")?.unwrap_or(default_words);
    let capitalize = number_argument(arguments, 1, "m")?.unwrap_or(0);
    if words < 0 || capitalize < 0 {
        return Err(FormulaError::new(0, "title word counts cannot be negative"));
    }
    let words = usize::try_from(words)
        .map_err(|_| FormulaError::new(0, "title word count is too large"))?;
    let capitalize = usize::try_from(capitalize)
        .map_err(|_| FormulaError::new(0, "title capitalization count is too large"))?;
    Ok(EvalValue::text(title_value(entry, Some(words), capitalize)))
}

#[allow(clippy::too_many_lines)]
fn evaluate_filter(value: EvalValue, filter: &FilterCall) -> Result<EvalValue, FormulaError> {
    let name = filter.name.to_ascii_lowercase();
    match name.as_str() {
        "abbr" => {
            let chars = number_argument(&filter.arguments, 0, "chars")?.unwrap_or(1);
            if chars < 1 {
                return Err(FormulaError::new(
                    filter.position,
                    "abbr() requires a positive character count",
                ));
            }
            Ok(EvalValue::text(
                value
                    .text
                    .split_whitespace()
                    .map(|word| take_characters(word, chars))
                    .collect::<Vec<_>>()
                    .join(" "),
            ))
        }
        "alphanum" => Ok(EvalValue::text(
            value
                .text
                .chars()
                .filter(|character| character.is_alphanumeric())
                .collect::<String>(),
        )),
        "ascii" => Ok(EvalValue::text(
            value
                .text
                .chars()
                .filter(char::is_ascii)
                .collect::<String>(),
        )),
        "capitalize" => Ok(EvalValue::text(capitalize_words(&value.text))),
        "clean" => Ok(EvalValue::text(clean_key_segment(&value.text))),
        "condense" => {
            let separator = string_argument(&filter.arguments, 0, "sep")?.unwrap_or_default();
            Ok(EvalValue::text(
                value
                    .text
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(&separator),
            ))
        }
        "default" => {
            let default = string_argument(&filter.arguments, 0, "text")?.ok_or_else(|| {
                FormulaError::new(filter.position, "default() requires a fallback string")
            })?;
            Ok(if value.text.is_empty() {
                EvalValue::text(default)
            } else {
                value
            })
        }
        "discard" => Ok(EvalValue::empty()),
        "len" => {
            if filter.arguments.is_empty() {
                return Ok(if value.text.is_empty() {
                    EvalValue::invalid()
                } else {
                    value
                });
            }
            let relation =
                string_argument(&filter.arguments, 0, "relation")?.unwrap_or_else(|| ">".into());
            let length = number_argument(&filter.arguments, 1, "length")?.unwrap_or(0);
            let relation = parse_relation(&relation).ok_or_else(|| {
                FormulaError::new(filter.position, "len() has an invalid comparison relation")
            })?;
            if length < 0 {
                return Err(FormulaError::new(
                    filter.position,
                    "len() length cannot be negative",
                ));
            }
            let actual_length = i64::try_from(value.text.chars().count()).unwrap_or(i64::MAX);
            if relation_matches(relation, actual_length, length) {
                Ok(value)
            } else {
                Ok(EvalValue::invalid())
            }
        }
        "lower" => Ok(EvalValue::text(value.text.to_lowercase())),
        "upper" => Ok(EvalValue::text(value.text.to_uppercase())),
        "nopunct" => {
            let dash = string_argument(&filter.arguments, 0, "dash")?.unwrap_or_else(|| "-".into());
            Ok(EvalValue::text(remove_punctuation(&value.text, &dash)))
        }
        "nopunctordash" => Ok(EvalValue::text(remove_punctuation(&value.text, ""))),
        "numeric" => Ok(if value.text.trim().parse::<i64>().is_ok() {
            value
        } else {
            EvalValue::invalid()
        }),
        "postfix" => {
            let suffix = string_argument(&filter.arguments, 0, "postfix")?
                .ok_or_else(|| FormulaError::new(filter.position, "postfix() requires a suffix"))?;
            Ok(if value.text.is_empty() {
                value
            } else {
                EvalValue::text(format!("{}{suffix}", value.text))
            })
        }
        "prefix" => {
            let prefix = string_argument(&filter.arguments, 0, "prefix")?
                .ok_or_else(|| FormulaError::new(filter.position, "prefix() requires a prefix"))?;
            Ok(if value.text.is_empty() {
                value
            } else {
                EvalValue::text(format!("{prefix}{}", value.text))
            })
        }
        "replace" => {
            let find = string_argument(&filter.arguments, 0, "find")?.ok_or_else(|| {
                FormulaError::new(filter.position, "replace() requires a search string")
            })?;
            let replacement =
                string_argument(&filter.arguments, 1, "replace")?.ok_or_else(|| {
                    FormulaError::new(filter.position, "replace() requires a replacement string")
                })?;
            Ok(EvalValue::text(value.text.replace(&find, &replacement)))
        }
        "select" => {
            let start = number_argument(&filter.arguments, 0, "start")?.unwrap_or(1);
            let count = number_argument(&filter.arguments, 1, "n")?;
            Ok(EvalValue::text(select_words(&value.text, start, count)?))
        }
        "skipwords" => {
            let nopunct = boolean_argument(&filter.arguments, 0, "nopunct")?.unwrap_or(false);
            let source = if nopunct {
                remove_punctuation(&value.text, "")
            } else {
                value.text.clone()
            };
            Ok(EvalValue::text(
                source
                    .split_whitespace()
                    .filter(|word| !is_skip_word(word))
                    .collect::<Vec<_>>()
                    .join(" "),
            ))
        }
        "substring" => {
            let start = number_argument(&filter.arguments, 0, "start")?.unwrap_or(1);
            let count = number_argument(&filter.arguments, 1, "n")?;
            Ok(EvalValue::text(select_characters(
                &value.text,
                start,
                count,
            )?))
        }
        "transliterate" => Ok(EvalValue::text(transliterate_text(&value.text))),
        _ => Err(FormulaError::new(
            filter.position,
            format!("unsupported Better BibTeX filter '.{}'", filter.name),
        )),
    }
}

fn parse_relation(value: &str) -> Option<Relation> {
    match value.trim() {
        "==" => Some(Relation::Equal),
        "!=" => Some(Relation::NotEqual),
        "<" => Some(Relation::Less),
        "<=" => Some(Relation::LessEqual),
        ">" => Some(Relation::Greater),
        ">=" => Some(Relation::GreaterEqual),
        _ => None,
    }
}

fn creators(entry: &Entry, selector: &str) -> Vec<Creator> {
    let selector = selector.to_ascii_lowercase();
    let fields: Vec<&str> = if selector == "*" {
        vec!["author", "editor", "translator", "collaborator"]
    } else {
        match selector.as_str() {
            "author" | "editor" | "translator" | "collaborator" => vec![selector.as_str()],
            _ => return Vec::new(),
        }
    };
    let Some(value) = fields.iter().find_map(|name| field_value(entry, name)) else {
        return Vec::new();
    };
    let rendered = render_value(value);
    split_creators(&rendered)
        .into_iter()
        .filter(|raw| !raw.trim().is_empty())
        .map(|raw| {
            let single_field = is_single_field_name(&raw);
            let surname = creator_surname(&raw, single_field);
            let visible = visible_text(&raw);
            let given = given_name(&visible, &surname);
            Creator { surname, given }
        })
        .collect()
}

impl Creator {
    fn given_initials(&self) -> String {
        self.given
            .split_whitespace()
            .filter_map(|word| word.chars().find(|character| character.is_alphabetic()))
            .map(|character| character.to_uppercase().collect::<String>())
            .collect()
    }
}

fn given_name(visible: &str, surname: &str) -> String {
    if let Some((_, given)) = visible.split_once(',') {
        return given.trim().to_string();
    }
    let visible_words = visible.split_whitespace().collect::<Vec<_>>();
    let surname_words = surname.split_whitespace().collect::<Vec<_>>();
    if visible_words.len() <= surname_words.len() {
        return String::new();
    }
    visible_words[..visible_words.len() - surname_words.len()].join(" ")
}

fn format_creator(creator: &Creator, initials: bool) -> String {
    let surname = clean_key_segment(&creator.surname);
    if initials {
        format!("{surname}{}", clean_key_segment(&creator.given_initials()))
    } else {
        surname
    }
}

fn first_initial(value: &str) -> String {
    value
        .chars()
        .find(|character| character.is_alphabetic())
        .map_or_else(String::new, |character| character.to_uppercase().collect())
}

fn field_text(entry: &Entry, name: &str) -> String {
    field_value(entry, name).map_or_else(String::new, render_value)
}

fn render_value(value: &Value) -> String {
    value
        .parts
        .iter()
        .map(ValuePart::render)
        .collect::<Vec<_>>()
        .join("")
}

fn year_value(entry: &Entry) -> String {
    let visible = visible_text(&field_text(entry, "year"));
    let digits = visible
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>();
    digits.get(..4.min(digits.len())).unwrap_or("").to_string()
}

fn title_value(entry: &Entry, limit: Option<usize>, capitalize: usize) -> String {
    let mut words = Vec::new();
    for raw in visible_text(&field_text(entry, "title")).split_whitespace() {
        let word = clean_key_segment(raw);
        if word.is_empty() || is_skip_word(&word) {
            continue;
        }
        let index = words.len();
        let word = if index < capitalize {
            capitalize_first(&word)
        } else {
            word
        };
        words.push(word);
        if limit.is_some_and(|limit| words.len() >= limit) {
            break;
        }
    }
    words.join("")
}

fn extra_value(entry: &Entry, wanted: &str) -> String {
    let wanted = normalize_extra_key(wanted);
    for line in field_text(entry, "extra").lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if normalize_extra_key(key) == wanted {
            return value.trim().to_string();
        }
    }
    String::new()
}

fn normalize_extra_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn page_value(entry: &Entry, first: bool) -> String {
    let numbers = field_text(entry, "pages")
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u64>().ok())
        .collect::<Vec<_>>();
    let value = if first {
        numbers.into_iter().min()
    } else {
        numbers.into_iter().max()
    };
    value.map_or_else(String::new, |number| number.to_string())
}

fn bibtex_type_matches(item_type: &str, wanted: &str) -> bool {
    let wanted = wanted.to_ascii_lowercase();
    if wanted == item_type {
        return true;
    }
    matches!(
        (item_type, wanted.as_str()),
        ("article", "journalarticle")
            | ("inproceedings" | "conference", "conferencepaper")
            | ("incollection" | "inbook", "booksection")
            | ("mastersthesis" | "phdthesis", "thesis")
            | ("techreport", "report")
            | ("misc", "document")
    )
}

fn take_characters(value: &str, count: i64) -> String {
    if count == 0 {
        value.to_string()
    } else {
        value
            .chars()
            .take(usize::try_from(count.max(0)).unwrap_or(usize::MAX))
            .collect()
    }
}

fn capitalize_words(value: &str) -> String {
    value
        .split_whitespace()
        .map(capitalize_first)
        .collect::<Vec<_>>()
        .join(" ")
}

fn remove_punctuation(value: &str, dash: &str) -> String {
    value
        .chars()
        .filter_map(|character| {
            if matches!(character, '-' | '–' | '—') {
                Some(dash.to_string())
            } else if character.is_ascii_punctuation() {
                None
            } else {
                Some(character.to_string())
            }
        })
        .collect()
}

fn select_words(value: &str, start: i64, count: Option<i64>) -> Result<String, FormulaError> {
    if start < 1 || count.is_some_and(|count| count < 0) {
        return Err(FormulaError::new(0, "select() indexes must be positive"));
    }
    let words = value.split_whitespace().collect::<Vec<_>>();
    let start = usize::try_from(start - 1)
        .map_err(|_| FormulaError::new(0, "select() start is too large"))?;
    if start >= words.len() {
        return Ok(String::new());
    }
    let end = if let Some(count) = count {
        start
            .saturating_add(
                usize::try_from(count)
                    .map_err(|_| FormulaError::new(0, "select() count is too large"))?,
            )
            .min(words.len())
    } else {
        words.len()
    };
    Ok(words[start..end].join(" "))
}

fn select_characters(value: &str, start: i64, count: Option<i64>) -> Result<String, FormulaError> {
    if start < 1 || count.is_some_and(|count| count < 0) {
        return Err(FormulaError::new(0, "substring() indexes must be positive"));
    }
    let characters = value.chars().collect::<Vec<_>>();
    let start = usize::try_from(start - 1)
        .map_err(|_| FormulaError::new(0, "substring() start is too large"))?;
    if start >= characters.len() {
        return Ok(String::new());
    }
    let end = if let Some(count) = count {
        start
            .saturating_add(
                usize::try_from(count)
                    .map_err(|_| FormulaError::new(0, "substring() count is too large"))?,
            )
            .min(characters.len())
    } else {
        characters.len()
    };
    Ok(characters[start..end].iter().collect())
}

fn transliterate_text(value: &str) -> String {
    visible_text(value)
        .chars()
        .flat_map(fold_character)
        .collect()
}

fn argument<'a>(arguments: &'a [Argument], index: usize, name: &str) -> Option<&'a ArgumentValue> {
    arguments
        .iter()
        .find(|argument| {
            argument
                .name
                .as_deref()
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
        })
        .or_else(|| {
            arguments
                .iter()
                .filter(|argument| argument.name.is_none())
                .nth(index)
        })
        .map(|argument| &argument.value)
}

fn argument_string(argument: &Argument) -> Option<&str> {
    match &argument.value {
        ArgumentValue::String(value) | ArgumentValue::Name(value) => Some(value),
        ArgumentValue::Number(_) | ArgumentValue::Boolean(_) => None,
    }
}

fn string_argument(
    arguments: &[Argument],
    index: usize,
    name: &str,
) -> Result<Option<String>, FormulaError> {
    argument(arguments, index, name)
        .map(|value| match value {
            ArgumentValue::String(value) | ArgumentValue::Name(value) => Ok(value.clone()),
            ArgumentValue::Number(_) | ArgumentValue::Boolean(_) => Err(FormulaError::new(
                0,
                format!("formula argument '{name}' must be a string"),
            )),
        })
        .transpose()
}

fn number_argument(
    arguments: &[Argument],
    index: usize,
    name: &str,
) -> Result<Option<i64>, FormulaError> {
    argument(arguments, index, name)
        .map(|value| match value {
            ArgumentValue::Number(value) => Ok(*value),
            ArgumentValue::String(value) | ArgumentValue::Name(value) => {
                value.parse().map_err(|_| {
                    FormulaError::new(0, format!("formula argument '{name}' must be a number"))
                })
            }
            ArgumentValue::Boolean(_) => Err(FormulaError::new(
                0,
                format!("formula argument '{name}' must be a number"),
            )),
        })
        .transpose()
}

fn boolean_argument(
    arguments: &[Argument],
    index: usize,
    name: &str,
) -> Result<Option<bool>, FormulaError> {
    argument(arguments, index, name)
        .map(|value| match value {
            ArgumentValue::Boolean(value) => Ok(*value),
            ArgumentValue::Name(value) if value.eq_ignore_ascii_case("true") => Ok(true),
            ArgumentValue::Name(value) if value.eq_ignore_ascii_case("false") => Ok(false),
            _ => Err(FormulaError::new(
                0,
                format!("formula argument '{name}' must be true or false"),
            )),
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::{compile_formula, evaluate_formula, validate_formula};
    use biblint_syntax::{Item, parse};

    fn entry(source: &str) -> biblint_syntax::Entry {
        let document = parse(source);
        let Item::Entry(entry) = document.items.into_iter().next().expect("entry") else {
            panic!("expected entry");
        };
        entry
    }

    fn evaluate(source: &str, formula: &str) -> Option<String> {
        let formula = compile_formula(formula).expect("formula compiles");
        evaluate_formula(&formula, &entry(source)).expect("formula evaluates")
    }

    #[test]
    fn parses_default_formula() {
        assert!(validate_formula("auth.lower + year + shorttitle(1,0)").is_ok());
    }

    #[test]
    fn evaluates_default_formula() {
        assert_eq!(
            evaluate(
                "@article{k,author={Smith, Ada},title={A Study of Things},year=2024}",
                "auth.lower + year + shorttitle(1,0)"
            ),
            Some("smith2024Study".to_string())
        );
    }

    #[test]
    fn supports_fallbacks_conditions_and_ternaries() {
        let source = "@article{k,title={A Study},year=2024}";
        assert_eq!(evaluate(source, "auth || title"), Some("Study".to_string()));
        assert_eq!(evaluate(source, "auth && title"), None);
        assert_eq!(
            evaluate(source, "auth ? year : title"),
            Some("Study".to_string())
        );
        assert_eq!(evaluate(source, "title > 3"), Some("Study".to_string()));
    }

    #[test]
    fn supports_direct_fields_and_filters() {
        assert_eq!(
            evaluate(
                "@article{k,title={A Study},year=2024,doi={10/ABC-123}}",
                "Title.skipwords.clean.lower + year + DOI.lower"
            ),
            Some("study202410/abc-123".to_string())
        );
    }

    #[test]
    fn supports_extra_and_pattern_fallbacks() {
        assert_eq!(
            evaluate(
                "@article{k,extra={tex.shortauthor: BBT},author={Smith, Ada},year=2024}",
                "extra('tex.shortauthor').clean.lower.len + year; auth.lower + year"
            ),
            Some("bbt2024".to_string())
        );
        assert_eq!(
            evaluate(
                "@article{k,author={Smith, Ada},year=2024}",
                "extra('tex.shortauthor').clean.lower.len + year; auth.lower + year"
            ),
            Some("smith2024".to_string())
        );
    }

    #[test]
    fn supports_creator_variants() {
        assert_eq!(
            evaluate(
                "@article{k,author={Smith, Ada and Jones, Bob and Doe, C.},title={A},year=2024}",
                "authEtal2"
            ),
            Some("Smith.Jones.etal".to_string())
        );
        assert_eq!(
            evaluate(
                "@article{k,author={Smith, Ada and Jones, Bob},title={A},year=2024}",
                "authorsn(2,sep='_').lower"
            ),
            Some("smith_jones".to_string())
        );
    }

    #[test]
    fn rejects_zotero_only_functions_and_unknown_filters() {
        assert!(validate_formula("group('Methods') + auth").is_err());
        assert!(validate_formula("auth.made_up").is_err());
    }

    #[test]
    fn supports_page_functions() {
        assert_eq!(
            evaluate(
                "@article{k,pages={7,41,73--97}}",
                "firstpage + '-' + lastpage"
            ),
            Some("7-97".to_string())
        );
    }
}
