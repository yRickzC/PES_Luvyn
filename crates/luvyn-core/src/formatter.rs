use crate::parser::strip_comment;
/// Normalize layout without discarding comments or changing prose/signatures.
pub fn format(source: &str) -> String {
    let mut lines = Vec::new();
    let mut blank = false;
    let mut methods = false;
    let mut main = false;
    for raw in source.lines() {
        let original = raw.trim();
        let normalized;
        let trimmed = if strip_comment(raw).trim() == original && original.starts_with("func ") {
            let signature = original.strip_prefix("func ").unwrap_or(original);
            normalized = format!(
                "func {}",
                crate::parser::normalized_signature(signature).unwrap_or_else(|| signature
                    .replace("->", " -> ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "))
            );
            normalized.as_str()
        } else {
            original
        };
        if trimmed.is_empty() {
            if !blank && !lines.is_empty() {
                lines.push(String::new());
            }
            blank = true;
            continue;
        }
        blank = false;
        if !raw.starts_with(char::is_whitespace) && !strip_comment(raw).trim().is_empty() {
            main = trimmed == "main:";
        }
        if (!raw.starts_with(char::is_whitespace)
            || (main && raw.len() - raw.trim_start().len() == 4))
            && !strip_comment(raw).trim().is_empty()
        {
            methods = matches!(
                strip_comment(raw).trim().split_once(':').map(|(k, _)| k),
                Some("export")
            );
        }
        if raw.starts_with(char::is_whitespace) && !strip_comment(raw).trim().is_empty() {
            let indent = if main {
                if raw.len() - raw.trim_start().len() >= 12 {
                    "            "
                } else if raw.len() - raw.trim_start().len() >= 8 {
                    "        "
                } else {
                    "    "
                }
            } else if methods && raw.len() - raw.trim_start().len() >= 8 {
                "        "
            } else {
                "    "
            };
            lines.push(format!("{indent}{trimmed}"));
        } else {
            lines.push(trimmed.into());
        }
    }
    while lines.last().is_some_and(|s| s.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}
