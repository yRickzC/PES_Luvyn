use crate::parser::strip_comment;
/// Normalize layout without discarding comments or changing prose/signatures.
pub fn format(source: &str) -> String {
    let mut lines = Vec::new();
    let mut blank = false;
    for raw in source.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            if !blank && !lines.is_empty() {
                lines.push(String::new());
            }
            blank = true;
            continue;
        }
        blank = false;
        if raw.starts_with(char::is_whitespace) && !strip_comment(raw).trim().is_empty() {
            lines.push(format!("    {trimmed}"));
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
