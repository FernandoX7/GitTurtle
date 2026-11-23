//! Bounded structured pretty-printing for JSON, TOML, and YAML previews.
//!
//! Operates strictly on supplied UTF-8 text; never reads files or alters
//! literal Git patches. On syntax errors, returns an explicit diagnostic so
//! callers can fall back to the literal source view.

use std::path::Path;

/// Maximum source size accepted for structured pretty-printing (2 MiB).
pub const MAX_STRUCTURED_BYTES: usize = 2 * 1024 * 1024;
/// Maximum output size produced by pretty-printing (4 MiB).
pub const MAX_STRUCTURED_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
/// Maximum bracket/brace nesting depth in flow pretty-printing.
const MAX_NESTING_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StructuredFormat {
    Json,
    Toml,
    Yaml,
}

impl StructuredFormat {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Toml => "TOML",
            Self::Yaml => "YAML",
        }
    }

    pub const fn language(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredPreview {
    pub format: StructuredFormat,
    pub formatted: String,
    pub changed_from_source: bool,
}

pub fn structured_format(path: &Path) -> Option<StructuredFormat> {
    let ext = path.extension()?.to_str()?;
    if ext.eq_ignore_ascii_case("json") {
        Some(StructuredFormat::Json)
    } else if ext.eq_ignore_ascii_case("toml") {
        Some(StructuredFormat::Toml)
    } else if ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml") {
        Some(StructuredFormat::Yaml)
    } else {
        None
    }
}

/// Attempt bounded structured pretty-printing for `.json`, `.toml`, `.yaml`, `.yml`.
/// Returns `None` for non-structured paths or oversized inputs.
pub fn pretty_print(source: &str, path: &Path) -> Option<Result<StructuredPreview, String>> {
    let format = structured_format(path)?;
    if source.trim().is_empty() || source.len() > MAX_STRUCTURED_BYTES {
        return None;
    }
    let result = match format {
        StructuredFormat::Json => format_json(source),
        StructuredFormat::Toml => format_toml(source),
        StructuredFormat::Yaml => format_yaml(source),
    };
    Some(result.and_then(|formatted| {
        if formatted.len() > MAX_STRUCTURED_OUTPUT_BYTES {
            return Err("Formatted output exceeds the 4 MiB preview limit.".into());
        }
        let changed_from_source = formatted.trim_end() != source.trim_end();
        Ok(StructuredPreview {
            format,
            formatted,
            changed_from_source,
        })
    }))
}

struct BoundedWriter {
    buf: Vec<u8>,
    limit: usize,
}

impl std::io::Write for BoundedWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        if self.buf.len().saturating_add(data.len()) > self.limit {
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                "Formatted output exceeds the 4 MiB preview limit.",
            ));
        }
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn check_json_depth(value: &serde_json::Value, depth: usize) -> Result<(), String> {
    if depth > MAX_NESTING_DEPTH {
        return Err("JSON nesting exceeds the 64-level preview limit.".into());
    }
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                check_json_depth(item, depth + 1)?;
            }
        }
        serde_json::Value::Object(map) => {
            for val in map.values() {
                check_json_depth(val, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn format_json(source: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(source).map_err(|err| format!("JSON syntax error: {err}"))?;
    check_json_depth(&value, 0)?;
    let mut writer = BoundedWriter {
        buf: Vec::with_capacity(source.len().min(MAX_STRUCTURED_OUTPUT_BYTES)),
        limit: MAX_STRUCTURED_OUTPUT_BYTES,
    };
    serde_json::to_writer_pretty(&mut writer, &value).map_err(|err| {
        if err.is_io() {
            "Formatted output exceeds the 4 MiB preview limit.".to_owned()
        } else {
            format!("Cannot format JSON: {err}")
        }
    })?;
    String::from_utf8(writer.buf).map_err(|err| format!("Cannot format JSON: {err}"))
}

fn format_toml(source: &str) -> Result<String, String> {
    let mut out = String::with_capacity(source.len().min(MAX_STRUCTURED_OUTPUT_BYTES));
    for raw_line in source.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('[') {
            push_line(&mut out, raw_line)?;
            continue;
        }
        // In TOML 1.0, inline tables `{ ... }` must stay on one line; only expand
        // long single-line array assignments `[...]`.
        if !trimmed.contains('{') && trimmed.contains('[') && trimmed.len() > 80 {
            let expanded = expand_delimited_line(raw_line, 0, false)?;
            push_str_bounded(&mut out, &expanded)?;
            push_str_bounded(&mut out, "\n")?;
        } else {
            push_line(&mut out, raw_line)?;
        }
    }
    Ok(out)
}

fn format_yaml(source: &str) -> Result<String, String> {
    let trimmed = source.trim();
    // Minified or flow-style YAML that is valid JSON pretty-prints cleanly into
    // indented YAML block syntax.
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed)
    {
        let mut out = String::with_capacity(source.len().min(MAX_STRUCTURED_OUTPUT_BYTES));
        emit_yaml_value(&value, 0, &mut out)?;
        return Ok(out);
    }
    let mut out = String::with_capacity(source.len().min(MAX_STRUCTURED_OUTPUT_BYTES));
    for raw_line in source.lines() {
        let indent = raw_line.bytes().take_while(|b| *b == b' ').count();
        let line_trimmed = raw_line.trim();
        if !line_trimmed.starts_with('#')
            && line_trimmed.len() > 80
            && (line_trimmed.contains('{') || line_trimmed.contains('['))
        {
            let expanded = expand_delimited_line(raw_line, indent, true)?;
            push_str_bounded(&mut out, &expanded)?;
            push_str_bounded(&mut out, "\n")?;
        } else {
            push_line(&mut out, raw_line)?;
        }
    }
    Ok(out)
}

fn yaml_key_needs_quotes(key: &str) -> bool {
    if key.is_empty() || key.trim() != key {
        return true;
    }
    if matches!(
        key,
        "true" | "false" | "null" | "~" | "yes" | "no" | "on" | "off"
    ) {
        return true;
    }
    key.chars().any(|ch| {
        ch.is_control()
            || matches!(
                ch,
                ':' | '#'
                    | '"'
                    | '\''
                    | '{'
                    | '}'
                    | '['
                    | ']'
                    | ','
                    | '&'
                    | '*'
                    | '!'
                    | '|'
                    | '>'
                    | '%'
                    | '@'
                    | '`'
                    | '?'
            )
    })
}

fn emit_yaml_value(
    value: &serde_json::Value,
    indent: usize,
    out: &mut String,
) -> Result<(), String> {
    if indent / 2 > MAX_NESTING_DEPTH {
        return Err("YAML nesting exceeds the 64-level preview limit.".into());
    }
    match value {
        serde_json::Value::Null => push_str_bounded(out, "null\n"),
        serde_json::Value::Bool(b) => push_str_bounded(out, if *b { "true\n" } else { "false\n" }),
        serde_json::Value::Number(n) => {
            push_str_bounded(out, &n.to_string())?;
            push_str_bounded(out, "\n")
        }
        serde_json::Value::String(s) => {
            let encoded = serde_json::to_string(s)
                .map_err(|err| format!("Cannot format YAML string: {err}"))?;
            push_str_bounded(out, &encoded)?;
            push_str_bounded(out, "\n")
        }
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                return push_str_bounded(out, "[]\n");
            }
            for item in items {
                push_indent(out, indent)?;
                match item {
                    serde_json::Value::Object(map) if !map.is_empty() => {
                        push_str_bounded(out, "-\n")?;
                        emit_yaml_value(item, indent + 2, out)?;
                    }
                    serde_json::Value::Array(nested) if !nested.is_empty() => {
                        push_str_bounded(out, "-\n")?;
                        emit_yaml_value(item, indent + 2, out)?;
                    }
                    _ => {
                        push_str_bounded(out, "- ")?;
                        emit_yaml_value(item, indent + 2, out)?;
                    }
                }
            }
            Ok(())
        }
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                return push_str_bounded(out, "{}\n");
            }
            for (key, val) in map {
                push_indent(out, indent)?;
                if yaml_key_needs_quotes(key) {
                    let encoded = serde_json::to_string(key)
                        .map_err(|err| format!("Cannot format YAML key: {err}"))?;
                    push_str_bounded(out, &encoded)?;
                } else {
                    push_str_bounded(out, key)?;
                }
                match val {
                    serde_json::Value::Object(inner) if !inner.is_empty() => {
                        push_str_bounded(out, ":\n")?;
                        emit_yaml_value(val, indent + 2, out)?;
                    }
                    serde_json::Value::Array(inner) if !inner.is_empty() => {
                        push_str_bounded(out, ":\n")?;
                        emit_yaml_value(val, indent + 2, out)?;
                    }
                    _ => {
                        push_str_bounded(out, ": ")?;
                        emit_yaml_value(val, indent + 2, out)?;
                    }
                }
            }
            Ok(())
        }
    }
}

fn expand_delimited_line(
    line: &str,
    base_indent: usize,
    allow_braces: bool,
) -> Result<String, String> {
    let mut out = String::with_capacity(line.len().saturating_mul(2));
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut prev_ch: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(q) = quote {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' && q == '"' {
                escaped = true;
            } else if ch == q {
                quote = None;
            }
            prev_ch = Some(ch);
            continue;
        }
        match ch {
            '"' => {
                quote = Some(ch);
                out.push(ch);
            }
            // Only treat `'` as opening a single-quoted string when it is not
            // an apostrophe inside an unquoted word (e.g. `don't`).
            '\'' if !prev_ch.is_some_and(char::is_alphanumeric) => {
                quote = Some(ch);
                out.push(ch);
            }
            '#' => {
                out.push(ch);
                for rest in chars.by_ref() {
                    out.push(rest);
                }
            }
            '[' => {
                depth += 1;
                if depth > MAX_NESTING_DEPTH {
                    return Err("Structured nesting exceeds the 64-level preview limit.".into());
                }
                out.push(ch);
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
                if chars.peek() != Some(&']') {
                    out.push('\n');
                    push_indent(&mut out, base_indent + depth * 2)?;
                }
            }
            '{' if allow_braces => {
                depth += 1;
                if depth > MAX_NESTING_DEPTH {
                    return Err("Structured nesting exceeds the 64-level preview limit.".into());
                }
                out.push(ch);
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
                if chars.peek() != Some(&'}') {
                    out.push('\n');
                    push_indent(&mut out, base_indent + depth * 2)?;
                }
            }
            ']' => {
                depth = depth.saturating_sub(1);
                let trimmed_end = out.trim_end_matches(' ');
                let just_opened = trimmed_end.ends_with('[');
                if !just_opened {
                    out.truncate(trimmed_end.len());
                    out.push('\n');
                    push_indent(&mut out, base_indent + depth * 2)?;
                }
                out.push(ch);
            }
            '}' if allow_braces => {
                depth = depth.saturating_sub(1);
                let trimmed_end = out.trim_end_matches(' ');
                let just_opened = trimmed_end.ends_with('{');
                if !just_opened {
                    out.truncate(trimmed_end.len());
                    out.push('\n');
                    push_indent(&mut out, base_indent + depth * 2)?;
                }
                out.push(ch);
            }
            ',' if depth > 0 => {
                out.push(',');
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
                out.push('\n');
                push_indent(&mut out, base_indent + depth * 2)?;
            }
            _ => out.push(ch),
        }
        prev_ch = Some(ch);
        if out.len() > MAX_STRUCTURED_OUTPUT_BYTES {
            return Err("Formatted output exceeds the 4 MiB preview limit.".into());
        }
    }
    Ok(out)
}

fn push_indent(out: &mut String, spaces: usize) -> Result<(), String> {
    if out.len() + spaces > MAX_STRUCTURED_OUTPUT_BYTES {
        return Err("Formatted output exceeds the 4 MiB preview limit.".into());
    }
    for _ in 0..spaces {
        out.push(' ');
    }
    Ok(())
}

fn push_line(out: &mut String, line: &str) -> Result<(), String> {
    push_str_bounded(out, line)?;
    push_str_bounded(out, "\n")
}

fn push_str_bounded(out: &mut String, s: &str) -> Result<(), String> {
    if out.len() + s.len() > MAX_STRUCTURED_OUTPUT_BYTES {
        return Err("Formatted output exceeds the 4 MiB preview limit.".into());
    }
    out.push_str(s);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_prints_minified_json_and_reports_invalid_json() {
        let min = r#"{"a":1,"nested":{"b":[true,false]}}"#;
        let preview = pretty_print(min, Path::new("config.json"))
            .expect("json detected")
            .expect("valid json");
        assert_eq!(preview.format, StructuredFormat::Json);
        assert!(preview.changed_from_source);
        assert!(preview.formatted.contains("  \"nested\": {\n"));

        let err = pretty_print("{bad", Path::new("broken.json"))
            .expect("json detected")
            .expect_err("invalid json fails");
        assert!(err.contains("JSON syntax error"));
    }

    #[test]
    fn pretty_prints_toml_arrays_preserves_inline_tables_and_quotes_yaml_keys() {
        let toml_inline = "dep = { version = \"1.0\", features = [\"a\", \"b\"] }\n";
        let toml_out = pretty_print(toml_inline, Path::new("Cargo.toml"))
            .expect("toml detected")
            .expect("valid toml");
        assert!(
            !toml_out.changed_from_source,
            "TOML 1.0 inline tables must remain single-line"
        );

        let toml_array = "features = [\"alpha\", \"beta\", \"gamma\", \"delta\", \"epsilon\", \"zeta\", \"eta\", \"theta\", \"iota\"]\n";
        let arr_out = pretty_print(toml_array, Path::new("Cargo.toml"))
            .expect("toml detected")
            .expect("valid toml");
        assert!(arr_out.changed_from_source);
        assert!(arr_out.formatted.contains("\"alpha\",\n"));

        let flow_yaml = r#"{"service":"api","a: b":"quoted","ports":[80,443]}"#;
        let yaml_out = pretty_print(flow_yaml, Path::new("deploy.yml"))
            .expect("yaml detected")
            .expect("valid yaml");
        assert!(yaml_out.changed_from_source);
        assert!(yaml_out.formatted.contains("service: \"api\"\n"));
        assert!(yaml_out.formatted.contains("\"a: b\": \"quoted\"\n"));
        assert!(yaml_out.formatted.contains("  - 443\n"));

        let deep_json = format!("{}0{}", "[".repeat(70), "]".repeat(70));
        let err = pretty_print(&deep_json, Path::new("deep.json"))
            .expect("json detected")
            .expect_err("deep nesting rejected");
        assert!(err.contains("64-level"));
    }
}
