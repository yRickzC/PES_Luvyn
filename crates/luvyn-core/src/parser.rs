use crate::model::*;

pub const KINDS: &[&str] = &[
    "class",
    "service",
    "interface",
    "component",
    "system",
    "entity",
    "module",
    "func",
    "function",
    "event",
    "struct",
    "enum",
    "concept",
];
pub const TEXT_SECTIONS: &[&str] = &[
    "purpose",
    "rules",
    "behavior",
    "responsibilities",
    "contracts",
    "fields",
    "values",
    "source",
    "metadata",
    "notes",
    "flow",
];
pub const RELATIONS: &[&str] = &[
    "depends",
    "implements",
    "uses",
    "returns",
    "emits",
    "listens",
    "references",
    "extends",
    "related",
];

pub fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
        && s.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.'))
        && !s.contains("..")
        && !s.ends_with('.')
}
pub fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escape = false;
    for (i, c) in line.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' {
            escape = true;
            continue;
        }
        if matches!(c, '\'' | '"') {
            if quote == Some(c) {
                quote = None
            } else if quote.is_none() {
                quote = Some(c)
            }
        }
        if quote.is_none()
            && (c == '#' || line[i..].starts_with("//"))
            && (i == 0 || line[..i].ends_with(char::is_whitespace))
        {
            return &line[..i];
        }
    }
    line
}
fn loc(file: &str, line: u32, raw: &str, token: &str) -> Location {
    let byte = raw.find(token).unwrap_or(0);
    Location {
        file: file.into(),
        line,
        column: raw[..byte].chars().count() as u32 + 1,
        length: token.chars().count() as u32,
    }
}
pub fn parse(path: &str, source: &str) -> ParsedFile {
    let mut result = ParsedFile {
        path: path.into(),
        ..Default::default()
    };
    let mut current: Option<usize> = None;
    let mut section = String::new();
    let total_lines = source.lines().count() as u32;
    for (index, raw) in source.lines().enumerate() {
        let line = index as u32 + 1;
        let clean = strip_comment(raw);
        let text = clean.trim();
        if text.is_empty() {
            continue;
        }
        let indentation = raw.len() - raw.trim_start().len();
        if raw[..indentation].contains('\t') {
            result.diagnostics.push(
                Diagnostic::error(
                    "L001",
                    "Use spaces for indentation",
                    loc(path, line, raw, "\t"),
                )
                .hint("Replace tabs with four spaces"),
            );
        }
        if indentation == 0 {
            if let Some(ns) = text.strip_prefix("namespace ") {
                if !identifier(ns) || current.is_some() {
                    result.diagnostics.push(Diagnostic::error(
                        "L002",
                        "namespace must be a valid name before declarations",
                        loc(path, line, raw, ns),
                    ));
                } else {
                    result.namespace = ns.into();
                }
                continue;
            }
            if let Some(value) = text.strip_prefix("import ") {
                let (target, alias) = value
                    .split_once(" as ")
                    .map_or((value, None), |(a, b)| (a, Some(b.to_string())));
                let target = target.trim_matches(['"', '\'']);
                if target.is_empty() || alias.as_deref().is_some_and(|a| !identifier(a)) {
                    result.diagnostics.push(Diagnostic::error(
                        "L003",
                        "Invalid import",
                        loc(path, line, raw, value),
                    ));
                }
                result.imports.push(Import {
                    target: target.into(),
                    alias,
                    location: loc(path, line, raw, target),
                });
                continue;
            }
            let (first, rest) = text.split_once(' ').unwrap_or((text, ""));
            if KINDS.contains(&first) {
                if let Some(i) = current {
                    result.symbols[i].end_line = line - 1;
                }
                let name = rest
                    .split(|c: char| c == '(' || c == ':' || c.is_whitespace())
                    .next()
                    .unwrap_or("");
                if !identifier(name) {
                    result.diagnostics.push(
                        Diagnostic::error(
                            "L004",
                            "Declaration requires a valid symbol name",
                            loc(path, line, raw, rest),
                        )
                        .hint("Example: service UserService"),
                    );
                    current = None;
                    continue;
                }
                let mut symbol = ParsedSymbol {
                    kind: if first == "function" {
                        "func".into()
                    } else {
                        first.into()
                    },
                    name: name.into(),
                    location: loc(path, line, raw, name),
                    end_line: total_lines,
                    ..Default::default()
                };
                if first == "func" || first == "function" {
                    symbol.signature = Some(rest.into());
                    signature_refs(&mut symbol, rest, path, line, raw, &mut result.diagnostics);
                } else if let Some(tail) = rest.strip_prefix(name)
                    && !tail.trim().is_empty()
                {
                    let (relation, target) =
                        tail.trim().split_once(' ').unwrap_or((tail.trim(), ""));
                    if RELATIONS.contains(&relation) {
                        add_references(&mut symbol, relation, target, path, line, raw);
                    } else {
                        result.diagnostics.push(Diagnostic::error(
                            "L005",
                            "Unexpected declaration suffix",
                            loc(path, line, raw, tail.trim()),
                        ));
                    }
                }
                result.symbols.push(symbol);
                current = Some(result.symbols.len() - 1);
                section.clear();
                continue;
            }
        }
        let Some(i) = current else {
            result.diagnostics.push(
                Diagnostic::error(
                    "L006",
                    "Expected a symbol declaration",
                    loc(path, line, raw, text),
                )
                .hint("Start with service, class, interface, entity, module or func"),
            );
            continue;
        };
        if indentation == 0 {
            if let Some((key, value)) = text.split_once(':')
                && (TEXT_SECTIONS.contains(&key) || RELATIONS.contains(&key) || key == "exposes")
            {
                section = key.into();
                if !value.trim().is_empty() {
                    item(&mut result, i, &section, value.trim(), path, line, raw);
                }
                continue;
            }
            if let Some((relation, target)) = text.split_once(' ') {
                if RELATIONS.contains(&relation) {
                    add_references(&mut result.symbols[i], relation, target, path, line, raw);
                    section.clear();
                    continue;
                }
                if relation == "link"
                    && let Some((kind, target)) = target.split_once(' ')
                    && identifier(kind)
                {
                    add_references(&mut result.symbols[i], kind, target, path, line, raw);
                    continue;
                }
            }
            result.diagnostics.push(
                Diagnostic::error(
                    "L007",
                    "Unknown section or relationship",
                    loc(path, line, raw, text),
                )
                .hint("Use purpose:, rules:, behavior:, depends: or exposes:"),
            );
            section.clear();
        } else if section.is_empty() {
            result.diagnostics.push(
                Diagnostic::error(
                    "L008",
                    "Indented content requires a section",
                    loc(path, line, raw, text),
                )
                .hint("Add a section such as purpose: above this line"),
            );
        } else {
            item(
                &mut result,
                i,
                &section,
                text.trim_start_matches("- "),
                path,
                line,
                raw,
            );
        }
    }
    for symbol in &result.symbols {
        if !symbol.sections.contains_key("purpose") && symbol.parent.is_none() {
            let mut d = Diagnostic::error("L101", "Symbol has no purpose", symbol.location.clone())
                .hint("Add purpose: to explain intent");
            d.severity = "warning".into();
            result.diagnostics.push(d);
        }
    }
    result
}
fn item(
    file: &mut ParsedFile,
    owner: usize,
    section: &str,
    text: &str,
    path: &str,
    line: u32,
    raw: &str,
) {
    if RELATIONS.contains(&section) {
        add_references(&mut file.symbols[owner], section, text, path, line, raw);
    } else if section == "exposes" {
        let text = text.strip_prefix("func ").unwrap_or(text);
        let name = text.split('(').next().unwrap_or("").trim();
        if !identifier(name) || !text.contains('(') {
            file.diagnostics.push(
                Diagnostic::error("L009", "Expected API signature", loc(path, line, raw, text))
                    .hint("createUser(name: String) -> User?"),
            );
            return;
        }
        let mut child = ParsedSymbol {
            kind: "func".into(),
            name: name.into(),
            parent: Some(file.symbols[owner].name.clone()),
            signature: Some(text.into()),
            location: loc(path, line, raw, name),
            end_line: line,
            ..Default::default()
        };
        signature_refs(&mut child, text, path, line, raw, &mut file.diagnostics);
        file.symbols.push(child);
    } else {
        file.symbols[owner]
            .sections
            .entry(section.into())
            .or_default()
            .push(text.into());
        if section == "fields" {
            type_refs(
                &mut file.symbols[owner],
                text,
                "uses",
                path,
                line,
                raw,
                raw.find(text).unwrap_or(0),
            );
        }
    }
}
pub fn builtin(name: &str) -> bool {
    matches!(
        name,
        "String"
            | "ID"
            | "Bool"
            | "Boolean"
            | "Int"
            | "Integer"
            | "Float"
            | "Number"
            | "Void"
            | "Unit"
            | "Date"
            | "DateTime"
            | "Bytes"
            | "Any"
            | "Null"
            | "List"
            | "Map"
            | "Set"
            | "Result"
            | "Option"
            | "true"
            | "false"
            | "null"
    )
}
fn add_references(
    symbol: &mut ParsedSymbol,
    relation: &str,
    text: &str,
    path: &str,
    line: u32,
    raw: &str,
) {
    for target in text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
    {
        symbol.references.push(Reference {
            target: target.into(),
            relation: relation.into(),
            location: loc(path, line, raw, target),
        });
    }
}
fn type_refs(
    symbol: &mut ParsedSymbol,
    text: &str,
    relation: &str,
    path: &str,
    line: u32,
    raw: &str,
    base: usize,
) {
    let type_start = text.find(':').map_or(0, |i| i + 1);
    let type_text = &text[type_start..];
    let mut offset = 0;
    for token in type_text
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.')
        .filter(|s| !s.is_empty())
    {
        let start = type_text[offset..].find(token).unwrap_or(0) + offset;
        offset = start + token.len();
        if !builtin(token) {
            let byte = (base + type_start + start).min(raw.len());
            symbol.references.push(Reference {
                target: token.into(),
                relation: relation.into(),
                location: Location {
                    file: path.into(),
                    line,
                    column: raw[..byte].chars().count() as u32 + 1,
                    length: token.chars().count() as u32,
                },
            });
        }
    }
}
fn signature_refs(
    symbol: &mut ParsedSymbol,
    text: &str,
    path: &str,
    line: u32,
    raw: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some((_, tail)) = text.split_once('(') else {
        diagnostics.push(Diagnostic::error(
            "L010",
            "Function requires parentheses",
            loc(path, line, raw, text),
        ));
        return;
    };
    let Some((args, return_text)) = tail.rsplit_once(')') else {
        diagnostics.push(
            Diagnostic::error(
                "L010",
                "Unclosed function signature",
                loc(path, line, raw, text),
            )
            .hint("Add closing )"),
        );
        return;
    };
    if args.contains(['(', ')']) {
        diagnostics.push(Diagnostic::error(
            "L010",
            "Nested parentheses are not supported in type signatures",
            loc(path, line, raw, text),
        ));
    }
    for arg in args.split(',').filter(|a| !a.trim().is_empty()) {
        if let Some((name, _)) = arg.split_once(':') {
            if !identifier(name.trim()) {
                diagnostics.push(Diagnostic::error(
                    "L011",
                    "Invalid parameter name",
                    loc(path, line, raw, name),
                ));
            }
            type_refs(
                symbol,
                arg,
                "uses",
                path,
                line,
                raw,
                raw.find(text).unwrap_or(0)
                    + text.find('(').unwrap_or(0)
                    + 1
                    + args.find(arg).unwrap_or(0),
            );
        } else {
            diagnostics.push(
                Diagnostic::error(
                    "L011",
                    "Parameter requires a type",
                    loc(path, line, raw, arg),
                )
                .hint("name: String"),
            );
        }
    }
    let return_text = return_text.trim();
    if let Some(ret) = return_text
        .strip_prefix("->")
        .or_else(|| return_text.strip_prefix(':'))
    {
        let ret = ret.trim();
        type_refs(
            symbol,
            ret,
            "returns",
            path,
            line,
            raw,
            raw.rfind(ret).unwrap_or(0),
        );
    } else if !return_text.is_empty() {
        diagnostics.push(Diagnostic::error(
            "L012",
            "Expected -> ReturnType",
            loc(path, line, raw, return_text),
        ));
    }
}
