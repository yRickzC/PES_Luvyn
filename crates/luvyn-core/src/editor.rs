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
    let parsed = parser::parse(file, source);
    let line = import_insertion_line(source, &target.qualified);
    let already_present = parsed.imports.iter().any(|import| {
        import.target == target.qualified
            || import.target == format!("{}.*", target.namespace)
            || import.target == target.namespace
            || (import.target.ends_with(".lyn")
                && normalized_import_path(file, &import.target)
                    == target.location.file.replace('\\', "/"))
    });
    TextEdit {
        file: file.into(),
        range: Range {
            start_line_number: line,
            start_column: 1,
            end_line_number: line,
            end_column: 1,
        },
        text: if already_present {
            String::new()
        } else {
            format!("import {}\n", target.qualified)
        },
    }
}

fn import_insertion_line(source: &str, qualified: &str) -> u32 {
    let lines: Vec<_> = source.lines().collect();
    let import_lines: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            line.trim_start()
                .strip_prefix("import ")
                .map(|target| (index, target.trim()))
        })
        .collect();
    if let Some((index, _)) = import_lines.iter().find(|(_, target)| *target > qualified) {
        return *index as u32 + 1;
    }
    if let Some((index, _)) = import_lines.last() {
        return *index as u32 + 2;
    }
    lines
        .iter()
        .position(|line| line.starts_with("namespace "))
        .map_or(1, |index| index as u32 + 2)
}

fn normalized_import_path(file: &str, target: &str) -> String {
    let parent = std::path::Path::new(file)
        .parent()
        .unwrap_or(std::path::Path::new(""));
    let joined = parent.join(target.replace('\\', "/"));
    let mut parts = Vec::new();
    for component in joined.components() {
        match component {
            std::path::Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::CurDir => {}
            _ => return String::new(),
        }
    }
    parts.join("/")
}

fn imported_name(
    parsed: &ParsedFile,
    symbol: &Symbol,
    modules: &std::collections::HashSet<&str>,
) -> Option<String> {
    for import in &parsed.imports {
        if import.target == symbol.qualified {
            return Some(import.alias.clone().unwrap_or_else(|| symbol.name.clone()));
        }
        if import.target == format!("{}.*", symbol.namespace) {
            return Some(symbol.name.clone());
        }
        if import.target == symbol.namespace && modules.contains(import.target.as_str()) {
            return Some(import.alias.as_ref().map_or_else(
                || symbol.name.clone(),
                |alias| format!("{alias}.{}", symbol.name),
            ));
        }
        if import.target.ends_with(".lyn")
            && normalized_import_path(&parsed.path, &import.target)
                == symbol.location.file.replace('\\', "/")
        {
            return Some(import.alias.clone().unwrap_or_else(|| symbol.name.clone()));
        }
    }
    None
}
pub fn completions(project: &Project, file: &str, source: &str, line: u32) -> Vec<Completion> {
    completions_at(project, file, source, line, u32::MAX)
}
pub fn completions_at(
    project: &Project,
    file: &str,
    source: &str,
    line: u32,
    column: u32,
) -> Vec<Completion> {
    let parsed = project.parsed(file);
    let namespace = parsed.map_or("", |f| f.namespace.as_str());
    let current = source
        .lines()
        .nth(line.saturating_sub(1) as usize)
        .unwrap_or("");
    let prefix: String = current
        .chars()
        .scan(1u32, |units, c| {
            let before = *units;
            *units += c.len_utf16() as u32;
            Some((before, c))
        })
        .take_while(|(units, _)| *units < column)
        .map(|(_, c)| c)
        .collect();
    if file.ends_with(".resource.lyn")
        && let Some(context) = parsed.and_then(|f| {
            f.symbols
                .iter()
                .filter(|s| s.kind == "resource" && s.location.line <= line && s.end_line >= line)
                .max_by_key(|s| s.location.line)
        })
    {
        let schema_name = context
            .signature
            .as_deref()
            .unwrap_or("")
            .split('<')
            .next()
            .unwrap_or("");
        let schema = project.graph.symbols.iter().find(|s| {
            s.kind == "class"
                && s.name == schema_name
                && (s.namespace == namespace
                    || project
                        .graph
                        .symbols
                        .iter()
                        .filter(|x| x.kind == "class" && x.name == schema_name)
                        .count()
                        == 1)
        });
        let Some(schema) = schema else {
            return vec![];
        };
        let filled: std::collections::BTreeSet<_> = context
            .sections
            .get("fields")
            .into_iter()
            .flatten()
            .filter_map(|v| v.split_once(':').map(|(n, _)| n.trim().to_string()))
            .collect();
        let trimmed = prefix.trim_start();
        if let Some((field_name, value_prefix)) = trimmed.split_once(':') {
            let Some(field) = project.graph.symbols.iter().find(|s| {
                s.kind == "field"
                    && s.parent.as_deref() == Some(schema.id.as_str())
                    && s.name == field_name.trim()
            }) else {
                return vec![];
            };
            let ty = field
                .signature
                .as_deref()
                .and_then(|s| s.split_once(':').map(|x| x.1.trim()))
                .unwrap_or("");
            let (_, schema_args) =
                completion_type_parts(context.signature.as_deref().unwrap_or(""));
            let bindings: std::collections::HashMap<_, _> = schema
                .generics
                .iter()
                .map(|g| g.name.as_str())
                .zip(schema_args)
                .collect();
            let ty = replace_type_parameters(ty, &bindings);
            let ty = completion_resolved_field_type(field, &ty, &project.graph);
            let mut options = Vec::<(String, String)>::new();
            resource_value_options(&ty, namespace, &project.graph, 0, &mut options);
            let typed = value_prefix.trim();
            return options
                .into_iter()
                .filter(|(label, _)| typed.is_empty() || label.starts_with(typed))
                .map(|(label, insert)| Completion {
                    label: label.clone(),
                    detail: format!("{ty} value"),
                    insert,
                    import: None,
                    snippet: label.contains("${"),
                })
                .collect();
        }
        let in_fields = source
            .lines()
            .take(line as usize)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .find(|l| l.trim_start() == *l && l.trim_end().ends_with(':'))
            .is_some_and(|l| l.trim() == "fields:");
        if in_fields {
            return project
                .graph
                .symbols
                .iter()
                .filter(|s| {
                    s.kind == "field"
                        && s.parent.as_deref() == Some(schema.id.as_str())
                        && !filled.contains(&s.name)
                        && s.name.starts_with(trimmed)
                })
                .map(|s| Completion {
                    label: s.name.clone(),
                    detail: s.signature.clone().unwrap_or_default(),
                    insert: format!("{}: ", s.name),
                    import: None,
                    snippet: false,
                })
                .collect();
        }
        return vec![];
    }
    if let Some((_, field)) = prefix.rsplit_once("self.")
        && field.chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        let context = parsed.and_then(|f| {
            f.symbols
                .iter()
                .filter(|s| s.kind != "field" && s.location.line <= line && s.end_line >= line)
                .max_by_key(|s| s.location.line)
        });
        let Some(context) = context else {
            return vec![];
        };
        let owner = context.parent.as_deref().unwrap_or(&context.name);
        let owner_q = crate::resolver::qualified(namespace, None, owner);
        let owner_id = project
            .graph
            .symbols
            .iter()
            .find(|s| {
                s.qualified == owner_q && crate::language::STATE_KINDS.contains(&s.kind.as_str())
            })
            .map(|s| s.id.as_str());
        // Completion must work while `self.` is still incomplete and has not
        // produced a semantic reference yet. Restrict it to state-like owners.
        return project
            .graph
            .symbols
            .iter()
            .filter(|s| s.kind == "field" && owner_id.is_some() && s.parent.as_deref() == owner_id)
            .map(|s| Completion {
                label: s.name.clone(),
                detail: s.signature.clone().unwrap_or_else(|| s.qualified.clone()),
                insert: s.name.clone(),
                import: None,
                snippet: false,
            })
            .collect();
    }
    let previous = source
        .lines()
        .take(line.saturating_sub(1) as usize)
        .filter(|l| !l.trim().is_empty())
        .last()
        .unwrap_or("");
    let interfaces = current.trim().starts_with("implements") || previous.trim() == "implements:";
    let mut output = Vec::new();
    if let Some(context) = parsed.and_then(|f| {
        f.symbols
            .iter()
            .filter(|s| s.kind != "field" && s.location.line <= line && s.end_line >= line)
            .max_by_key(|s| s.location.line)
    }) {
        let owner = context.parent.as_ref().and_then(|name| {
            parsed?
                .symbols
                .iter()
                .find(|s| s.name == *name && s.parent.is_none())
        });
        let mut seen = std::collections::BTreeSet::new();
        for parameter in context
            .generics
            .iter()
            .chain(owner.into_iter().flat_map(|s| &s.generics))
        {
            if seen.insert(&parameter.name) {
                output.push(Completion {
                    label: parameter.name.clone(),
                    detail: parameter
                        .bound
                        .as_ref()
                        .map_or("Generic parameter".into(), |b| {
                            format!("Generic parameter: {b}")
                        }),
                    insert: parameter.name.clone(),
                    import: None,
                    snippet: false,
                });
            }
        }
    }
    let mut name_counts = std::collections::HashMap::new();
    let mut local_name_counts = std::collections::HashMap::new();
    for s in project
        .graph
        .symbols
        .iter()
        .filter(|s| s.parent.is_none() && !matches!(s.kind.as_str(), "module" | "main"))
    {
        *name_counts.entry(s.name.as_str()).or_insert(0usize) += 1;
        if s.namespace == namespace {
            *local_name_counts.entry(s.name.as_str()).or_insert(0usize) += 1;
        }
    }
    let modules: std::collections::HashSet<_> = project
        .graph
        .symbols
        .iter()
        .filter(|s| s.kind == "module")
        .map(|s| s.qualified.as_str())
        .collect();
    let mut imported_binding_counts = std::collections::HashMap::new();
    if let Some(parsed) = parsed {
        for candidate in project
            .graph
            .symbols
            .iter()
            .filter(|s| s.parent.is_none() && !matches!(s.kind.as_str(), "module" | "main"))
        {
            if let Some(binding) = imported_name(parsed, candidate, &modules) {
                *imported_binding_counts.entry(binding).or_insert(0usize) += 1;
            }
        }
    }
    let in_import = current.trim_start().starts_with("import ");
    let qualified_prefix = prefix.trim_end().ends_with('.');
    for s in project.graph.symbols.iter().filter(|s| {
        s.parent.is_none()
            && !matches!(s.kind.as_str(), "module" | "main")
            && (!interfaces || s.kind == "interface")
    }) {
        let same = namespace == s.namespace;
        let imported = parsed.and_then(|f| imported_name(f, s, &modules));
        let imported_conflict = imported.as_ref().is_some_and(|binding| {
            let local_collision = binding == &s.name
                && local_name_counts.get(s.name.as_str()).copied().unwrap_or(0) > 0;
            imported_binding_counts.get(binding).copied().unwrap_or(0) > 1 || local_collision
        });
        let ambiguous = !same
            && (imported_conflict
                || (imported.is_none()
                    && name_counts.get(s.name.as_str()).copied().unwrap_or(0) > 1));
        let import = if !in_import && !same && imported.is_none() && !qualified_prefix && !ambiguous
        {
            Some(import_edit(file, source, s))
        } else {
            None
        };
        let insert = if in_import {
            s.qualified.clone()
        } else if same {
            s.name.clone()
        } else if ambiguous {
            s.qualified.clone()
        } else if let Some(name) = imported {
            name
        } else if qualified_prefix {
            s.name.clone()
        } else {
            s.name.clone()
        };
        output.push(Completion {
            label: s.name.clone(),
            detail: format!("{} {}", s.kind, s.qualified),
            insert,
            import,
            snippet: false,
        });
    }
    for e in crate::language::dictionary() {
        output.push(Completion {
            label: e.keyword.clone(),
            detail: e.description.clone(),
            insert: e.completion.clone(),
            import: None,
            snippet: e.completion.contains("${"),
        });
    }
    output
}

fn completion_type_parts(ty: &str) -> (&str, Vec<&str>) {
    let ty = ty.trim();
    let Some(open) = ty.find('<').filter(|_| ty.ends_with('>')) else {
        return (ty, Vec::new());
    };
    let inner = &ty[open + 1..ty.len() - 1];
    let (mut depth, mut start, mut args) = (0i32, 0usize, Vec::new());
    for (i, c) in inner.char_indices() {
        match c {
            '<' | '[' | '{' => depth += 1,
            '>' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                args.push(inner[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    if !inner[start..].trim().is_empty() {
        args.push(inner[start..].trim());
    }
    (ty[..open].trim(), args)
}

fn replace_type_parameters<K>(ty: &str, bindings: &std::collections::HashMap<K, &str>) -> String
where
    K: std::borrow::Borrow<str> + Eq + std::hash::Hash,
{
    let mut output = String::new();
    let mut token = String::new();
    for c in ty.chars() {
        if c.is_alphanumeric() || c == '_' {
            token.push(c);
        } else {
            if !token.is_empty() {
                output.push_str(
                    bindings
                        .get(token.as_str())
                        .copied()
                        .unwrap_or(token.as_str()),
                );
                token.clear();
            }
            output.push(c);
        }
    }
    if !token.is_empty() {
        output.push_str(
            bindings
                .get(token.as_str())
                .copied()
                .unwrap_or(token.as_str()),
        );
    }
    output
}

fn completion_resolved_field_type(field: &Symbol, ty: &str, graph: &Graph) -> String {
    let mut owned = std::collections::HashMap::new();
    for edge in graph
        .edges
        .iter()
        .filter(|e| e.from == field.id && e.relation == "uses")
    {
        let Some(target) = graph.symbols.iter().find(|s| s.id == edge.to) else {
            continue;
        };
        let offset = edge.location.column.saturating_sub(field.location.column) as usize;
        let token: String = field
            .signature
            .as_deref()
            .unwrap_or("")
            .chars()
            .skip(offset)
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !token.is_empty() {
            owned.insert(token, target.qualified.as_str());
        }
    }
    replace_type_parameters(ty, &owned)
}

fn resource_value_options(
    ty: &str,
    namespace: &str,
    graph: &Graph,
    depth: usize,
    out: &mut Vec<(String, String)>,
) {
    if depth > 8 {
        return;
    }
    let ty = ty.trim();
    match ty {
        "bool" => {
            out.push(("true".into(), "true".into()));
            out.push(("false".into(), "false".into()));
            return;
        }
        "String" | "str" => {
            out.push(("\"text\"".into(), "\"${1:text}\"".into()));
            return;
        }
        "char" => {
            out.push(("'x'".into(), "'${1:x}'".into()));
            return;
        }
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128"
        | "usize" => {
            for n in ["0", "1", "42"] {
                out.push((n.into(), n.into()));
            }
            return;
        }
        "f32" | "f64" => {
            for n in ["0.0", "1.0", "2.5"] {
                out.push((n.into(), n.into()));
            }
            return;
        }
        _ => {}
    }
    let (name, args) = completion_type_parts(ty);
    if !args.is_empty() {
        match (name, args.as_slice()) {
            ("Option", [inner]) => {
                out.push(("null".into(), "null".into()));
                resource_value_options(inner, namespace, graph, depth + 1, out);
                return;
            }
            ("Vec", [inner]) => {
                out.push(("[]".into(), "[${1}]".into()));
                let mut elements = Vec::new();
                resource_value_options(inner, namespace, graph, depth + 1, &mut elements);
                for (label, insert) in elements {
                    out.push((format!("[{label}]"), format!("[{insert}]")));
                }
                return;
            }
            ("HashMap", [key, value]) => {
                out.push(("{}".into(), "{${1}}".into()));
                let mut keys = Vec::new();
                let mut values = Vec::new();
                resource_value_options(key, namespace, graph, depth + 1, &mut keys);
                resource_value_options(value, namespace, graph, depth + 1, &mut values);
                if let (Some((key, key_insert)), Some((value, value_insert))) =
                    (keys.first(), values.first())
                {
                    out.push((
                        format!("{{{key}: {value}}}"),
                        format!("{{{key_insert}: {value_insert}}}"),
                    ));
                }
                return;
            }
            _ => {}
        }
    }
    if let Some(symbol) = graph.symbols.iter().find(|s| {
        matches!(s.kind.as_str(), "class" | "enum" | "type")
            && (s.name == name || s.qualified == name)
            && (s.namespace == namespace
                || s.qualified == name
                || graph
                    .symbols
                    .iter()
                    .filter(|x| {
                        x.name == name && matches!(x.kind.as_str(), "class" | "enum" | "type")
                    })
                    .count()
                    == 1)
    }) {
        if symbol.kind == "type" {
            if let Some(signature) = symbol.signature.as_deref()
                && let Some((left, right)) = signature.split_once('=')
            {
                let (_, parameters) = completion_type_parts(left.trim());
                if parameters.len() == args.len() {
                    let bindings: std::collections::HashMap<_, _> =
                        parameters.into_iter().zip(args).collect();
                    let expanded = replace_type_parameters(right.trim(), &bindings);
                    resource_value_options(&expanded, namespace, graph, depth + 1, out);
                }
            }
        } else if symbol.kind == "enum" {
            for value in symbol.sections.get("values").into_iter().flatten() {
                let value = value.trim().trim_start_matches("- ");
                out.push((value.into(), value.into()));
            }
        } else {
            for instance in graph.symbols.iter().filter(|s| {
                if s.kind != "resource" {
                    return false;
                }
                let (actual, actual_args) =
                    completion_type_parts(s.signature.as_deref().unwrap_or(""));
                actual == symbol.name && actual_args == args
            }) {
                out.push((instance.name.clone(), instance.name.clone()));
            }
        }
    }
}
pub fn symbol_at<'a>(
    graph: &'a Graph,
    file: &str,
    line: u32,
    column: u32,
    source: &str,
) -> Option<&'a Symbol> {
    for s in graph
        .symbols
        .iter()
        .filter(|s| s.location.file == file && s.kind != "module")
    {
        let r = range(&s.location, source);
        if line == r.start_line_number && column >= r.start_column && column <= r.end_column {
            if s.kind == "field"
                && s.parent
                    .as_deref()
                    .and_then(|parent| graph.symbol(parent))
                    .is_some_and(|p| p.kind == "resource")
                && let Some(definition) = graph
                    .edges
                    .iter()
                    .find(|e| e.from == s.id && e.relation == "sets")
                    .and_then(|e| graph.symbol(&e.to))
            {
                return Some(definition);
            }
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
    if word == "self" || word.starts_with("self.") {
        let owner = graph
            .symbols
            .iter()
            .filter(|s| {
                s.parent.is_none()
                    && s.location.file == file
                    && s.location.line <= line
                    && s.end_line >= line
                    && crate::language::STATE_KINDS.contains(&s.kind.as_str())
            })
            .max_by_key(|s| s.location.line)?;
        if word == "self" || byte < left + 5 {
            return Some(owner);
        }
        return graph.symbols.iter().find(|s| {
            s.kind == "field"
                && s.parent.as_deref() == Some(&owner.id)
                && s.name == word.trim_start_matches("self.")
        });
    }
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
    if symbol.kind == "module" {
        return Err(Error::Message(
            "File modules are renamed by moving the document, or editing its explicit override"
                .into(),
        ));
    }
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
        .filter(|e| e.to == id && !matches!(e.relation.as_str(), "export" | "contains"))
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
            if old == "self" {
                continue;
            }
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
    let parsed: Vec<_> = changed
        .iter()
        .map(|(f, s)| project.parse_document(f, s))
        .collect();
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
        .filter(|e| e.to == id && !matches!(e.relation.as_str(), "export" | "contains"))
        .map(|e| e.location.clone())
        .collect()
}
pub fn symbol_context(graph: &Graph, id: &str) -> String {
    if let Some(field) = graph.symbol(id).filter(|s| s.kind == "field") {
        let owner = field
            .parent
            .as_deref()
            .and_then(|id| graph.symbol(id))
            .map_or("", |s| s.name.as_str());
        return format!(
            "field {}\nowner: {}",
            field.signature.as_deref().unwrap_or(&field.name),
            owner
        );
    }
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
