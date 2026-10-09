use crate::model::*;

pub use crate::language::{KINDS, RELATIONS, TEXT_SECTIONS};

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
fn resource_header(rest: &str) -> Option<(String, String)> {
    let mut parts = rest.trim().splitn(2, char::is_whitespace);
    let schema = parts.next()?.trim();
    let identity = parts.next()?.trim();
    let id = identity.strip_prefix('"')?.strip_suffix('"')?;
    (!schema.is_empty() && !id.is_empty() && !id.contains('"')).then(|| (schema.into(), id.into()))
}
fn valid_resource_schema(schema: &str) -> bool {
    let Some(open) = schema.find('<') else {
        return identifier(schema);
    };
    if !schema.ends_with('>') || !identifier(schema[..open].trim()) {
        return false;
    }
    let inner = &schema[open + 1..schema.len() - 1];
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, c) in inner.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            ',' if depth == 0 => {
                if !valid_resource_schema(inner[start..i].trim()) {
                    return false;
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    depth == 0 && !inner.is_empty() && valid_resource_schema(inner[start..].trim())
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
    parse_in_module(path, source, &module_name(path))
}
/// Relative to a configured source root; consecutive repeated segments collapse.
pub fn module_name(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let stem = normalized
        .strip_suffix(".resource.lyn")
        .or_else(|| normalized.strip_suffix(".lyn"))
        .unwrap_or(&normalized);
    let mut parts = Vec::new();
    for part in stem.split('/').filter(|p| !p.is_empty() && *p != ".") {
        if parts.last().copied() != Some(part) {
            parts.push(part);
        }
    }
    parts.join(".")
}
pub fn parse_in_module(path: &str, source: &str, module: &str) -> ParsedFile {
    let resource_document = path.ends_with(".resource.lyn");
    let mut result = ParsedFile {
        path: path.into(),
        namespace: module.into(),
        ..Default::default()
    };
    let mut current: Option<usize> = None;
    let mut section = String::new();
    let mut method: Option<usize> = None;
    let mut annotations = Vec::new();
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
        if text.starts_with('@') && (indentation == 0 || section == "export") {
            if resource_document {
                result.diagnostics.push(Diagnostic::error(
                    "L130",
                    "Annotations are not allowed in resource documents",
                    loc(path, line, raw, text),
                ));
                continue;
            }
            match parse_annotation(text, path, line, raw) {
                Ok(annotation) => annotations.push(annotation),
                Err(message) => result.diagnostics.push(Diagnostic::error(
                    "L020",
                    message,
                    loc(path, line, raw, text),
                )),
            }
            continue;
        }
        if indentation == 0 {
            method = None;
            if text == "main:" {
                if resource_document {
                    result.diagnostics.push(Diagnostic::error(
                        "L135",
                        "`main:` is not allowed in a resource document",
                        loc(path, line, raw, "main"),
                    ));
                    continue;
                }
                if let Some(i) = current {
                    result.symbols[i].end_line = line - 1;
                }
                result.symbols.push(ParsedSymbol {
                    kind: "main".into(),
                    name: "main".into(),
                    location: loc(path, line, raw, "main"),
                    end_line: total_lines,
                    ..Default::default()
                });
                current = Some(result.symbols.len() - 1);
                section.clear();
                continue;
            }
            if let Some(declaration) = text.strip_prefix("module ") {
                let (name, explicit_override) = declaration
                    .strip_suffix(" override")
                    .map_or((declaration, false), |name| (name, true));
                if current.is_none() && identifier(name) && (name == module || explicit_override) {
                    result.namespace = name.into();
                    continue;
                }
                let mut diagnostic = Diagnostic::error("L102", "module declaration no longer matches file module semantics", loc(path, line, raw, "module"))
                    .hint("Remove obsolete module declarations; use module <name> override before declarations for an explicit module override");
                diagnostic.severity = "warning".into();
                result.diagnostics.push(diagnostic);
            }
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
                if resource_document {
                    if first != "class" {
                        result.diagnostics.push(Diagnostic::error(
                            "L131", "Resource documents may contain only `class Schema \"id\"` declarations", loc(path, line, raw, first),
                        ));
                        current = None;
                        continue;
                    }
                    let Some((schema, identity)) = resource_header(rest) else {
                        result.diagnostics.push(
                            Diagnostic::error(
                                "L132",
                                "Resource declaration requires a schema and quoted identity",
                                loc(path, line, raw, rest),
                            )
                            .hint("Use: class Weapon \"iron_sword\""),
                        );
                        current = None;
                        continue;
                    };
                    if !identifier(&identity) || !valid_resource_schema(&schema) {
                        result.diagnostics.push(
                            Diagnostic::error(
                                "L132",
                                "Invalid resource schema or identity",
                                loc(path, line, raw, rest),
                            )
                            .hint(
                                "Use a class name, optional generic types, and a quoted identifier",
                            ),
                        );
                        current = None;
                        continue;
                    }
                    if let Some(i) = current {
                        result.symbols[i].end_line = line - 1;
                    }
                    let mut symbol = ParsedSymbol {
                        kind: "resource".into(),
                        name: identity,
                        signature: Some(schema.clone()),
                        location: loc(path, line, raw, &schema),
                        end_line: total_lines,
                        ..Default::default()
                    };
                    let target = schema.split('<').next().unwrap_or(&schema).trim();
                    symbol.references.push(Reference {
                        target: target.into(),
                        relation: "instance_of".into(),
                        location: loc(path, line, raw, target),
                    });
                    result.symbols.push(symbol);
                    current = Some(result.symbols.len() - 1);
                    section.clear();
                    continue;
                }
                if let Some(i) = current {
                    result.symbols[i].end_line = line - 1;
                }
                let name = rest
                    .split(|c: char| matches!(c, '(' | ':' | '<' | '=') || c.is_whitespace())
                    .next()
                    .unwrap_or("");
                if !identifier(name) {
                    result.diagnostics.push(
                        Diagnostic::error(
                            "L004",
                            "Declaration requires a valid symbol name",
                            loc(path, line, raw, rest),
                        )
                        .hint("Example: class UserService"),
                    );
                    current = None;
                    continue;
                }
                let mut symbol = ParsedSymbol {
                    kind: first.into(),
                    annotations: std::mem::take(&mut annotations),
                    name: name.into(),
                    location: loc(path, line, raw, name),
                    end_line: total_lines,
                    ..Default::default()
                };
                let tail = declaration_generics(
                    &mut symbol,
                    rest,
                    path,
                    line,
                    raw,
                    &mut result.diagnostics,
                );
                if !symbol.generics.is_empty() {
                    symbol.signature = Some(rest.into());
                }
                if first == "func" {
                    symbol.signature = Some(rest.into());
                    signature_refs(&mut symbol, rest, path, line, raw, &mut result.diagnostics);
                } else if first == "type" && tail.trim_start().starts_with('=') {
                    let ty = tail.trim_start().strip_prefix('=').unwrap_or("").trim();
                    symbol.signature = Some(rest.into());
                    validate_type(ty, path, line, raw, &mut result.diagnostics);
                    type_refs(
                        &mut symbol,
                        ty,
                        "uses",
                        path,
                        line,
                        raw,
                        raw.rfind(ty).unwrap_or(0),
                    );
                } else if !tail.trim().is_empty() {
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
        if indentation == 0 {
            let old = text
                .split(|c: char| c.is_whitespace() || c == ':')
                .next()
                .unwrap_or("");
            if let Some(replacement) = migration(old) {
                result.diagnostics.push(
                    Diagnostic::error(
                        "L120",
                        format!("Removed keyword {old}"),
                        loc(path, line, raw, old),
                    )
                    .hint(replacement),
                );
                continue;
            }
        }
        let indentation =
            if current.is_some_and(|i| result.symbols[i].kind == "main") && indentation >= 4 {
                indentation - 4
            } else {
                indentation
            };
        let Some(i) = current else {
            result.diagnostics.push(
                Diagnostic::error(
                    "L006",
                    "Expected a symbol declaration",
                    loc(path, line, raw, text),
                )
                .hint("Start with class, func, type, enum, interface or main:"),
            );
            continue;
        };
        if indentation == 0 {
            if let Some((key, value)) = text.split_once(':')
                && (TEXT_SECTIONS.contains(&key) || RELATIONS.contains(&key) || key == "export")
            {
                if resource_document && !matches!(key, "purpose" | "fields" | "notes") {
                    result.diagnostics.push(
                        Diagnostic::error(
                            "L133",
                            format!("`{key}` is not allowed in a resource document"),
                            loc(path, line, raw, key),
                        )
                        .hint("Resource documents support purpose:, fields: and notes:"),
                    );
                    section.clear();
                    continue;
                }
                section = key.into();
                if !value.trim().is_empty() {
                    item(&mut result, i, &section, value.trim(), path, line, raw);
                }
                continue;
            }
            if let Some((relation, target)) = text.split_once(' ')
                && RELATIONS.contains(&relation)
            {
                if resource_document {
                    result.diagnostics.push(Diagnostic::error(
                        "L133",
                        format!("`{relation}` is not allowed in a resource document"),
                        loc(path, line, raw, relation),
                    ));
                    section.clear();
                    continue;
                }
                add_references(&mut result.symbols[i], relation, target, path, line, raw);
                section.clear();
                continue;
            }
            result.diagnostics.push(
                Diagnostic::error(
                    "L007",
                    "Unknown section or relationship",
                    loc(path, line, raw, text),
                )
                .hint("Use purpose:, rules:, behavior:, depends: or export:"),
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
            if section == "export" && indentation >= 8 {
                if let Some(child) = method {
                    let content = text.strip_prefix("behavior:").unwrap_or(text).trim();
                    file_description(&mut result.symbols[child], content, path, line, raw);
                    result.symbols[child].end_line = line;
                } else {
                    result.diagnostics.push(Diagnostic::error(
                        "L013",
                        "Method description requires a preceding signature indented four spaces",
                        loc(path, line, raw, text),
                    ));
                }
                continue;
            }
            let before = result.symbols.len();
            item(
                &mut result,
                i,
                &section,
                text.trim_start_matches("- "),
                path,
                line,
                raw,
            );
            if section == "export" && result.symbols.len() > before {
                method = Some(result.symbols.len() - 1);
                result.symbols.last_mut().unwrap().annotations = std::mem::take(&mut annotations);
            }
        }
    }
    if let Some(annotation) = annotations.first() {
        result.diagnostics.push(Diagnostic::error(
            "L020",
            "Annotation requires a following declaration",
            annotation.location.clone(),
        ));
    }
    for symbol in &result.symbols {
        if !symbol.sections.contains_key("purpose") && symbol.parent.is_none() {
            let mut d = Diagnostic::error("L101", "Symbol has no purpose", symbol.location.clone())
                .hint("Add purpose: to explain intent");
            d.severity = "warning".into();
            result.diagnostics.push(d);
        }
    }
    if !identifier(&result.namespace) {
        result.diagnostics.push(
            Diagnostic::error(
                "L018",
                "File module requires valid identifier segments",
                Location {
                    file: path.into(),
                    line: 1,
                    column: 1,
                    length: 0,
                },
            )
            .hint("Rename the file/path or add module <valid.name> override before declarations"),
        );
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
    if file.symbols[owner].kind == "resource" {
        file.symbols[owner]
            .sections
            .entry(section.into())
            .or_default()
            .push(text.into());
        if section == "fields" {
            let Some((name, value)) = text.split_once(':') else {
                file.diagnostics.push(Diagnostic::error(
                    "L134",
                    "Resource field requires name: value",
                    loc(path, line, raw, text),
                ));
                return;
            };
            let name = name.trim();
            let value = value.trim();
            if !identifier(name) || name.contains('.') || value.is_empty() {
                file.diagnostics.push(Diagnostic::error(
                    "L134",
                    "Invalid resource field; expected name: value",
                    loc(path, line, raw, text),
                ));
                return;
            }
            file.symbols.push(ParsedSymbol {
                kind: "field".into(),
                name: name.into(),
                parent: Some(file.symbols[owner].name.clone()),
                signature: Some(format!("{name}: {value}")),
                location: loc(path, line, raw, name),
                end_line: line,
                ..Default::default()
            });
        }
        return;
    }
    if RELATIONS.contains(&section) {
        add_references(&mut file.symbols[owner], section, text, path, line, raw);
    } else if section == "export" {
        if !text.starts_with("func ") {
            add_references(&mut file.symbols[owner], "export", text, path, line, raw);
            return;
        }
        let text = text.strip_prefix("func ").unwrap_or(text);
        let name = text.split(['(', '<']).next().unwrap_or("").trim();
        if !identifier(name) || !text.contains('(') {
            file.diagnostics.push(
                Diagnostic::error("L009", "Expected API signature", loc(path, line, raw, text))
                    .hint("func createUser(name: String) -> Option<User>"),
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
        declaration_generics(&mut child, text, path, line, raw, &mut file.diagnostics);
        signature_refs(&mut child, text, path, line, raw, &mut file.diagnostics);
        file.symbols.push(child);
    } else {
        file.symbols[owner]
            .sections
            .entry(section.into())
            .or_default()
            .push(text.into());
        if section == "fields" {
            if !crate::language::STATE_KINDS.contains(&file.symbols[owner].kind.as_str()) {
                file.diagnostics.push(Diagnostic::error(
                    "L014",
                    "fields requires class, type or interface",
                    loc(path, line, raw, text),
                ));
            }
            let Some((name, ty)) = text.split_once(':') else {
                file.diagnostics.push(Diagnostic::error(
                    "L015",
                    "Field requires name: Type",
                    loc(path, line, raw, text),
                ));
                return;
            };
            let name = name.trim();
            if !identifier(name) || name.contains('.') || ty.trim().is_empty() {
                file.diagnostics.push(Diagnostic::error(
                    "L015",
                    "Invalid field declaration; expected name: Type",
                    loc(path, line, raw, name),
                ));
                return;
            }
            let mut field = ParsedSymbol {
                kind: "field".into(),
                name: name.into(),
                parent: Some(file.symbols[owner].name.clone()),
                signature: Some(text.into()),
                location: loc(path, line, raw, name),
                end_line: line,
                ..Default::default()
            };
            validate_type(ty.trim(), path, line, raw, &mut file.diagnostics);
            type_refs(
                &mut field,
                text,
                "uses",
                path,
                line,
                raw,
                raw.find(text).unwrap_or(0),
            );
            file.symbols.push(field);
        } else {
            self_refs(
                &mut file.symbols[owner],
                section,
                text,
                path,
                line,
                raw,
                &mut file.diagnostics,
            );
        }
    }
}
pub fn builtin(name: &str) -> bool {
    crate::language::BUILTINS.contains(&name)
}

fn migration(keyword: &str) -> Option<String> {
    match keyword {
        "entity" | "component" | "service" | "system" | "event" | "concept" | "struct" => {
            Some(format!("Use @{keyword} followed by class Name"))
        }
        "function" => Some("Use func".into()),
        "exposes" | "functions" => Some("Use export: with func signatures".into()),
        "responsibilities" => Some("Move responsibilities to purpose:".into()),
        "emits" | "listens" => Some("Use @event classes and depends:/export:".into()),
        "uses" | "returns" | "references" | "related" | "link" => {
            Some("Use depends: for consumption; uses/returns are derived from signatures".into())
        }
        "metadata" => Some("Use @annotations or notes:".into()),
        "flow" => Some("Move flow descriptions to behavior:".into()),
        _ => None,
    }
}
fn parse_annotation(text: &str, path: &str, line: u32, raw: &str) -> Result<Annotation, String> {
    let body = &text[1..];
    let name = body.split('(').next().unwrap_or("").trim();
    if !identifier(name) {
        return Err("Annotation requires @name with optional arguments".into());
    }
    let tail = body[name.len()..].trim();
    let arguments = if tail.is_empty() {
        vec![]
    } else {
        let inner = tail
            .strip_prefix('(')
            .and_then(|s| s.strip_suffix(')'))
            .ok_or("Unclosed annotation; expected @name(args)")?;
        let mut args = Vec::new();
        let (mut quote, mut escaped, mut start, mut depth) = (None, false, 0, 0i32);
        for (i, c) in inner.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if quote.is_some() && c == '\\' {
                escaped = true;
                continue;
            }
            if matches!(c, '\'' | '"') {
                if quote == Some(c) {
                    quote = None;
                } else if quote.is_none() {
                    quote = Some(c);
                }
                continue;
            }
            if quote.is_some() {
                continue;
            }
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                ',' if depth == 0 => {
                    args.push(inner[start..i].trim().to_owned());
                    start = i + 1;
                }
                _ => {}
            }
            if depth < 0 {
                return Err("Unbalanced annotation arguments".into());
            }
        }
        if quote.is_some() || depth != 0 {
            return Err("Unclosed annotation string or argument".into());
        }
        if !inner.trim().is_empty() {
            args.push(inner[start..].trim().to_owned());
        }
        if args.iter().any(String::is_empty) {
            return Err("Empty annotation argument".into());
        }
        args
    };
    Ok(Annotation {
        name: name.into(),
        arguments,
        location: loc(path, line, raw, text),
    })
}
fn declaration_generics<'a>(
    symbol: &mut ParsedSymbol,
    text: &'a str,
    path: &str,
    line: u32,
    raw: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> &'a str {
    let tail = text.strip_prefix(&symbol.name).unwrap_or(text).trim_start();
    if !tail.starts_with('<') {
        return tail;
    }
    let mut depth = 0;
    let mut end = None;
    for (i, c) in tail.char_indices() {
        if c == '<' {
            depth += 1;
        } else if c == '>' {
            depth -= 1;
            if depth == 0 {
                end = Some(i);
                break;
            }
        }
    }
    let Some(end) = end else {
        diagnostics.push(Diagnostic::error(
            "L021",
            "Unclosed generic parameters; expected >",
            loc(path, line, raw, tail),
        ));
        return "";
    };
    let mut names = std::collections::HashSet::new();
    if tail[1..end].trim().is_empty() {
        diagnostics.push(Diagnostic::error(
            "L021",
            "Generic parameters cannot be empty",
            loc(path, line, raw, tail),
        ));
    }
    for part in type_arguments(&tail[1..end]) {
        let (name, bound) = part
            .split_once(':')
            .map_or((part, None), |(n, b)| (n, Some(b.trim())));
        let name = name.trim();
        if !identifier(name)
            || name.contains('.')
            || builtin(name)
            || !names.insert(name.to_owned())
        {
            diagnostics.push(Diagnostic::error(
                "L021",
                "Generic parameter must be a unique local name",
                loc(path, line, raw, name),
            ));
            continue;
        }
        if let Some(bound) = bound {
            validate_type(bound, path, line, raw, diagnostics);
            type_refs(
                symbol,
                bound,
                "bound",
                path,
                line,
                raw,
                raw.find(bound).unwrap_or(0),
            );
        }
        symbol.generics.push(GenericParameter {
            name: name.into(),
            bound: bound.map(str::to_owned),
            location: loc(path, line, raw, name),
        });
    }
    tail[end + 1..].trim_start()
}

fn file_description(symbol: &mut ParsedSymbol, text: &str, path: &str, line: u32, raw: &str) {
    symbol
        .sections
        .entry("behavior".into())
        .or_default()
        .push(text.into());
    self_refs(symbol, "behavior", text, path, line, raw, &mut Vec::new());
}
fn self_refs(
    symbol: &mut ParsedSymbol,
    section: &str,
    text: &str,
    path: &str,
    line: u32,
    raw: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let base = raw.find(text).unwrap_or(0);
    let mut quoted = None;
    let mut escaped = false;
    let chars: Vec<_> = text.char_indices().collect();
    for (n, &(byte, c)) in chars.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if matches!(c, '\'' | '"') {
            if quoted == Some(c) {
                quoted = None;
            } else if quoted.is_none() {
                quoted = Some(c);
            }
            continue;
        }
        if quoted.is_some()
            || !text[byte..].starts_with("self")
            || (n > 0 && (chars[n - 1].1.is_alphanumeric() || matches!(chars[n - 1].1, '_' | '.')))
        {
            continue;
        }
        let tail = &text[byte + 4..];
        if tail
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            continue;
        }
        let (target, start, len) = if let Some(rest) = tail.strip_prefix('.') {
            let field: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            (
                format!("self.{field}"),
                byte + 5,
                field.chars().count().max(1),
            )
        } else {
            ("self".into(), byte, 4)
        };
        let location = Location {
            file: path.into(),
            line,
            column: raw[..base + start].chars().count() as u32 + 1,
            length: len as u32,
        };
        if !crate::language::SELF_SECTIONS.contains(&section) {
            diagnostics.push(Diagnostic::error(
                "L016",
                format!("self is not allowed in {section}"),
                location,
            ));
        } else {
            symbol.references.push(Reference {
                target,
                relation: "references".into(),
                location,
            });
        }
    }
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
    let type_start = text
        .find(':')
        .filter(|i| !text[*i..].starts_with("::"))
        .map_or(0, |i| i + 1);
    let type_text = &text[type_start..];
    let mut offset = 0;
    for token in type_text
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.' && c != ':')
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
/// Documentary Rust-like type AST. No executable Rust semantics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeExpr {
    Unit,
    Named(String),
    Generic(String, Vec<TypeExpr>),
}
impl std::fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unit => write!(f, "()"),
            Self::Named(name) => write!(f, "{name}"),
            Self::Generic(name, arguments) => write!(
                f,
                "{name}<{}>",
                arguments
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}
pub fn normalized_signature(text: &str) -> Option<String> {
    let open = text.find('(')?;
    let name = text[..open].trim();
    if !identifier(name.split('<').next().unwrap_or(name).trim()) {
        return None;
    }
    let mut depth = 0;
    let mut close = None;
    for (offset, c) in text[open..].char_indices() {
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
            if depth == 0 {
                close = Some(open + offset);
                break;
            }
        }
    }
    let close = close?;
    let mut parameters = Vec::new();
    for argument in type_arguments(&text[open + 1..close])
        .into_iter()
        .filter(|a| !a.trim().is_empty())
    {
        let (name, ty) = argument.split_once(':')?;
        if !identifier(name.trim()) {
            return None;
        }
        parameters.push(format!(
            "{}: {}",
            name.trim(),
            TypeExpr::parse(ty.trim()).ok()?
        ));
    }
    let tail = text[close + 1..].trim();
    let returns = if tail.is_empty() {
        String::new()
    } else {
        format!(
            " -> {}",
            TypeExpr::parse(tail.strip_prefix("->")?.trim()).ok()?
        )
    };
    Some(format!("{name}({}){returns}", parameters.join(", ")))
}
impl TypeExpr {
    pub fn parse(text: &str) -> std::result::Result<Self, String> {
        fn whitespace(input: &mut &str) {
            *input = input.trim_start();
        }
        fn read(input: &mut &str) -> std::result::Result<TypeExpr, String> {
            whitespace(input);
            if let Some(rest) = input.strip_prefix("()") {
                *input = rest;
                return Ok(TypeExpr::Unit);
            }
            let end = input
                .find(|c: char| !c.is_alphanumeric() && !matches!(c, '_' | '.' | ':'))
                .unwrap_or(input.len());
            let name = &input[..end];
            if !identifier(&name.replace("::", "."))
                || crate::language::LITERALS.contains(&name)
                || name == "null"
            {
                return Err("Expected a documentary type (bool literals are not types)".into());
            }
            let name = name.to_string();
            *input = &input[end..];
            whitespace(input);
            let mut node = TypeExpr::Named(name.clone());
            if let Some(rest) = input.strip_prefix('<') {
                *input = rest;
                let mut arguments = Vec::new();
                loop {
                    if arguments.len() >= 64 {
                        return Err("Too many generic arguments".into());
                    }
                    arguments.push(read(input)?);
                    whitespace(input);
                    if let Some(rest) = input.strip_prefix('>') {
                        *input = rest;
                        break;
                    }
                    if let Some(rest) = input.strip_prefix(',') {
                        *input = rest;
                    } else {
                        return Err("Expected , or > in generic type".into());
                    }
                }
                if let Some((_, arity)) = crate::language::GENERICS.iter().find(|(n, _)| *n == name)
                    && arguments.len() != *arity
                {
                    return Err(format!("{name} requires {arity} type arguments"));
                }
                if builtin(&name) && !crate::language::GENERICS.iter().any(|(n, _)| *n == name) {
                    return Err(format!("{name} is not a generic type"));
                }
                node = TypeExpr::Generic(name, arguments);
            } else if crate::language::GENERICS.iter().any(|(n, _)| *n == name) {
                return Err(format!("{name} requires type arguments"));
            }
            Ok(node)
        }
        // Bound recursion and allocations for untrusted editor buffers.
        if text.len() > 4096 || text.matches('<').count() > 32 {
            return Err("Type nesting/length limit exceeded".into());
        }
        let mut input = text;
        let value = read(&mut input)?;
        if !input.trim().is_empty() {
            return Err("Unexpected type suffix; use Option<T> for optional values".into());
        }
        Ok(value)
    }
}
fn validate_type(text: &str, path: &str, line: u32, raw: &str, diagnostics: &mut Vec<Diagnostic>) {
    let check = if let Some(old) = text.strip_suffix('?') {
        let mut d = Diagnostic::error(
            "L103",
            format!("`{text}` is deprecated; use `Option<{old}>`"),
            loc(path, line, raw, text),
        )
        .hint(format!("Option<{old}>"));
        d.severity = "warning".into();
        diagnostics.push(d);
        old
    } else {
        text
    };
    if let Err(message) = TypeExpr::parse(check) {
        diagnostics.push(Diagnostic::error(
            "L017",
            message,
            loc(path, line, raw, text),
        ));
    }
}
/// Split commas only outside generic/unit delimiters.
fn type_arguments(text: &str) -> Vec<&str> {
    let mut depth = 0i32;
    let mut start = 0;
    let mut values = Vec::new();
    for (i, c) in text.char_indices() {
        match c {
            '<' | '(' => depth += 1,
            '>' | ')' => depth -= 1,
            ',' if depth == 0 => {
                values.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    values.push(&text[start..]);
    values
}
fn signature_refs(
    symbol: &mut ParsedSymbol,
    text: &str,
    path: &str,
    line: u32,
    raw: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(open) = text.find('(') else {
        diagnostics.push(Diagnostic::error(
            "L010",
            "Function requires parentheses",
            loc(path, line, raw, text),
        ));
        return;
    };
    let mut depth = 0;
    let mut close = None;
    for (i, c) in text[open..].char_indices() {
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
            if depth == 0 {
                close = Some(open + i);
                break;
            }
        }
    }
    let Some(close) = close else {
        diagnostics.push(Diagnostic::error(
            "L010",
            "Unclosed function signature",
            loc(path, line, raw, text),
        ));
        return;
    };
    let args = &text[open + 1..close];
    let mut offset = 0;
    for arg in type_arguments(args)
        .into_iter()
        .filter(|a| !a.trim().is_empty())
    {
        let start = args[offset..].find(arg).unwrap_or(0) + offset;
        offset = start + arg.len();
        if let Some((name, ty)) = arg.split_once(':') {
            if !identifier(name.trim()) || name.trim().contains('.') {
                diagnostics.push(Diagnostic::error(
                    "L011",
                    "Invalid parameter name",
                    loc(path, line, raw, name),
                ));
            }
            validate_type(ty.trim(), path, line, raw, diagnostics);
            type_refs(
                symbol,
                arg,
                "uses",
                path,
                line,
                raw,
                raw.find(text).unwrap_or(0) + open + 1 + start,
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
    let return_text = text[close + 1..].trim();
    if let Some(ret) = return_text
        .strip_prefix("->")
        .or_else(|| return_text.strip_prefix(':'))
    {
        if return_text.starts_with(':') {
            let mut d = Diagnostic::error(
                "L104",
                "Return : is deprecated; use ->",
                loc(path, line, raw, return_text),
            )
            .hint(format!("->{}", ret));
            d.severity = "warning".into();
            diagnostics.push(d);
        }
        let ret = ret.trim();
        validate_type(ret, path, line, raw, diagnostics);
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
