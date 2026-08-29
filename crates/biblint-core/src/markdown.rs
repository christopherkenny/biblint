use crate::KeyChange;
use std::collections::HashMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkdownUpdate {
    pub output: String,
    pub replacements: usize,
}

pub fn update_markdown_citations(
    source: &str,
    changes: &[KeyChange],
) -> Result<MarkdownUpdate, String> {
    let renames = rename_map(changes)?;
    if renames.is_empty() {
        return Ok(MarkdownUpdate {
            output: source.to_string(),
            replacements: 0,
        });
    }

    let protected = protected_regions(source);
    let mut edits = Vec::new();
    let mut position = 0usize;
    while position < source.len() {
        if protected[position] {
            position += 1;
            continue;
        }
        if source.as_bytes()[position] != b'@' || !can_start_citation(source, position) {
            position = next_char_boundary(source, position);
            continue;
        }
        if let Some((end, replacement)) = find_replacement(source, position + 1, &renames) {
            edits.push((position + 1, end, replacement));
            position = end;
        } else {
            position = next_char_boundary(source, position);
        }
    }

    let replacements = edits.len();
    let mut output = source.to_string();
    for (start, end, replacement) in edits.into_iter().rev() {
        output.replace_range(start..end, &replacement);
    }
    Ok(MarkdownUpdate {
        output,
        replacements,
    })
}

fn rename_map(changes: &[KeyChange]) -> Result<HashMap<String, String>, String> {
    let mut renames = HashMap::new();
    for change in changes {
        let old = change.old.trim();
        let new = change.new.trim();
        if old.is_empty() || new.is_empty() {
            return Err("citation key changes cannot contain an empty key".to_string());
        }
        if old == new {
            continue;
        }
        let normalized = normalize_key(old);
        if let Some(previous) = renames.get(&normalized)
            && previous != new
        {
            return Err(format!(
                "citation key '{old}' has conflicting replacements '{previous}' and '{new}'"
            ));
        }
        renames.insert(normalized, new.to_string());
    }
    Ok(renames)
}

fn find_replacement(
    source: &str,
    start: usize,
    renames: &HashMap<String, String>,
) -> Option<(usize, String)> {
    let mut end = start;
    while let Some(character) = source[end..].chars().next() {
        if !is_citation_key_character(character) {
            break;
        }
        end += character.len_utf8();
    }
    if end == start {
        return None;
    }

    let mut candidate_end = end;
    loop {
        let candidate = &source[start..candidate_end];
        if let Some(replacement) = renames.get(&normalize_key(candidate)) {
            return Some((candidate_end, replacement.clone()));
        }
        let Some((last_start, last)) = source[start..candidate_end].char_indices().next_back()
        else {
            break;
        };
        if !is_terminal_citation_punctuation(last) {
            break;
        }
        candidate_end = start + last_start;
    }
    None
}

fn can_start_citation(source: &str, at: usize) -> bool {
    let previous = source[..at].chars().next_back();
    match previous {
        Some('-') => source[..at - 1]
            .chars()
            .next_back()
            .is_none_or(|character| {
                character.is_whitespace() || matches!(character, '[' | '(' | '{' | ';')
            }),
        Some(character)
            if character.is_alphanumeric() || matches!(character, '_' | '/' | '\\' | '.' | ':') =>
        {
            false
        }
        Some('@') => false,
        None | Some(_) => true,
    }
}

fn is_citation_key_character(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | ':' | '.' | '-' | '+' | '/')
}

fn is_terminal_citation_punctuation(character: char) -> bool {
    matches!(character, '.' | ':' | '!' | '?')
}

fn normalize_key(key: &str) -> String {
    key.to_ascii_lowercase()
}

fn protected_regions(source: &str) -> Vec<bool> {
    let mut protected = vec![false; source.len()];
    mark_fenced_code(source, &mut protected);
    mark_inline_code(source, &mut protected);
    mark_html_regions(source, &mut protected);
    protected
}

fn mark_fenced_code(source: &str, protected: &mut [bool]) {
    let mut line_start = 0usize;
    let mut fence = None;
    while line_start < source.len() {
        let line_end = source[line_start..]
            .find('\n')
            .map_or(source.len(), |offset| line_start + offset + 1);
        let line = &source[line_start..line_end];
        if let Some((marker, length)) = fence {
            mark_range(protected, line_start, line_end);
            if closes_fence(line, marker, length) {
                fence = None;
            }
        } else if let Some((marker, length)) = opens_fence(line) {
            mark_range(protected, line_start, line_end);
            fence = Some((marker, length));
        }
        line_start = line_end;
    }
}

fn opens_fence(line: &str) -> Option<(u8, usize)> {
    let bytes = line.as_bytes();
    let mut position = 0usize;
    while position < bytes.len() && position < 3 && bytes[position] == b' ' {
        position += 1;
    }
    let marker = *bytes.get(position)?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let start = position;
    while bytes.get(position) == Some(&marker) {
        position += 1;
    }
    let length = position - start;
    (length >= 3).then_some((marker, length))
}

fn closes_fence(line: &str, marker: u8, minimum_length: usize) -> bool {
    let bytes = line.as_bytes();
    let mut position = 0usize;
    while position < bytes.len() && position < 3 && bytes[position] == b' ' {
        position += 1;
    }
    if bytes.get(position) != Some(&marker) {
        return false;
    }
    let start = position;
    while bytes.get(position) == Some(&marker) {
        position += 1;
    }
    position - start >= minimum_length
        && line[position..]
            .trim_matches([' ', '\t', '\r', '\n'])
            .is_empty()
}

fn mark_inline_code(source: &str, protected: &mut [bool]) {
    let bytes = source.as_bytes();
    let mut position = 0usize;
    while position < source.len() {
        if protected[position] {
            position += 1;
            continue;
        }
        if bytes[position] != b'`' {
            position = next_char_boundary(source, position);
            continue;
        }
        let run_length = backtick_run_length(bytes, position);
        let content_start = position + run_length;
        if let Some(close) = find_backtick_run(source, content_start, run_length, protected) {
            let end = close + run_length;
            mark_range(protected, position, end);
            position = end;
        } else {
            position = content_start;
        }
    }
}

fn find_backtick_run(
    source: &str,
    start: usize,
    length: usize,
    protected: &[bool],
) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut position = start;
    while position + length <= source.len() {
        if bytes[position] == b'`'
            && bytes[position..position + length]
                .iter()
                .all(|byte| *byte == b'`')
            && !protected[position..position + length]
                .iter()
                .any(|is_protected| *is_protected)
        {
            return Some(position);
        }
        position += 1;
    }
    None
}

fn mark_html_regions(source: &str, protected: &mut [bool]) {
    let bytes = source.as_bytes();
    let mut position = 0usize;
    while position < source.len() {
        if protected[position] || bytes[position] != b'<' {
            position = next_char_boundary(source, position);
            continue;
        }
        let end = if source[position..].starts_with("<!--") {
            source[position + 4..]
                .find("-->")
                .map(|offset| position + 4 + offset + 3)
        } else {
            find_html_tag_end(source, position + 1)
        };
        if let Some(end) = end {
            mark_range(protected, position, end);
            position = end;
        } else {
            position = next_char_boundary(source, position);
        }
    }
}

fn find_html_tag_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut quote = None;
    let mut position = start;
    while position < source.len() {
        let byte = bytes[position];
        if let Some(expected) = quote {
            if byte == expected {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
        } else if byte == b'>' {
            return Some(position + 1);
        }
        position += 1;
    }
    None
}

fn mark_range(protected: &mut [bool], start: usize, end: usize) {
    if start < end && end <= protected.len() {
        protected[start..end].fill(true);
    }
}

fn backtick_run_length(bytes: &[u8], start: usize) -> usize {
    let mut length = 0usize;
    while bytes.get(start + length) == Some(&b'`') {
        length += 1;
    }
    length
}

fn next_char_boundary(source: &str, position: usize) -> usize {
    position + source[position..].chars().next().map_or(1, char::len_utf8)
}

#[cfg(test)]
mod tests {
    use super::{MarkdownUpdate, update_markdown_citations};
    use crate::KeyChange;

    fn change(old: &str, new: &str) -> KeyChange {
        KeyChange {
            old: old.to_string(),
            new: new.to_string(),
        }
    }

    #[test]
    fn updates_pandoc_and_textual_citations() {
        let source = "See [@old; @other, p. 4] and @old.\n";
        let result =
            update_markdown_citations(source, &[change("old", "new"), change("other", "next")])
                .expect("update citations");
        assert_eq!(
            result,
            MarkdownUpdate {
                output: "See [@new; @next, p. 4] and @new.\n".to_string(),
                replacements: 3,
            }
        );
    }

    #[test]
    fn preserves_code_and_non_citation_at_signs() {
        let source = "`[@old] @old`\n\n```text\n[@old]\n```\n\nemail@example.com [@old]\n";
        let result =
            update_markdown_citations(source, &[change("old", "new")]).expect("update citations");
        assert_eq!(
            result.output,
            "`[@old] @old`\n\n```text\n[@old]\n```\n\nemail@example.com [@new]\n"
        );
        assert_eq!(result.replacements, 1);
    }

    #[test]
    fn preserves_html_tags_and_comments() {
        let source = "<span data-cite=\"@old\">[@old]</span>\n<!-- @old -->\n";
        let result =
            update_markdown_citations(source, &[change("old", "new")]).expect("update citations");
        assert_eq!(
            result.output,
            "<span data-cite=\"@old\">[@new]</span>\n<!-- @old -->\n"
        );
        assert_eq!(result.replacements, 1);
    }

    #[test]
    fn rejects_conflicting_case_insensitive_changes() {
        let error =
            update_markdown_citations("[@old]", &[change("old", "new"), change("OLD", "other")])
                .expect_err("conflicting changes should fail");
        assert!(error.contains("conflicting replacements"));
    }
}
