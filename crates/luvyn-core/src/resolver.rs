use crate::{model::*, parser::builtin};
use std::collections::{BTreeMap, HashMap, HashSet};

pub fn qualified(namespace: &str, parent: Option<&str>, name: &str) -> String {
    let local = parent.map_or_else(|| name.into(), |p| format!("{p}::{name}"));
    if namespace.is_empty() {
        local
    } else {
        format!("{namespace}.{local}")
    }
}

pub fn resolve(files: &[ParsedFile], sources: BTreeMap<String, String>) -> Graph {
    let mut graph = Graph {
        sources,
        ..Default::default()
    };
    let mut qualified_index = HashMap::<String, Vec<usize>>::new();
    let mut names = HashMap::<String, Vec<usize>>::new();
    let mut file_index = HashMap::<String, Vec<usize>>::new();
    for file in files {
        graph.diagnostics.extend(file.diagnostics.clone());
        for s in &file.symbols {
            let q = if s.kind == "main" {
                "main".into()
            } else {
                qualified(&file.namespace, s.parent.as_deref(), &s.name)
            };
            let i = graph.symbols.len();
            qualified_index.entry(q.clone()).or_default().push(i);
            names.entry(s.name.clone()).or_default().push(i);
            file_index.entry(file.path.clone()).or_default().push(i);
            graph.symbols.push(Symbol {
                id: stable_id(&q, &s.kind),
                kind: s.kind.clone(),
                name: s.name.clone(),
                qualified: q,
                namespace: if s.kind == "main" {
                    String::new()
                } else {
                    file.namespace.clone()
                },
                parent: s.parent.as_ref().map(|p| {
                    if p == "main" && file.symbols.iter().any(|a| a.kind == "main") {
                        return stable_id("main", "main");
                    }
                    stable_id(
                        &qualified(&file.namespace, None, p),
                        file.symbols
                            .iter()
                            .find(|a| &a.name == p && a.parent.is_none())
                            .map_or("class", |a| &a.kind),
                    )
                }),
                signature: s.signature.clone(),
                annotations: s.annotations.clone(),
                generics: s.generics.clone(),
                sections: s.sections.clone(),
                location: s.location.clone(),
                end_line: s.end_line,
            });
        }
    }
    // File modules are graph nodes, never arbitrary declarations inside a file.
    let mut modules = BTreeMap::new();
    for file in files {
        modules.entry(file.namespace.clone()).or_insert(file);
    }
    for (namespace, file) in modules {
        if namespace.is_empty() {
            continue;
        }
        let id = stable_id(&namespace, "module");
        let location = Location {
            file: file.path.clone(),
            line: 1,
            column: 1,
            length: 0,
        };
        for s in graph
            .symbols
            .iter()
            .filter(|s| s.namespace == namespace && s.parent.is_none() && s.kind != "main")
        {
            graph.edges.push(Edge {
                from: id.clone(),
                to: s.id.clone(),
                relation: "contains".into(),
                location: s.location.clone(),
            });
        }
        let i = graph.symbols.len();
        // main.lyn is a conventional project entry; its inferred file module
        // must not collide with the project-wide main block.
        let module_qualified =
            if namespace == "main" && graph.symbols.iter().any(|s| s.kind == "main") {
                "main::module".to_string()
            } else {
                namespace.clone()
            };
        qualified_index
            .entry(module_qualified.clone())
            .or_default()
            .push(i);
        graph.symbols.push(Symbol {
            id,
            kind: "module".into(),
            name: namespace.clone(),
            qualified: module_qualified,
            namespace,
            parent: None,
            signature: None,
            annotations: vec![],
            generics: vec![],
            sections: BTreeMap::new(),
            location,
            end_line: file.symbols.iter().map(|s| s.end_line).max().unwrap_or(1),
        });
    }
    let roots: Vec<_> = graph.symbols.iter().filter(|s| s.kind == "main").collect();
    if roots.len() > 1 {
        for root in roots {
            graph.diagnostics.push(Diagnostic::error(
                "S008",
                "project can contain only one main block",
                root.location.clone(),
            ));
        }
    }
    for indices in qualified_index.values().filter(|v| v.len() > 1) {
        for i in indices {
            let s = &graph.symbols[*i];
            graph.diagnostics.push(
                Diagnostic::error(
                    "S001",
                    format!("Duplicate symbol {}", s.qualified),
                    s.location.clone(),
                )
                .hint("Rename the symbol or use a distinct namespace"),
            );
        }
    }
    let mut cursor = 0;
    for file in files {
        let mut imported = HashMap::<String, Vec<usize>>::new();
        for import in &file.imports {
            let matches = if import.target.ends_with(".lyn") {
                let parent = std::path::Path::new(&file.path)
                    .parent()
                    .unwrap_or(std::path::Path::new(""));
                let joined = parent.join(&import.target);
                let mut normalized = Vec::new();
                for c in joined.components() {
                    match c {
                        std::path::Component::Normal(n) => {
                            normalized.push(n.to_string_lossy().to_string())
                        }
                        std::path::Component::ParentDir => {
                            normalized.pop();
                        }
                        _ => {}
                    }
                }
                file_index
                    .get(&normalized.join("/"))
                    .cloned()
                    .unwrap_or_default()
            } else if import.target.ends_with(".*") {
                graph
                    .symbols
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| {
                        s.namespace == import.target.trim_end_matches(".*") && s.parent.is_none()
                    })
                    .map(|(i, _)| i)
                    .collect()
            } else if graph
                .symbols
                .iter()
                .any(|s| s.kind == "module" && s.qualified == import.target)
                && import.alias.is_none()
            {
                graph
                    .symbols
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| {
                        s.namespace == import.target && s.kind != "module" && s.parent.is_none()
                    })
                    .map(|(i, _)| i)
                    .collect()
            } else {
                qualified_index
                    .get(&import.target)
                    .cloned()
                    .unwrap_or_default()
            };
            if matches.is_empty() {
                graph.diagnostics.push(Diagnostic::error(
                    "S002",
                    format!("Import not found: {}", import.target),
                    import.location.clone(),
                ));
            }
            for i in matches {
                let key = import
                    .alias
                    .clone()
                    .unwrap_or_else(|| graph.symbols[i].name.clone());
                imported.entry(key).or_default().push(i);
            }
        }
        for parsed in &file.symbols {
            let owner = cursor;
            cursor += 1;
            if let Some(parent) = graph.symbols[owner].parent.clone() {
                graph.edges.push(Edge {
                    from: parent,
                    to: graph.symbols[owner].id.clone(),
                    relation: if parsed.kind == "field" {
                        "contains"
                    } else {
                        "export"
                    }
                    .into(),
                    location: parsed.location.clone(),
                });
            }
            for reference in &parsed.references {
                if reference.target == "self" || reference.target.starts_with("self.") {
                    let root = qualified(
                        &file.namespace,
                        None,
                        parsed.parent.as_deref().unwrap_or(&parsed.name),
                    );
                    let owner_symbol = qualified_index
                        .get(&root)
                        .filter(|v| v.len() == 1)
                        .map(|v| v[0]);
                    let compatible = owner_symbol.is_some_and(|i| {
                        crate::language::STATE_KINDS.contains(&graph.symbols[i].kind.as_str())
                    });
                    let target = if reference.target == "self" {
                        root.clone()
                    } else {
                        format!("{root}::{}", reference.target.trim_start_matches("self."))
                    };
                    let found = qualified_index
                        .get(&target)
                        .filter(|v| v.len() == 1)
                        .map(|v| v[0])
                        .filter(|i| {
                            reference.target == "self" || graph.symbols[*i].kind == "field"
                        });
                    if compatible && let Some(target) = found {
                        graph.edges.push(Edge {
                            from: graph.symbols[owner].id.clone(),
                            to: graph.symbols[target].id.clone(),
                            relation: "references".into(),
                            location: reference.location.clone(),
                        });
                    } else {
                        graph.diagnostics.push(Diagnostic::error(if compatible { "S007" } else { "S006" }, if compatible { format!("Unknown field {} on {root}", reference.target) } else { "self requires a compatible owning symbol; standalone functions/modules have no local state".into() }, reference.location.clone()).hint("Declare the field in fields: on the owning symbol"));
                    }
                    continue;
                }
                let parent_generics = parsed
                    .parent
                    .as_ref()
                    .and_then(|p| {
                        file.symbols
                            .iter()
                            .find(|s| s.name == *p && s.parent.is_none())
                    })
                    .map(|s| s.generics.as_slice())
                    .unwrap_or(&[]);
                if parsed
                    .generics
                    .iter()
                    .chain(parent_generics)
                    .any(|g| g.name == reference.target)
                {
                    continue;
                }
                if builtin(&reference.target) {
                    continue;
                }
                let local = qualified(&file.namespace, None, &reference.target);
                let mut candidates = qualified_index.get(&local).cloned().unwrap_or_default();
                if candidates.is_empty() {
                    candidates = imported.get(&reference.target).cloned().unwrap_or_default();
                }
                if candidates.is_empty() {
                    candidates = qualified_index
                        .get(&reference.target)
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|i| graph.symbols[*i].kind != "module")
                        .collect();
                }
                if candidates.is_empty() {
                    for import in &file.imports {
                        if let Some(alias) = &import.alias
                            && let Some(tail) = reference.target.strip_prefix(&format!("{alias}."))
                        {
                            candidates.extend(
                                qualified_index
                                    .get(&format!("{}.{tail}", import.target))
                                    .cloned()
                                    .unwrap_or_default(),
                            );
                        }
                    }
                }
                if candidates.is_empty() {
                    candidates = names
                        .get(&reference.target)
                        .into_iter()
                        .flatten()
                        .copied()
                        .filter(|i| {
                            graph.symbols[*i].parent.is_none()
                                && !matches!(graph.symbols[*i].kind.as_str(), "module" | "main")
                        })
                        .collect();
                }
                // A file module can share its leaf name with its declaration (User.lyn / class User).
                // Prefer the actual declaration; a namespace remains a valid dependency when no symbol matches.
                if candidates.is_empty() {
                    candidates = qualified_index
                        .get(&reference.target)
                        .cloned()
                        .unwrap_or_default();
                }
                candidates.sort();
                candidates.dedup();
                if candidates.len() == 1 {
                    let target = candidates[0];
                    graph.edges.push(Edge {
                        from: graph.symbols[owner].id.clone(),
                        to: graph.symbols[target].id.clone(),
                        relation: reference.relation.clone(),
                        location: reference.location.clone(),
                    });
                } else if candidates.len() > 1 {
                    graph.diagnostics.push(
                        Diagnostic::error(
                            "S003",
                            format!("Ambiguous reference: {}", reference.target),
                            reference.location.clone(),
                        )
                        .hint("Use a qualified name or an import alias"),
                    );
                } else {
                    let suggestion = names
                        .keys()
                        .filter_map(|name| {
                            let distance = crate::query::distance(
                                &name.to_lowercase(),
                                &reference.target.to_lowercase(),
                            );
                            (distance <= 2).then_some((distance, name))
                        })
                        .min_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)))
                        .map(|(_, n)| format!("Did you mean {n}?"));
                    let mut d = Diagnostic::error(
                        "S005",
                        format!("Unresolved reference: {}", reference.target),
                        reference.location.clone(),
                    );
                    d.suggestion = suggestion;
                    graph.diagnostics.push(d);
                }
            }
        }
    }
    validate_resources(&mut graph);
    let mut unique = HashSet::new();
    graph.edges.retain(|e| {
        unique.insert((
            e.from.clone(),
            e.to.clone(),
            e.relation.clone(),
            e.location.clone(),
        ))
    });
    graph
        .symbols
        .sort_by(|a, b| a.qualified.cmp(&b.qualified).then(a.kind.cmp(&b.kind)));
    graph.edges.sort();
    graph
        .diagnostics
        .sort_by(|a, b| a.location.cmp(&b.location).then(a.code.cmp(&b.code)));
    graph.index();
    graph
}

fn split_type(value: &str) -> (String, Vec<String>) {
    let value = value.trim();
    let Some(open) = value.find('<').filter(|_| value.ends_with('>')) else {
        return (value.into(), Vec::new());
    };
    let inner = &value[open + 1..value.len() - 1];
    let mut args = Vec::new();
    let (mut depth, mut quoted, mut start) = (0i32, None, 0usize);
    for (i, c) in inner.char_indices() {
        if let Some(q) = quoted {
            if c == q {
                quoted = None;
            }
            continue;
        }
        if matches!(c, '\'' | '"') {
            quoted = Some(c);
            continue;
        }
        match c {
            '<' | '[' | '{' => depth += 1,
            '>' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                args.push(inner[start..i].trim().into());
                start = i + 1;
            }
            _ => {}
        }
    }
    if !inner[start..].trim().is_empty() {
        args.push(inner[start..].trim().into());
    }
    (value[..open].trim().into(), args)
}

fn replace_generics(value: &str, bindings: &HashMap<String, String>) -> String {
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if !word.is_empty() {
            out.push_str(
                bindings
                    .get(word.as_str())
                    .map_or(word.as_str(), String::as_str),
            );
            word.clear();
        }
    };
    for c in value.chars() {
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

fn resolved_field_type(field: &Symbol, value: &str, graph: &Graph) -> String {
    let mut bindings = HashMap::new();
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
            bindings.insert(token, target.qualified.clone());
        }
    }
    replace_generics(value, &bindings)
}

fn matching_type<'a>(graph: &'a Graph, name: &str, namespace: &str) -> Option<&'a Symbol> {
    graph.symbols.iter().find(|s| {
        matches!(s.kind.as_str(), "class" | "enum" | "type" | "interface")
            && (s.qualified == name
                || (s.name == name
                    && (s.namespace == namespace
                        || graph
                            .symbols
                            .iter()
                            .filter(|x| {
                                x.name == name
                                    && matches!(
                                        x.kind.as_str(),
                                        "class" | "enum" | "type" | "interface"
                                    )
                            })
                            .count()
                            == 1)))
    })
}

fn quoted(value: &str) -> bool {
    if value.len() < 2 {
        return false;
    }
    if value.starts_with('"') && value.ends_with('"') {
        return serde_json::from_str::<String>(value).is_ok();
    }
    value.starts_with('\'') && value.ends_with('\'')
}

fn value_matches(value: &str, ty: &str, namespace: &str, graph: &Graph, depth: usize) -> bool {
    if depth > 16 {
        return false;
    }
    let value = value.trim();
    let (name, args) = split_type(ty);
    match name.as_str() {
        "String" | "str" => return quoted(value),
        "char" => {
            return value.len() >= 3
                && value.starts_with('\'')
                && value.ends_with('\'')
                && value[1..value.len() - 1].chars().count() == 1;
        }
        "bool" => return matches!(value, "true" | "false"),
        "i8" => return value.parse::<i8>().is_ok(),
        "i16" => return value.parse::<i16>().is_ok(),
        "i32" => return value.parse::<i32>().is_ok(),
        "i64" => return value.parse::<i64>().is_ok(),
        "i128" => return value.parse::<i128>().is_ok(),
        "isize" => return value.parse::<isize>().is_ok(),
        "u8" => return value.parse::<u8>().is_ok(),
        "u16" => return value.parse::<u16>().is_ok(),
        "u32" => return value.parse::<u32>().is_ok(),
        "u64" => return value.parse::<u64>().is_ok(),
        "u128" => return value.parse::<u128>().is_ok(),
        "usize" => return value.parse::<usize>().is_ok(),
        "f32" => return value.parse::<f32>().is_ok_and(f32::is_finite),
        "f64" => return value.parse::<f64>().is_ok_and(f64::is_finite),
        "()" => return value == "()",
        "Option" if args.len() == 1 => {
            if matches!(value, "null" | "None") {
                return true;
            }
            if let Some(inner) = value
                .strip_prefix("Some(")
                .and_then(|v| v.strip_suffix(')'))
            {
                return value_matches(inner, &args[0], namespace, graph, depth + 1);
            }
            return value_matches(value, &args[0], namespace, graph, depth + 1);
        }
        "Vec" if args.len() == 1 => {
            if !value.starts_with('[') || !value.ends_with(']') {
                return false;
            }
            let inner = &value[1..value.len() - 1];
            if inner.trim().is_empty() {
                return true;
            }
            return split_type(&format!("Tuple<{inner}>"))
                .1
                .iter()
                .all(|v| value_matches(v, &args[0], namespace, graph, depth + 1));
        }
        "HashMap" if args.len() == 2 => {
            if !value.starts_with('{') || !value.ends_with('}') {
                return false;
            }
            let inner = &value[1..value.len() - 1];
            if inner.trim().is_empty() {
                return true;
            }
            return split_type(&format!("Tuple<{inner}>"))
                .1
                .iter()
                .all(|entry| {
                    entry.split_once(':').is_some_and(|(k, v)| {
                        value_matches(k, &args[0], namespace, graph, depth + 1)
                            && value_matches(v, &args[1], namespace, graph, depth + 1)
                    })
                });
        }
        "Result" if args.len() == 2 => {
            if let Some(inner) = value.strip_prefix("Ok(").and_then(|v| v.strip_suffix(')')) {
                return value_matches(inner, &args[0], namespace, graph, depth + 1);
            }
            if let Some(inner) = value.strip_prefix("Err(").and_then(|v| v.strip_suffix(')')) {
                return value_matches(inner, &args[1], namespace, graph, depth + 1);
            }
            return false;
        }
        _ => {}
    }
    let Some(symbol) = matching_type(graph, &name, namespace) else {
        return false;
    };
    if symbol.kind == "enum" {
        return symbol.sections.get("values").is_some_and(|values| {
            values.iter().any(|v| {
                v.trim().trim_start_matches("- ") == value
                    || (quoted(value)
                        && value.trim_matches(['\'', '"']) == v.trim().trim_start_matches("- "))
            })
        });
    }
    if symbol.kind == "type" {
        let Some(signature) = symbol.signature.as_deref() else {
            return false;
        };
        let Some((left, right)) = signature.split_once('=') else {
            return false;
        };
        let (_, parameters) = split_type(left.trim());
        if parameters.len() != args.len() {
            return false;
        }
        let bindings: HashMap<_, _> = parameters.into_iter().zip(args).collect();
        return value_matches(
            &value,
            &replace_generics(right.trim(), &bindings),
            namespace,
            graph,
            depth + 1,
        );
    }
    if symbol.kind == "class" {
        return graph.symbols.iter().any(|candidate| {
            if candidate.kind != "resource" || candidate.name != value {
                return false;
            }
            let (_, actual_args) = split_type(candidate.signature.as_deref().unwrap_or(""));
            actual_args == args
                && graph.edges.iter().any(|e| {
                    e.from == candidate.id && e.to == symbol.id && e.relation == "instance_of"
                })
        });
    }
    false
}

fn validate_resources(graph: &mut Graph) {
    let resources: Vec<_> = graph
        .symbols
        .iter()
        .filter(|s| s.kind == "resource")
        .cloned()
        .collect();
    for resource in resources {
        let schema_edge = graph
            .edges
            .iter()
            .find(|e| e.from == resource.id && e.relation == "instance_of");
        let schema = schema_edge
            .and_then(|e| graph.symbols.iter().find(|s| s.id == e.to))
            .cloned();
        let Some(schema) = schema.filter(|s| s.kind == "class") else {
            graph.diagnostics.push(Diagnostic::error(
                "S010",
                format!(
                    "Resource schema {} must resolve to a class",
                    resource.signature.as_deref().unwrap_or("<missing>")
                ),
                resource.location.clone(),
            ));
            continue;
        };
        let (_, type_args) = split_type(resource.signature.as_deref().unwrap_or(""));
        if type_args.len() != schema.generics.len() {
            graph.diagnostics.push(Diagnostic::error(
                "S011",
                format!(
                    "Resource {} supplies {} type arguments; {} expects {}",
                    resource.name,
                    type_args.len(),
                    schema.name,
                    schema.generics.len()
                ),
                resource.location.clone(),
            ));
            continue;
        }
        let bindings: HashMap<_, _> = schema
            .generics
            .iter()
            .map(|p| p.name.clone())
            .zip(type_args)
            .collect();
        let schema_fields: Vec<_> = graph
            .symbols
            .iter()
            .filter(|s| s.kind == "field" && s.parent.as_deref() == Some(schema.id.as_str()))
            .cloned()
            .collect();
        let resource_fields: Vec<_> = graph
            .symbols
            .iter()
            .filter(|s| s.kind == "field" && s.parent.as_deref() == Some(resource.id.as_str()))
            .cloned()
            .collect();
        let mut field_edges = Vec::new();
        for value_field in resource_fields {
            let Some((_, value)) = value_field
                .signature
                .as_deref()
                .unwrap_or("")
                .split_once(':')
            else {
                continue;
            };
            let Some(field) = schema_fields.iter().find(|f| f.name == value_field.name) else {
                graph.diagnostics.push(Diagnostic::error(
                    "S012",
                    format!(
                        "Unknown field {} on resource {}",
                        value_field.name, resource.name
                    ),
                    value_field.location.clone(),
                ));
                continue;
            };
            let expected = field
                .signature
                .as_deref()
                .and_then(|s| s.split_once(':').map(|x| x.1.trim()))
                .unwrap_or("");
            field_edges.push(Edge {
                from: value_field.id.clone(),
                to: field.id.clone(),
                relation: "sets".into(),
                location: value_field.location.clone(),
            });
            let expected = replace_generics(expected, &bindings);
            let expected = resolved_field_type(field, &expected, graph);
            if !value_matches(value, &expected, &resource.namespace, graph, 0) {
                graph.diagnostics.push(Diagnostic::error("S013", format!("Value for {} does not match field type {expected}", value_field.name), value_field.location.clone()).hint("Use a value with the declared field type, such as a quoted string, number, boolean, enum value or typed list"));
                continue;
            }
            let (type_name, _) = split_type(&expected);
            if let Some(target) = matching_type(graph, &type_name, &resource.namespace) {
                if target.kind == "enum" {
                    field_edges.push(Edge {
                        from: value_field.id.clone(),
                        to: target.id.clone(),
                        relation: "uses".into(),
                        location: value_field.location.clone(),
                    });
                } else if target.kind == "class" {
                    if let Some(instance) = graph.symbols.iter().find(|candidate| {
                        candidate.kind == "resource"
                            && candidate.name == value.trim()
                            && graph.edges.iter().any(|e| {
                                e.from == candidate.id
                                    && e.to == target.id
                                    && e.relation == "instance_of"
                            })
                    }) {
                        field_edges.push(Edge {
                            from: value_field.id.clone(),
                            to: instance.id.clone(),
                            relation: "references".into(),
                            location: value_field.location.clone(),
                        });
                    }
                }
            }
        }
        graph.edges.extend(field_edges);
    }
}
