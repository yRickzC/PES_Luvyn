use crate::{
    Error, Result,
    model::*,
    query::{QueryResult, render_document},
};
use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
    path::Path,
};
use zip::{ZipWriter, write::SimpleFileOptions};

pub fn bytes(graph: &Graph) -> Result<Vec<u8>> {
    if graph.has_errors() {
        return Err(Error::Message("Fix diagnostics before export".into()));
    }
    let mut modules = BTreeMap::<String, Vec<Symbol>>::new();
    for symbol in &graph.symbols {
        modules
            .entry(symbol.namespace.clone())
            .or_default()
            .push(symbol.clone());
    }
    let module_key = |namespace: &str| namespace.to_string();
    let mut module_edges = BTreeMap::<String, Vec<Edge>>::new();
    for e in &graph.edges {
        if let Some(owner) = graph.symbol(&e.from) {
            module_edges
                .entry(module_key(&owner.namespace))
                .or_default()
                .push(e.clone());
        }
    }
    let mut documents = BTreeMap::new();
    let mut index = String::from(
        "# Luvyn index\n\nNames are relative to each namespace; owner::API identifies exposed functions.\n\n",
    );
    for (module, mut symbols) in modules {
        let name = if module.is_empty() {
            "GLOBAL.md".into()
        } else {
            format!("modules/{}.md", module.replace('.', "/"))
        };
        index.push_str(&format!(
            "[{}]({name})\n",
            if module.is_empty() {
                "(global)"
            } else {
                &module
            }
        ));
        let mut apis = BTreeMap::<&str, Vec<&str>>::new();
        for s in &symbols {
            if let Some(parent) = s.parent.as_deref() {
                apis.entry(parent).or_default().push(&s.name);
            }
        }
        for s in symbols.iter().filter(|s| s.parent.is_none()) {
            index.push_str(&format!("{} {}", s.kind, s.name));
            if let Some(names) = apis.get(s.id.as_str()) {
                index.push_str(&format!(" {{{}}}", names.join(", ")));
            }
            index.push('\n');
        }
        index.push('\n');
        let namespace = symbols
            .first()
            .map_or("", |s| s.namespace.as_str())
            .to_string();
        let prefix = format!("{namespace}.");
        let edges = module_edges.remove(&module).unwrap_or_default();
        let labels = edges
            .iter()
            .flat_map(|e| [&e.from, &e.to])
            .filter_map(|id| {
                graph.symbol(id).map(|s| {
                    (
                        id.clone(),
                        if !namespace.is_empty() {
                            s.qualified
                                .strip_prefix(&prefix)
                                .unwrap_or(&s.qualified)
                                .to_string()
                        } else {
                            s.qualified.clone()
                        },
                    )
                })
            })
            .collect();
        if !namespace.is_empty() {
            for s in &mut symbols {
                s.qualified = s
                    .qualified
                    .strip_prefix(&prefix)
                    .unwrap_or(&s.qualified)
                    .to_string();
            }
        }
        let context = QueryResult {
            query: module.clone(),
            symbols,
            edges,
            labels,
            truncated: false,
        };
        // Export has no retrieval budget; exposed APIs are grouped under their owner.
        let document = format!(
            "# namespace {}\n\n{}\n",
            if namespace.is_empty() {
                "(global)"
            } else {
                &namespace
            },
            render_document(&context)?
        );
        documents.insert(name, document);
    }
    documents.insert("INDEX.md".into(), index);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    for (name, content) in documents {
        zip.start_file(name, options)
            .map_err(|e| Error::Message(e.to_string()))?;
        zip.write_all(content.as_bytes())?;
    }
    Ok(zip
        .finish()
        .map_err(|e| Error::Message(e.to_string()))?
        .into_inner())
}
pub fn write(path: &Path, graph: &Graph) -> Result<()> {
    crate::workspace::atomic_write(path, &bytes(graph)?)
}
