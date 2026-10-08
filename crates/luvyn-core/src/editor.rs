//! Editor operations return UTF-16 ranges, directly usable by Monaco or a future LSP adapter.
use crate::{Error, Result, model::*, parser, query, workspace::Project};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Range {
    pub start_line_number: u32,
    pub start_column: u32,
    pub end_line_number: u32,
    pub end_column: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextEdit {
    pub file: String,
    pub range: Range,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Completion {
    pub label: String,
    pub detail: String,
    pub insert: String,
    pub import: Option<TextEdit>,
    pub snippet: bool,
}

pub fn range(location: &Location, source: &str) -> Range {
    let line = source
        .lines()
        .nth(location.line.saturating_sub(1) as usize)
        .unwrap_or("");
    let start = line
        .chars()
        .take(location.column.saturating_sub(1) as usize)
        .map(char::len_utf16)
        .sum::<usize>() as u32
        + 1;
    let length = line
        .chars()
        .skip(location.column.saturating_sub(1) as usize)
        .take(location.length as usize)
        .map(char::len_utf16)
        .sum::<usize>() as u32;
    Range {
        start_line_number: location.line,
        start_column: start,
        end_line_number: location.line,
        end_column: start + length,
    }
}
pub fn import_edit(file: &str, source: &str, target: &Symbol) -> TextEdit {
    let line = source
        .lines()
        .enumerate()
        .filter(|(_, l)| l.starts_with("namespace ") || l.starts_with("import "))
        .map(|(i, _)| i as u32 + 2)
        .max()
        .unwrap_or(1);
    TextEdit {
        file: file.into(),
        range: Range {
            start_line_number: line,
            start_column: 1,
            end_line_number: line,
            end_column: 1,
        },
        text: format!("import {}\n", target.qualified),
    }
}
pub fn completions(project: &Project, file: &str, source: &str, line: u32) -> Vec<Completion> {
    let parsed = project.parsed(file);
    let namespace = parsed.map_or("", |f| f.namespace.as_str());
    let current = source
        .lines()
        .nth(line.saturating_sub(1) as usize)
        .unwrap_or("");
    let previous = source
        .lines()
        .take(line.saturating_sub(1) as usize)
        .filter(|l| !l.trim().is_empty())
        .last()
        .unwrap_or("");
    let interfaces = current.trim().starts_with("implements") || previous.trim() == "implements:";
    let mut output = Vec::new();
    for s in project
        .graph
        .symbols
        .iter()
        .filter(|s| s.parent.is_none() && (!interfaces || s.kind == "interface"))
    {
        let same = namespace == s.namespace;
        let imports = parsed.is_some_and(|f| {
            f.imports
                .iter()
                .any(|i| i.target == s.qualified || i.target == format!("{}.*", s.namespace))
        });
        let alias = parsed.and_then(|f| {
            f.imports
                .iter()
                .find(|i| i.target == s.qualified)
                .and_then(|i| i.alias.clone())
        });
        output.push(Completion {
            label: s.name.clone(),
            detail: format!("{} {}", s.kind, s.qualified),
            insert: alias.unwrap_or_else(|| s.name.clone()),
            import: (!same && !imports).then(|| import_edit(file, source, s)),
            snippet: false,
        });
    }
    for kind in parser::KINDS {
        output.push(Completion {
            label: (*kind).into(),
            detail: "declaration".into(),
            insert: format!("{kind} ${{1:Name}}\n\npurpose:\n    ${{2:intent}}\n"),
            import: None,
            snippet: true,
        });
    }
    for key in parser::TEXT_SECTIONS
        .iter()
        .chain(parser::RELATIONS)
        .chain(["exposes"].iter())
    {
        output.push(Completion {
            label: format!("{key}:"),
            detail: "section".into(),
            insert: format!("{key}:\n    ${{1}}"),
            import: None,
            snippet: true,
        });
    }
    output
}
pub fn symbol_at<'a>(
    graph: &'a Graph,
    file: &str,
    line: u32,
    column: u32,
    source: &str,
) -> Option<&'a Symbol> {
    for s in graph.symbols.iter().filter(|s| s.location.file == file) {
        let r = range(&s.location, source);
        if line == r.start_line_number && column >= r.start_column && column <= r.end_column {
            return Some(s);
        }
    }
    for edge in graph.edges.iter().filter(|e| e.location.file == file) {
        let r = range(&edge.location, source);
        if line == r.start_line_number && column >= r.start_column && column <= r.end_column {
            return graph.symbol(&edge.to);
        }
    }
    // Unresolved names can still provide a unique workspace suggestion, without creating an edge.
    let text = source.lines().nth(line.saturating_sub(1) as usize)?;
    let mut byte = 0;
    let mut units = 1;
    for (i, c) in text.char_indices() {
        if units >= column {
            byte = i;
            break;
        }
        units += c.len_utf16() as u32;
        byte = i + c.len_utf8();
    }
    let left = text[..byte]
        .char_indices()
        .rev()
        .find(|(_, c)| !c.is_alphanumeric() && !matches!(c, '_' | '.'))
        .map_or(0, |(i, c)| i + c.len_utf8());
    let right = text[byte..]
        .find(|c: char| !c.is_alphanumeric() && !matches!(c, '_' | '.'))
        .map_or(text.len(), |i| byte + i);
    let word = &text[left..right];
    let matches = graph.by_name.get(&word.to_lowercase())?;
    (matches.len() == 1).then(|| &graph.symbols[matches[0]])
}
/// Rename only resolved declarations, relationship/type references and explicit symbol imports.
/// Plain prose and source mappings remain untouched. Ambiguous/invalid workspaces are refused.
pub fn rename(
    project: &Project,
    id: &str,
    new_name: &str,
    sources: &BTreeMap<String, String>,
) -> Result<Vec<TextEdit>> {
    if project.graph.has_errors() {
        return Err(Error::Message(
            "Resolve project diagnostics before rename".into(),
        ));
    }
    if !parser::identifier(new_name) || new_name.contains('.') {
        return Err(Error::Message("Rename requires a simple identifier".into()));
    }
    let symbol = project
        .graph
        .symbol(id)
        .ok_or_else(|| Error::Message("Symbol not found".into()))?;
    let prefix = symbol.qualified.strip_suffix(&symbol.name).unwrap_or("");
    let new_qualified = format!("{prefix}{new_name}");
    if project
        .graph
        .symbols
        .iter()
        .any(|s| s.id != id && s.qualified == new_qualified)
    {
        return Err(Error::Message(
            "Rename would create a duplicate symbol".into(),
        ));
    }
    let mut edits = Vec::new();
    let source = sources
        .get(&symbol.location.file)
        .ok_or_else(|| Error::Message("Missing document for rename".into()))?;
    edits.push(TextEdit {
        file: symbol.location.file.clone(),
        range: range(&symbol.location, source),
        text: new_name.into(),
    });
    for edge in project
        .graph
        .edges
        .iter()
        .filter(|e| e.to == id && e.relation != "exposes")
    {
        if let Some(source) = sources.get(&edge.location.file) {
            let parsed = project.parsed(&edge.location.file);
            let old = source
                .lines()
                .nth(edge.location.line.saturating_sub(1) as usize)
                .unwrap_or("")
                .chars()
                .skip(edge.location.column.saturating_sub(1) as usize)
                .take(edge.location.length as usize)
                .collect::<String>();
            if parsed.is_some_and(|f| {
                f.imports
                    .iter()
                    .any(|i| i.target == symbol.qualified && i.alias.as_deref() == Some(&old))
            }) {
                continue;
            }
            let text = if old == symbol.qualified {
                new_qualified.clone()
            } else {
                new_name.into()
            };
            edits.push(TextEdit {
                file: edge.location.file.clone(),
                range: range(&edge.location, source),
                text,
            });
        }
    }
    for (file, source) in sources {
        if let Some(parsed) = project.parsed(file) {
            for import in parsed
                .imports
                .iter()
                .filter(|i| i.target == symbol.qualified)
            {
                edits.push(TextEdit {
                    file: file.clone(),
                    range: range(&import.location, source),
                    text: new_qualified.clone(),
                });
            }
        }
    }
    edits.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.range.start_line_number.cmp(&b.range.start_line_number))
            .then(a.range.start_column.cmp(&b.range.start_column))
    });
    edits.dedup_by(|a, b| {
        a.file == b.file
            && a.range.start_line_number == b.range.start_line_number
            && a.range.start_column == b.range.start_column
    });
    // Simulate the transaction. Rename is accepted only if the resulting project resolves without errors.
    let changed = apply_edits(sources, &edits)?;
    let parsed: Vec<_> = changed.iter().map(|(f, s)| parser::parse(f, s)).collect();
    let graph = crate::resolver::resolve(&parsed, BTreeMap::new());
    if graph.has_errors() {
        return Err(Error::Message(
            "Rename would break references; use a qualified name or resolve import conflicts"
                .into(),
        ));
    }
    Ok(edits)
}
pub fn apply_edits(
    sources: &BTreeMap<String, String>,
    edits: &[TextEdit],
) -> Result<BTreeMap<String, String>> {
    let mut changed = sources.clone();
    let mut grouped = BTreeMap::<&str, Vec<&TextEdit>>::new();
    for edit in edits {
        grouped.entry(&edit.file).or_default().push(edit);
    }
    for (file, mut edits) in grouped {
        let text = changed
            .get_mut(file)
            .ok_or_else(|| Error::Message("Missing edit document".into()))?;
        edits.sort_by(|a, b| {
            b.range
                .start_line_number
                .cmp(&a.range.start_line_number)
                .then(b.range.start_column.cmp(&a.range.start_column))
        });
        for edit in edits {
            let start = offset(text, edit.range.start_line_number, edit.range.start_column)?;
            let end = offset(text, edit.range.end_line_number, edit.range.end_column)?;
            if start > end {
                return Err(Error::Message("Invalid edit range".into()));
            }
            text.replace_range(start..end, &edit.text);
        }
    }
    Ok(changed)
}
fn offset(text: &str, line: u32, column: u32) -> Result<usize> {
    let mut line_start = 0;
    let mut number = 1;
    for part in text.split_inclusive('\n') {
        if number == line {
            let mut units = 1;
            for (i, c) in part.char_indices() {
                if units == column {
                    return Ok(line_start + i);
                }
                units += c.len_utf16() as u32;
            }
            if units == column {
                return Ok(line_start + part.len());
            }
            break;
        }
        line_start += part.len();
        number += 1;
    }
    if number == line && column == 1 && line_start == text.len() {
        return Ok(line_start);
    }
    Err(Error::Message("Invalid UTF-16 edit position".into()))
}
pub fn missing_imports(project: &Project, file: &str, source: &str) -> Vec<TextEdit> {
    let mut targets = std::collections::BTreeSet::new();
    let mut edits = Vec::new();
    for diagnostic in project
        .graph
        .diagnostics
        .iter()
        .filter(|d| d.code == "S004" && d.location.file == file)
    {
        if let Some(target) = diagnostic
            .suggestion
            .as_deref()
            .and_then(|s| s.strip_prefix("import "))
            && targets.insert(target.to_string())
            && let Some(symbol) = project.graph.symbols.iter().find(|s| s.qualified == target)
        {
            edits.push(import_edit(file, source, symbol));
        }
    }
    if edits.len() > 1 {
        let text = edits.iter().map(|e| e.text.as_str()).collect::<String>();
        edits.truncate(1);
        edits[0].text = text;
    }
    edits
}
pub fn references(graph: &Graph, id: &str) -> Vec<Location> {
    graph
        .edges
        .iter()
        .filter(|e| e.to == id && e.relation != "exposes")
        .map(|e| e.location.clone())
        .collect()
}
pub fn symbol_context(graph: &Graph, id: &str) -> String {
    query::query(
        graph,
        id,
        &query::QueryOptions {
            depth: 0,
            ..Default::default()
        },
    )
    .and_then(|r| query::render(&r, "markdown", 500))
    .unwrap_or_default()
}
