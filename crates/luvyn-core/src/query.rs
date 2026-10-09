use crate::{Error, Result, model::*};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryOptions {
    pub depth: usize,
    pub limit: usize,
    pub budget: usize,
}
impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            depth: 1,
            limit: 12,
            budget: 1800,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryResult {
    pub query: String,
    pub symbols: Vec<Symbol>,
    pub edges: Vec<Edge>,
    pub labels: std::collections::BTreeMap<String, String>,
    pub truncated: bool,
}
pub fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut current = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            current[j + 1] = (previous[j + 1] + 1)
                .min(current[j] + 1)
                .min(previous[j] + usize::from(ca != *cb));
        }
        previous = current;
    }
    previous[b.len()]
}
fn normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ã' | 'â' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' => 'u',
            'ç' => 'c',
            _ => c,
        })
        .collect()
}
fn mentions(text: &str, name: &str) -> bool {
    text.match_indices(name).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + name.len()..].chars().next();
        before.is_none_or(|c| !c.is_alphanumeric() && c != '_')
            && after.is_none_or(|c| !c.is_alphanumeric() && c != '_')
    })
}
pub fn search(graph: &Graph, query: &str, limit: usize) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    if let Some(i) = graph.by_id.get(query.trim()) {
        return vec![*i];
    }
    if let Some(exact) = graph.by_name.get(&q) {
        return exact.iter().copied().take(limit).collect();
    }
    let mut scores = HashMap::<usize, i32>::new();
    let query_words = words(&q);
    let stop = [
        "quem",
        "de",
        "do",
        "da",
        "o",
        "que",
        "como",
        "onde",
        "the",
        "of",
        "who",
        "how",
        "is",
        "usa",
        "depende",
        "fluxo",
        "flow",
        "dependencias",
        "export",
        "dependencies",
    ];
    for word in query_words
        .iter()
        .filter(|w| !stop.contains(&normalize(w).as_str()))
    {
        if let Some(indices) = graph.text_index.get(word) {
            for i in indices {
                *scores.entry(*i).or_default() += 25;
            }
        }
        let aliases: &[&str] = match word.as_str() {
            "usuario" | "usuarios" | "user" | "users" => &["usuario", "usuarios", "user", "users"],
            "criacao" | "criar" | "cria" | "create" | "creation" => {
                &["criacao", "criar", "cria", "create", "creation"]
            }
            _ => &[],
        };
        for alias in aliases.iter().filter(|a| **a != word.as_str()) {
            if let Some(indices) = graph.text_index.get(*alias) {
                for i in indices {
                    *scores.entry(*i).or_default() += 15;
                }
            }
        }
    }
    if words(&q)
        .iter()
        .any(|w| matches!(w.as_str(), "fluxo" | "flow" | "como" | "how"))
    {
        for key in ["__section:flow", "__section:behavior"] {
            for i in graph.text_index.get(key).into_iter().flatten() {
                if let Some(score) = scores.get_mut(i) {
                    *score += 60;
                }
            }
        }
    }
    for (i, s) in graph.symbols.iter().enumerate() {
        let name = s.name.to_lowercase();
        if mentions(&q, &name) {
            *scores.entry(i).or_default() += 200 + name.len() as i32;
        } else if name.contains(&q) || s.qualified.to_lowercase().contains(&q) {
            *scores.entry(i).or_default() += 90;
        } else if q.len() <= 80 && name.len() <= 80 {
            let d = distance(&name, &q);
            if d <= 2 {
                *scores.entry(i).or_default() += 60 - d as i32 * 10;
            }
        }
    }
    let mut ranked: Vec<_> = scores.into_iter().collect();
    ranked.sort_by(|(a, sa), (b, sb)| {
        sb.cmp(sa).then(
            graph.symbols[*a]
                .qualified
                .cmp(&graph.symbols[*b].qualified),
        )
    });
    ranked
        .into_iter()
        .filter(|(_, score)| *score > 0)
        .take(limit)
        .map(|(i, _)| i)
        .collect()
}
pub fn query(graph: &Graph, input: &str, options: &QueryOptions) -> Result<QueryResult> {
    if input.trim().is_empty() {
        return Err(Error::Message("Query cannot be empty".into()));
    }
    let limit = options.limit.clamp(1, 500);
    let depth = options.depth.min(8);
    if let Some(path) = input
        .strip_prefix("path ")
        .or_else(|| input.strip_prefix("caminho "))
    {
        let Some((a, b)) = path.split_once(" -> ").or_else(|| path.split_once(" to ")) else {
            return Err(Error::Message("Use: path Source -> Target".into()));
        };
        let start = unique_match(graph, a)?;
        let end = unique_match(graph, b)?;
        let mut queue = VecDeque::from([start.clone()]);
        let mut previous = HashMap::<String, (String, usize)>::new();
        let mut seen = HashSet::from([start.clone()]);
        while let Some(id) = queue.pop_front() {
            if id == end {
                break;
            }
            for edge in graph.outgoing.get(&id).into_iter().flatten() {
                let e = &graph.edges[*edge];
                if seen.insert(e.to.clone()) {
                    previous.insert(e.to.clone(), (id.clone(), *edge));
                    queue.push_back(e.to.clone());
                }
            }
        }
        if !seen.contains(&end) {
            return Err(Error::Message("No directed path found".into()));
        }
        let mut nodes = vec![end.clone()];
        let mut edges = Vec::new();
        let mut cursor = end;
        while cursor != start {
            let Some((p, e)) = previous.get(&cursor) else {
                break;
            };
            edges.push(graph.edges[*e].clone());
            nodes.push(p.clone());
            cursor = p.clone();
        }
        nodes.reverse();
        edges.reverse();
        if nodes.len() > limit {
            return Err(Error::Message(
                "Path exceeds --limit; increase the limit".into(),
            ));
        }
        return Ok(result(graph, input, nodes, edges, false));
    }
    let mut term = input.trim();
    let mut incoming = false;
    let mut relation: Option<&str> = None;
    let mut both = false;
    if let Some((prefix, tail)) = term.split_once(' ') {
        if let Some(r) = prefix.strip_prefix("in:") {
            relation = Some(r);
            incoming = true;
            term = tail;
        } else if let Some(r) = prefix.strip_prefix("out:") {
            relation = Some(r);
            term = tail;
        } else if prefix == "neighbors" {
            both = true;
            term = tail;
        }
    }
    let normalized = normalize(term);
    let natural = [
        ("quem depende de ", "depends", true),
        ("who depends on ", "depends", true),
        ("dependencias de ", "depends", false),
        ("dependencies of ", "depends", false),
        ("quem usa ", "*", true),
        ("onde ", "*", true),
        ("who uses ", "*", true),
        ("references to ", "references", true),
        ("o que ", "export", false),
        ("what ", "export", false),
    ];
    for (prefix, r, reverse) in natural {
        if normalized.starts_with(prefix) {
            incoming = reverse;
            relation = Some(r);
            // Prefixes contain only ASCII after accent normalization; locate symbol by indexed names below.
            let candidate = graph
                .symbols
                .iter()
                .filter(|s| {
                    normalized.contains(&normalize(&s.name))
                        || normalized.contains(&normalize(&s.qualified))
                })
                .max_by_key(|s| s.qualified.len());
            if let Some(s) = candidate {
                term = &s.qualified;
            }
            break;
        }
    }
    let matches = search(graph, term, limit);
    if matches.is_empty() {
        return Err(Error::Message(format!("No symbols match {term:?}")));
    }
    let is_exact =
        graph.by_name.contains_key(&term.to_lowercase()) || graph.by_id.contains_key(term);
    let roots: Vec<String> = matches
        .iter()
        .take(if is_exact {
            limit
        } else if words(input)
            .iter()
            .any(|w| matches!(w.as_str(), "fluxo" | "flow" | "como" | "how"))
        {
            1
        } else {
            3
        })
        .map(|i| graph.symbols[*i].id.clone())
        .collect();
    let mut seen: HashSet<String> = roots.iter().cloned().collect();
    let mut nodes = roots.clone();
    let mut queue: VecDeque<_> = roots.into_iter().map(|id| (id, 0)).collect();
    let mut edges = Vec::new();
    let mut truncated = false;
    while let Some((id, level)) = queue.pop_front() {
        if level >= depth {
            continue;
        }
        let mut incident = if incoming {
            graph.incoming.get(&id).cloned().unwrap_or_default()
        } else {
            graph.outgoing.get(&id).cloned().unwrap_or_default()
        };
        if both {
            incident.extend(graph.incoming.get(&id).into_iter().flatten().copied());
        }
        for i in incident {
            let e = &graph.edges[i];
            if relation.is_some_and(|r| r != "*" && e.relation != r) {
                continue;
            }
            let other = if e.from == id { &e.to } else { &e.from };
            if !seen.contains(other) && nodes.len() >= limit {
                truncated = true;
                continue;
            }
            edges.push(e.clone());
            if seen.insert(other.clone()) {
                nodes.push(other.clone());
                queue.push_back((other.clone(), level + 1));
            }
        }
    }
    // Include root relations as context without loading every target symbol.
    if relation.is_none() {
        for id in &nodes {
            for i in graph.outgoing.get(id).into_iter().flatten() {
                edges.push(graph.edges[*i].clone());
            }
        }
    }
    edges.sort();
    edges.dedup();
    Ok(result(graph, input, nodes, edges, truncated))
}
fn unique_match(graph: &Graph, name: &str) -> Result<String> {
    let found = search(graph, name, 2);
    if found.len() != 1 {
        return Err(Error::Message(format!(
            "Path endpoint must resolve uniquely: {name}"
        )));
    }
    Ok(graph.symbols[found[0]].id.clone())
}
fn result(
    graph: &Graph,
    input: &str,
    nodes: Vec<String>,
    edges: Vec<Edge>,
    truncated: bool,
) -> QueryResult {
    let labels = edges
        .iter()
        .flat_map(|e| [&e.from, &e.to])
        .filter_map(|id| graph.symbol(id).map(|s| (id.clone(), s.qualified.clone())))
        .collect();
    QueryResult {
        query: input.into(),
        symbols: nodes
            .iter()
            .filter_map(|id| graph.symbol(id).cloned())
            .collect(),
        edges,
        labels,
        truncated,
    }
}
pub fn compact_signature(signature: &str) -> String {
    format!(
        "func {}",
        signature
            .trim()
            .strip_prefix("func ")
            .unwrap_or(signature.trim())
    )
}
/// Compact structure, deduplicated relationships and signatures; prose is never guessed or rewritten.
pub fn render(result: &QueryResult, format: &str, budget: usize) -> Result<String> {
    render_context(result, format, Some(budget))
}
pub fn render_document(result: &QueryResult) -> Result<String> {
    render_context(result, "markdown", None)
}
fn render_context(result: &QueryResult, format: &str, budget: Option<usize>) -> Result<String> {
    if format == "json" {
        return serde_json::to_string_pretty(result).map_err(|e| Error::Message(e.to_string()));
    }
    if !matches!(format, "compact" | "text" | "markdown") {
        return Err(Error::Message(
            "Format must be compact, text, markdown or json".into(),
        ));
    }
    let mut output = String::new();
    let max_chars = budget.map_or(usize::MAX, |b| b.clamp(64, 100_000) * 4);
    let present: HashSet<_> = result.symbols.iter().map(|s| s.id.as_str()).collect();
    let mut children = std::collections::BTreeMap::<&str, Vec<&Symbol>>::new();
    let mut edges = std::collections::BTreeMap::<&str, Vec<&Edge>>::new();
    for s in &result.symbols {
        if let Some(parent) = s.parent.as_deref() {
            children.entry(parent).or_default().push(s);
        }
    }
    for e in &result.edges {
        edges.entry(&e.from).or_default().push(e);
    }
    let mut used_chars: usize = 0;
    for s in &result.symbols {
        if s.parent.as_deref().is_some_and(|p| present.contains(p)) {
            continue;
        }
        let mut block = if s.kind == "resource" {
            format!(
                "{}{}: {}\n",
                if format == "markdown" { "## " } else { "" },
                s.name,
                s.signature.as_deref().unwrap_or("<unknown schema>")
            )
        } else {
            format!(
                "{}{} {}\n",
                if format == "markdown" { "## " } else { "" },
                s.kind,
                s.qualified
            )
        };
        for annotation in &s.annotations {
            block.push_str(&format!(
                "@{}{}\n",
                annotation.name,
                if annotation.arguments.is_empty() {
                    String::new()
                } else {
                    format!("({})", annotation.arguments.join(", "))
                }
            ));
        }
        if s.signature.is_none() && !s.generics.is_empty() {
            block.push_str(&format!(
                "generics: {}\n",
                s.generics
                    .iter()
                    .map(|g| g
                        .bound
                        .as_ref()
                        .map_or(g.name.clone(), |b| format!("{}: {b}", g.name)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if let Some(signature) = &s.signature
            && s.kind != "resource"
        {
            block.push_str(&format!(
                "{} {}\n",
                if s.kind == "func" {
                    "api"
                } else {
                    "declaration"
                },
                if s.kind == "func" {
                    compact_signature(signature)
                } else {
                    signature.clone()
                }
            ));
        }
        for (key, values) in &s.sections {
            if s.kind == "resource" && key == "fields" {
                for field in values {
                    if let Some((name, value)) = field.split_once(':') {
                        block.push_str(&format!("{}={}\n", name.trim(), value.trim()));
                    }
                }
                continue;
            }
            let mut seen = HashSet::new();
            let unique: Vec<_> = values
                .iter()
                .filter(|v| seen.insert(v.as_str()))
                .cloned()
                .collect();
            block.push_str(&format!("{key}: {}\n", unique.join(" | ")));
        }
        let mut relations = std::collections::BTreeMap::<&str, Vec<&str>>::new();
        for edge in edges.get(s.id.as_str()).into_iter().flatten() {
            if s.kind == "resource" && edge.relation == "instance_of" {
                continue;
            }
            if matches!(edge.relation.as_str(), "export" | "contains")
                && present.contains(edge.to.as_str())
            {
                continue;
            }
            if let Some(label) = result.labels.get(&edge.to) {
                relations.entry(&edge.relation).or_default().push(label);
            }
        }
        for (kind, mut targets) in relations {
            targets.sort();
            targets.dedup();
            block.push_str(&format!("{kind}: {}\n", targets.join(", ")));
        }
        if let Some(apis) = children.get(s.id.as_str()) {
            for api in apis {
                if api.kind == "field" {
                    continue;
                }
                if let Some(signature) = &api.signature {
                    for annotation in &api.annotations {
                        block.push_str(&format!(
                            "{} @{}({})\n",
                            api.name,
                            annotation.name,
                            annotation.arguments.join(", ")
                        ));
                    }
                    block.push_str(&format!("api {}\n", compact_signature(signature)));
                }
                for (key, values) in &api.sections {
                    block.push_str(&format!("{}.{}: {}\n", api.name, key, values.join(" | ")));
                }
                let mut seen = HashSet::new();
                for edge in edges.get(api.id.as_str()).into_iter().flatten() {
                    if let Some(target) = result.labels.get(&edge.to) {
                        let short = target.rsplit('.').next().unwrap_or(target);
                        let derivable = matches!(edge.relation.as_str(), "uses" | "returns")
                            && api
                                .signature
                                .as_ref()
                                .is_some_and(|sig| mentions(sig, short));
                        if !derivable && seen.insert((&edge.relation, target)) {
                            block.push_str(&format!("{}.{}: {target}\n", api.name, edge.relation));
                        }
                    }
                }
            }
        }
        block.push('\n');
        let block_chars = block.chars().count();
        if used_chars.saturating_add(block_chars) > max_chars {
            if output.is_empty() {
                for line in block.lines() {
                    if output.chars().count() + line.chars().count() + 1 > max_chars {
                        break;
                    }
                    output.push_str(line);
                    output.push('\n');
                }
            }
            output.push_str("[context budget reached; narrow query or raise --budget]\n");
            break;
        }
        used_chars += block_chars;
        output.push_str(&block);
    }
    if result.truncated {
        output.push_str("[more symbols; raise --limit]\n");
    }
    Ok(output.trim_end().to_string())
}
