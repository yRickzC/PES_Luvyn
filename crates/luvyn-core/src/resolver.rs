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
            let q = qualified(&file.namespace, s.parent.as_deref(), &s.name);
            let i = graph.symbols.len();
            qualified_index.entry(q.clone()).or_default().push(i);
            names.entry(s.name.clone()).or_default().push(i);
            file_index.entry(file.path.clone()).or_default().push(i);
            graph.symbols.push(Symbol {
                id: stable_id(&q, &s.kind),
                kind: s.kind.clone(),
                name: s.name.clone(),
                qualified: q,
                namespace: file.namespace.clone(),
                parent: s.parent.as_ref().map(|p| {
                    stable_id(
                        &qualified(&file.namespace, None, p),
                        file.symbols
                            .iter()
                            .find(|a| &a.name == p && a.parent.is_none())
                            .map_or("class", |a| &a.kind),
                    )
                }),
                signature: s.signature.clone(),
                sections: s.sections.clone(),
                location: s.location.clone(),
                end_line: s.end_line,
            });
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
                    relation: "exposes".into(),
                    location: parsed.location.clone(),
                });
            }
            for reference in &parsed.references {
                if builtin(&reference.target) {
                    continue;
                }
                let mut candidates = qualified_index
                    .get(&reference.target)
                    .cloned()
                    .unwrap_or_default();
                if candidates.is_empty() {
                    let q = qualified(&file.namespace, None, &reference.target);
                    candidates = qualified_index.get(&q).cloned().unwrap_or_default();
                }
                if candidates.is_empty() {
                    candidates = imported.get(&reference.target).cloned().unwrap_or_default();
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
                } else if let Some(global) = names.get(&reference.target).filter(|v| v.len() == 1) {
                    let target = &graph.symbols[global[0]];
                    graph.diagnostics.push(
                        Diagnostic::error(
                            "S004",
                            format!("Missing import for {}", reference.target),
                            reference.location.clone(),
                        )
                        .hint(format!("import {}", target.qualified)),
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
