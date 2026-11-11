//! Prompt templates: `{{name}}` and `{{name = default}}` blanks inside a
//! prompt, filled in by hand before the prompt is copied and used.
//! A `\{{name}}` prefix keeps the braces as literal text. Semantics mirror
//! Threadbook's PromptTemplates.

/// How much of a template is scanned for variables, in characters.
/// Longer text is still parsed; the tail is simply not scanned.
const MAX_SCAN_CHARS: usize = 100_000;
/// Longest variable name, in characters.
const MAX_NAME_CHARS: usize = 60;

/// One distinct `{{variable}}` in a template, in first-appearance order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    pub name: String,
    pub default: Option<String>,
}

/// A parsed template: the distinct variables it asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Template {
    pub variables: Vec<Variable>,
}

/// One raw `{{...}}` occurrence in the text; positions are byte offsets.
struct Occurrence {
    /// Offset of the occurrence (of the backslash, when escaped).
    start: usize,
    /// Offset just past the closing `}}`.
    end: usize,
    name: String,
    default: Option<String>,
    escaped: bool,
}

/// Finds every well-formed `{{...}}`. Escaped occurrences are only
/// returned when `include_escaped` is set (rendering needs them; parsing
/// a template's variable list does not).
fn scan(text: &str, include_escaped: bool) -> Vec<Occurrence> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] != b'{' || b[i + 1] != b'{' {
            i += 1;
            continue;
        }
        let escaped = i > 0 && b[i - 1] == b'\\';
        if escaped && !include_escaped {
            i += 2;
            continue;
        }
        let mut close = None;
        let mut j = i + 2;
        while j + 1 < b.len() {
            if b[j] == b'}' && b[j + 1] == b'}' {
                close = Some(j);
                break;
            }
            j += 1;
        }
        let Some(close) = close else { break };
        if let Some((name, default)) = parse_inner(&text[i + 2..close]) {
            out.push(Occurrence {
                start: if escaped { i - 1 } else { i },
                end: close + 2,
                name,
                default,
                escaped,
            });
        }
        i = close + 2;
    }
    out
}

/// Splits the inside of `{{...}}` into name and optional default. The
/// name is trimmed and must be 1..=60 characters; an empty default is no
/// default. Braces and newlines inside disqualify the whole occurrence.
fn parse_inner(inner: &str) -> Option<(String, Option<String>)> {
    if inner.is_empty() || inner.chars().any(|c| matches!(c, '{' | '}' | '\n' | '\r')) {
        return None;
    }
    let (raw_name, raw_default) = match inner.find('=') {
        Some(eq) => (&inner[..eq], Some(&inner[eq + 1..])),
        None => (inner, None),
    };
    let name = raw_name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return None;
    }
    let default = raw_default.map(str::trim).filter(|d| !d.is_empty()).map(str::to_string);
    Some((name.to_string(), default))
}

/// The first `MAX_SCAN_CHARS` characters of `text`.
fn scan_prefix(text: &str) -> &str {
    match text.char_indices().nth(MAX_SCAN_CHARS) {
        Some((byte, _)) => &text[..byte],
        None => text,
    }
}

/// Parses a template's variables: distinct names, case-insensitively, in
/// first-appearance order, keeping the first occurrence's name and the
/// first default any occurrence gives it.
pub fn parse_template(text: &str) -> Template {
    let mut variables: Vec<Variable> = Vec::new();
    for occ in scan(scan_prefix(text), false) {
        let key = occ.name.to_lowercase();
        match variables.iter_mut().find(|v| v.name.to_lowercase() == key) {
            Some(existing) => {
                if existing.default.is_none() {
                    existing.default = occ.default;
                }
            }
            None => variables.push(Variable { name: occ.name, default: occ.default }),
        }
    }
    Template { variables }
}

/// Fills a template in. Values match variable names case-insensitively
/// and unknown values are ignored; a variable with no (non-empty) value
/// falls back to its default, and `\{{name}}` renders as literal
/// `{{name}}`. When any variable is left with neither, returns the
/// missing names, deduplicated, in template order.
pub fn render(template_text: &str, values: &[(String, String)]) -> Result<String, Vec<String>> {
    // Case-insensitive value lookup; a later pair wins over an earlier
    // one, and keys are trimmed, as in the reference implementation.
    let mut lookup: Vec<(String, &str)> = Vec::new();
    for (name, value) in values {
        let key = name.trim().to_lowercase();
        match lookup.iter_mut().find(|(n, _)| *n == key) {
            Some(slot) => slot.1 = value,
            None => lookup.push((key, value)),
        }
    }
    let mut out = String::with_capacity(template_text.len());
    let mut missing: Vec<String> = Vec::new();
    let mut cursor = 0;
    for occ in scan(template_text, true) {
        out.push_str(&template_text[cursor..occ.start]);
        if occ.escaped {
            // Drop the backslash, keep the braces as literal text.
            out.push_str(&template_text[occ.start + 1..occ.end]);
        } else if let Some(value) = lookup
            .iter()
            .find(|(n, _)| *n == occ.name.to_lowercase())
            .map(|(_, v)| *v)
            .filter(|v| !v.is_empty())
        {
            out.push_str(value);
        } else if let Some(default) = &occ.default {
            out.push_str(default);
        } else {
            out.push_str(&template_text[occ.start..occ.end]);
            if !missing.iter().any(|m| m.to_lowercase() == occ.name.to_lowercase()) {
                missing.push(occ.name.clone());
            }
        }
        cursor = occ.end;
    }
    out.push_str(&template_text[cursor..]);
    if missing.is_empty() { Ok(out) } else { Err(missing) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn basic_substitution() {
        let t = parse_template("Hello {{name}}, welcome.");
        assert_eq!(t.variables.len(), 1);
        assert_eq!(t.variables[0].name, "name");
        assert_eq!(t.variables[0].default, None);
        let out = render("Hello {{name}}, welcome.", &values(&[("name", "Ada")])).unwrap();
        assert_eq!(out, "Hello Ada, welcome.");
    }

    #[test]
    fn default_used_and_overridden() {
        let t = parse_template("Write in {{lang = Rust}}.");
        assert_eq!(t.variables[0].default.as_deref(), Some("Rust"));
        assert_eq!(render("Write in {{lang = Rust}}.", &[]).unwrap(), "Write in Rust.");
        let out = render("Write in {{lang = Rust}}.", &values(&[("lang", "Go")])).unwrap();
        assert_eq!(out, "Write in Go.");
    }

    #[test]
    fn escape_is_literal() {
        let t = parse_template(r"Use \{{name}} as-is.");
        assert!(t.variables.is_empty());
        let out = render(r"Use \{{name}} as-is.", &values(&[("name", "Ada")])).unwrap();
        assert_eq!(out, "Use {{name}} as-is.");
    }

    #[test]
    fn names_are_case_insensitive() {
        let out = render("Hi {{Name}}.", &values(&[("nAmE", "Ada")])).unwrap();
        assert_eq!(out, "Hi Ada.");
    }

    #[test]
    fn duplicates_dedupe_keeping_first() {
        let t = parse_template("{{a}} {{A}} {{a = x}} {{b}}");
        assert_eq!(t.variables.len(), 2);
        assert_eq!(t.variables[0].name, "a");
        assert_eq!(t.variables[0].default.as_deref(), Some("x"));
        assert_eq!(t.variables[1].name, "b");
    }

    #[test]
    fn missing_variables_are_an_error() {
        let err = render("{{a}} and {{b}} and {{B}}", &values(&[("a", "1")])).unwrap_err();
        assert_eq!(err, vec!["b".to_string()]);
    }

    #[test]
    fn unknown_values_are_ignored() {
        let out = render("{{a}}", &values(&[("a", "1"), ("zzz", "9")])).unwrap();
        assert_eq!(out, "1");
    }

    #[test]
    fn scan_stops_at_the_bound() {
        let mut text = "x".repeat(MAX_SCAN_CHARS);
        text.push_str("{{late}}");
        assert!(parse_template(&text).variables.is_empty());
        let mut text = "x".repeat(MAX_SCAN_CHARS - 10);
        text.push_str("{{early}}");
        assert_eq!(parse_template(&text).variables.len(), 1);
    }
}
