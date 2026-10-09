//! Shared IDE session; host-independent semantic state, filesystem and actions.
use crate::{
    Error, Project, Result, editor,
    query::{self, QueryOptions},
    workspace::{atomic_write, read_source},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
struct Session {
    project: Project,
    overlays: BTreeMap<String, String>,
    dirty: bool,
}
pub struct IdeCore {
    session: Mutex<Session>,
    pub external: AtomicBool,
    pub revision: AtomicU64,
    pub building: AtomicBool,
    pub build_logs: Mutex<Vec<String>>,
}
impl IdeCore {
    pub fn with_saved_workspace<T>(&self, action: impl FnOnce() -> Result<T>) -> Result<T> {
        let session = self
            .session
            .lock()
            .map_err(|_| Error::Message("Workspace session unavailable".into()))?;
        if self.building.load(Ordering::Acquire) || !session.overlays.is_empty() {
            return Err(Error::Message(
                "Save documents and wait for Build before synchronization".into(),
            ));
        }
        action()
    }
    pub fn open_workspace(&self, root: &std::path::Path) -> Result<()> {
        self.open_workspace_with_target(root, None)
    }
    pub fn open_workspace_with_target(
        &self,
        root: &std::path::Path,
        target: Option<&std::path::Path>,
    ) -> Result<()> {
        let mut session = self
            .session
            .lock()
            .map_err(|_| Error::Message("Workspace session unavailable".into()))?;
        if self.building.load(Ordering::Acquire) || !session.overlays.is_empty() {
            return Err(Error::Message(
                "Save documents and wait for Build before switching projects".into(),
            ));
        }
        let mut project = Project::open_with_target(root, target)?;
        project.analyze(&BTreeMap::new())?;
        session.project = project;
        session.dirty = false;
        self.external.store(false, Ordering::Relaxed);
        self.revision.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    pub fn new(project: Project) -> Self {
        Self {
            session: Mutex::new(Session {
                project,
                overlays: BTreeMap::new(),
                dirty: false,
            }),
            external: AtomicBool::new(false),
            revision: AtomicU64::new(1),
            building: AtomicBool::new(false),
            build_logs: Mutex::new(vec![]),
        }
    }
    pub fn overlays_empty(&self) -> bool {
        self.session.lock().unwrap().overlays.is_empty()
    }
}
fn string<'a>(request: &'a Value, key: &str) -> Result<&'a str> {
    request
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Message(format!("Missing {key}")))
}
fn sources(session: &Session) -> Result<BTreeMap<String, String>> {
    let mut sources = BTreeMap::new();
    for file in session.project.discover()? {
        let text = session
            .overlays
            .get(&file)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| read_source(&session.project.safe_path(&file)?));
        if let Ok(text) = text {
            sources.insert(file, text);
        }
    }
    sources.extend(
        session
            .overlays
            .iter()
            .filter(|(file, _)| {
                file.ends_with(".lyn")
                    && session.project.is_source(file)
                    && !session.project.is_ignored(file, false).unwrap_or(true)
            })
            .map(|(file, text)| (file.clone(), text.clone())),
    );
    Ok(sources)
}
fn analyze(session: &mut Session) -> Result<()> {
    if session.dirty {
        session.project.analyze(&session.overlays)?;
        session.dirty = false;
    }
    Ok(())
}
fn protected(path: &str) -> Result<()> {
    if path
        .split(['/', '\\'])
        .any(|p| matches!(p, ".luvyn" | ".git" | "target" | "node_modules"))
    {
        return Err(Error::Message(
            "Generated/internal directories are protected".into(),
        ));
    }
    Ok(())
}
pub fn handle(state: &IdeCore, request: Value) -> Result<Value> {
    let op = string(&request, "op")?;
    if op == "language" {
        return Ok(json!({"entries":crate::language::dictionary()}));
    }
    if op == "tokens" {
        return Ok(json!({"tokens": crate::lexer::tokenize(string(&request, "text")?)}));
    }
    if op == "build-status" {
        return Ok(
            json!({"running":state.building.load(Ordering::Acquire),"logs":*state.build_logs.lock().map_err(|_|Error::Message("Build status unavailable".into()))?}),
        );
    }
    struct BuildGuard<'a>(&'a IdeCore);
    impl Drop for BuildGuard<'_> {
        fn drop(&mut self) {
            self.0.building.store(false, Ordering::Release);
            self.0.revision.fetch_add(1, Ordering::Relaxed);
        }
    }
    let _guard = if matches!(op, "build" | "export") {
        state
            .building
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::Message("A build/export is already running".into()))?;
        state
            .build_logs
            .lock()
            .map_err(|_| Error::Message("Build status unavailable".into()))?
            .clear();
        Some(BuildGuard(state))
    } else {
        None
    };
    let mut session = state
        .session
        .lock()
        .map_err(|_| Error::Message("Workspace session unavailable".into()))?;
    if state.external.swap(false, Ordering::Relaxed) {
        session.dirty = true;
    }
    match op {
        "snapshot" => {
            if !session.dirty
                && request.get("revision").and_then(Value::as_u64)
                    == Some(state.revision.load(Ordering::Relaxed))
            {
                return Ok(json!({"unchanged":true}));
            }
            analyze(&mut session)?;
            let files = session.project.discover()?;
            let mut entries = Vec::new();
            for file in files {
                let path = session.project.safe_path(&file)?;
                let disk = read_source(&path).ok();
                entries.push(json!({"path":file,"hash":disk.as_ref().map(|s|blake3::hash(s.as_bytes()).to_hex().to_string())}));
            }
            let docs = sources(&session)?;
            let mut configs = Vec::new();
            for file in [".ignore.luvyn", "luvyn.toml"] {
                if let Ok(text) = read_source(&session.project.safe_path(file)?) {
                    configs.push(json!({"path":file,"hash":blake3::hash(text.as_bytes()).to_hex().to_string()}));
                }
            }
            let diagnostics:Vec<_>=session.project.graph.diagnostics.iter().map(|d|json!({"diagnostic":d,"range":editor::range(&d.location,docs.get(&d.location.file).map_or("",String::as_str))})).collect();
            let symbols: Vec<_> = session
                .project
                .graph
                .symbols
                .iter()
                .take(10_000)
                .map(|s| json!(s))
                .collect();
            let mut folders = std::collections::BTreeSet::new();
            let walk = session.project.walk(&session.project.root)?;
            for entry in walk.build().flatten().take(20_000) {
                if entry.file_type().is_some_and(|t| t.is_dir())
                    && let Ok(p) = entry.path().strip_prefix(&session.project.root)
                {
                    let p = p.to_string_lossy().replace('\\', "/");
                    if !p.is_empty() && session.project.is_source(&p) {
                        folders.insert(p);
                    }
                }
            }
            Ok(
                json!({"workspace":session.project.root.display().to_string(),"documentation_root":session.project.root.display().to_string(),"target_project_root":session.project.target_project_root.display().to_string(),"artifact_root":session.project.artifact_root.display().to_string(),"source_roots":session.project.config.sources,"files":entries,"configs":configs,"folders":folders,"diagnostics":diagnostics,"symbols":symbols,"symbol_count":session.project.graph.symbols.len(),"edge_count":session.project.graph.edges.len(),"stats":session.project.stats,"revision":state.revision.load(Ordering::Relaxed),"autosave":session.project.config.autosave}),
            )
        }
        "ignore-config" => {
            use std::io::Write;
            let path = session.project.safe_path(".ignore.luvyn")?;
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
            {
                Ok(mut file) => {
                    file.write_all(b"# Luvyn ignore patterns, relative to the documentation workspace.\n# generated/\n# *.generated.lyn\n")?;
                    file.sync_all()?;
                    session.dirty = true;
                    state.revision.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
            Ok(json!({"ok":true,"file":".ignore.luvyn"}))
        }
        "file" => {
            let file = string(&request, "file")?;
            protected(file)?;
            let path = session.project.safe_path(file)?;
            let text = read_source(&path)?;
            Ok(json!({"text":text,"hash":blake3::hash(text.as_bytes()).to_hex().to_string()}))
        }
        "edit" => {
            let file = string(&request, "file")?;
            protected(file)?;
            let existing = session.project.safe_path(file)?;
            if !existing.is_file() {
                return Err(Error::Message(
                    "Document does not exist; create the real file first".into(),
                ));
            }
            if !session.project.is_source(file) || session.project.is_ignored(file, false)? {
                return Err(Error::Message(
                    "Document is outside source roots or ignored by .ignore.luvyn".into(),
                ));
            }
            let text = string(&request, "text")?;
            if text.len() > crate::workspace::MAX_SOURCE {
                return Err(Error::Message("Document exceeds 2 MiB".into()));
            }
            let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
            let changed = session.project.graph.sources.get(file) != Some(&hash);
            let disk = read_source(&session.project.safe_path(file)?).ok();
            if disk.as_deref() == Some(text) {
                session.overlays.remove(file);
            } else {
                session.overlays.insert(file.into(), text.into());
            }
            if changed {
                session.dirty = true;
                analyze(&mut session)?;
                state.revision.fetch_add(1, Ordering::Relaxed);
            }
            Ok(json!({"ok":true}))
        }
        "save" => {
            let file = string(&request, "file")?;
            protected(file)?;
            let path = session.project.safe_path(file)?;
            let text = string(&request, "text")?;
            if text.len() > crate::workspace::MAX_SOURCE {
                return Err(Error::Message("Document exceeds 2 MiB".into()));
            }
            if !path.is_file() {
                return Err(Error::Message(
                    "Document was removed; restore or create it before saving".into(),
                ));
            }
            let disk = read_source(&path).ok();
            let hash = disk
                .as_ref()
                .map(|s| blake3::hash(s.as_bytes()).to_hex().to_string());
            if request.get("hash").and_then(Value::as_str) != hash.as_deref() {
                return Err(Error::Message(
                    "File changed externally. Reload or review the conflict before saving".into(),
                ));
            }
            atomic_write(&path, text.as_bytes())?;
            session.overlays.remove(file);
            session.dirty = true;
            state.revision.fetch_add(1, Ordering::Relaxed);
            Ok(json!({"hash":blake3::hash(text.as_bytes()).to_hex().to_string()}))
        }
        "discard" => {
            session.overlays.remove(string(&request, "file")?);
            session.dirty = true;
            Ok(json!({"ok":true}))
        }
        "create" | "mkdir" => {
            let file = string(&request, "file")?;
            protected(file)?;
            let path = session.project.safe_path(file)?;
            if path.exists() {
                return Err(Error::Message("Path already exists".into()));
            }
            if op == "mkdir" {
                if !session.project.is_source(file) || session.project.is_ignored(file, true)? {
                    return Err(Error::Message(
                        "Choose a folder under a configured source root, not an ignored path"
                            .into(),
                    ));
                }
                std::fs::create_dir_all(path)?;
            } else {
                if !session.project.is_source(file) || session.project.is_ignored(file, false)? {
                    return Err(Error::Message(
                        "Choose a path under a configured source root, not an ignored path".into(),
                    ));
                }
                if !file.ends_with(".lyn") {
                    return Err(Error::Message("New documents must end in .lyn".into()));
                }
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut handle = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?;
                use std::io::Write;
                handle.write_all(
                    request
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .as_bytes(),
                )?;
                handle.sync_all()?;
                drop(handle);
                let text = read_source(&path)?;
                session.dirty = true;
                state.revision.fetch_add(1, Ordering::Relaxed);
                return Ok(
                    json!({"ok":true,"file":file,"text":text,"hash":blake3::hash(text.as_bytes()).to_hex().to_string(),"documentation_root":session.project.root}),
                );
            }
            session.dirty = true;
            state.revision.fetch_add(1, Ordering::Relaxed);
            Ok(json!({"ok":true}))
        }
        "move" => {
            let from = string(&request, "file")?;
            let to = string(&request, "to")?;
            protected(from)?;
            protected(to)?;
            if session
                .overlays
                .keys()
                .any(|f| f == from || f.starts_with(&format!("{from}/")))
            {
                return Err(Error::Message(
                    "Save modified documents before moving".into(),
                ));
            }
            let from_path = session.project.safe_path(from)?;
            let to_path = session.project.safe_path(to)?;
            if to_path.exists() || to_path.starts_with(&from_path) {
                return Err(Error::Message(
                    "Move destination exists or is inside the source".into(),
                ));
            }
            if let Some(parent) = to_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(from_path, to_path)?;
            session.dirty = true;
            state.revision.fetch_add(1, Ordering::Relaxed);
            Ok(json!({"ok":true}))
        }
        "delete" => {
            let file = string(&request, "file")?;
            protected(file)?;
            if session
                .overlays
                .keys()
                .any(|f| f == file || f.starts_with(&format!("{file}/")))
            {
                return Err(Error::Message(
                    "Save or discard modified documents before deleting".into(),
                ));
            }
            let path = session.project.safe_path(file)?;
            if path == session.project.root {
                return Err(Error::Message("Workspace root cannot be deleted".into()));
            }
            if path.is_dir() {
                if request.get("recursive").and_then(Value::as_bool) == Some(true) {
                    std::fs::remove_dir_all(path)?;
                } else {
                    std::fs::remove_dir(path)?;
                }
            } else {
                std::fs::remove_file(path)?;
            }
            session.dirty = true;
            state.revision.fetch_add(1, Ordering::Relaxed);
            Ok(json!({"ok":true}))
        }
        "build" | "check" | "export" => {
            if op == "check" {
                session.dirty = true;
                analyze(&mut session)?;
                return Ok(
                    json!({"ok":!session.project.graph.has_errors(),"diagnostics":session.project.graph.diagnostics,"stats":session.project.stats}),
                );
            }
            if !session.overlays.is_empty() {
                return Err(Error::Message(
                    "Save modified documents before build/export".into(),
                ));
            }
            let result = session.project.build_with_progress(|line| {
                if let Ok(mut logs) = state.build_logs.lock() {
                    logs.push(line);
                }
            });
            session.dirty = false;
            if let Err(error) = &result
                && let Ok(mut logs) = state.build_logs.lock()
            {
                logs.push(format!("[build] error: {error}"));
            }
            let stats = result?;
            if op == "export" {
                let destination = session.project.safe_path(&session.project.config.export)?;
                crate::export::write(&destination, &session.project.graph)?;
                Ok(json!({"output":destination.display().to_string(),"stats":stats}))
            } else {
                Ok(
                    json!({"stats":stats,"output":session.project.output_path()?.display().to_string(),"output_relative":session.project.config.output,"logs":*state.build_logs.lock().map_err(|_|Error::Message("Build status unavailable".into()))?}),
                )
            }
        }
        "format" => Ok(json!({"text":crate::formatter::format(string(&request,"text")?)})),
        "query" | "graph" => {
            analyze(&mut session)?;
            let input = request.get("query").and_then(Value::as_str).unwrap_or("");
            let options = QueryOptions {
                depth: request.get("depth").and_then(Value::as_u64).unwrap_or(1) as usize,
                limit: request
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(if op == "graph" { 120 } else { 12 })
                    .min(120) as usize,
                budget: 1800,
            };
            let result = if input.is_empty() && op == "graph" {
                let symbols: Vec<_> = session
                    .project
                    .graph
                    .symbols
                    .iter()
                    .take(options.limit)
                    .cloned()
                    .collect();
                let ids: std::collections::HashSet<_> =
                    symbols.iter().map(|s| s.id.clone()).collect();
                query::QueryResult {
                    query: String::new(),
                    symbols,
                    edges: session
                        .project
                        .graph
                        .edges
                        .iter()
                        .filter(|e| ids.contains(&e.from) && ids.contains(&e.to))
                        .cloned()
                        .collect(),
                    labels: session
                        .project
                        .graph
                        .symbols
                        .iter()
                        .map(|s| (s.id.clone(), s.qualified.clone()))
                        .collect(),
                    truncated: session.project.graph.symbols.len() > options.limit,
                }
            } else {
                query::query(&session.project.graph, input, &options)?
            };
            Ok(json!({"context":query::render(&result,"compact",1800)?,"result":result}))
        }
        "search" => {
            let needle = string(&request, "query")?;
            if needle.is_empty() {
                return Ok(json!({"matches":[]}));
            }
            let mut matches = Vec::new();
            // Walk real text files, respecting ignore rules; cap IO and result count.
            let walk = session.project.walk(&session.project.root)?;
            let mut size = 0usize;
            for entry in walk
                .build()
                .flatten()
                .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
                .take(20_000)
            {
                let path = entry.path();
                if std::fs::metadata(path).is_ok_and(|m| m.len() > 512 * 1024) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(path) else {
                    continue;
                };
                size += text.len();
                if size > 32 * 1024 * 1024 {
                    break;
                }
                let file = path
                    .strip_prefix(&session.project.root)
                    .map_err(|e| Error::Message(e.to_string()))?
                    .to_string_lossy()
                    .replace('\\', "/");
                let text = session.overlays.get(&file).unwrap_or(&text);
                for (i, line) in text.lines().enumerate() {
                    if let Some(column) = line.to_lowercase().find(&needle.to_lowercase()) {
                        let mut lower_bytes = 0;
                        let mut editor_column = 1;
                        for c in line.chars() {
                            if lower_bytes >= column {
                                break;
                            }
                            lower_bytes += c.to_lowercase().map(char::len_utf8).sum::<usize>();
                            editor_column += c.len_utf16();
                        }
                        matches.push(json!({"file":file,"line":i+1,"column":editor_column,"text":line.chars().take(250).collect::<String>()}));
                        if matches.len() >= 200 {
                            break;
                        }
                    }
                }
                if matches.len() >= 200 {
                    break;
                }
            }
            Ok(json!({"matches":matches,"truncated":matches.len()>=200 || size>32*1024*1024}))
        }
        "completion" | "imports" | "symbol" | "rename" | "ambiguous-options" => {
            analyze(&mut session)?;
            let file = string(&request, "file")?;
            let source = session
                .overlays
                .get(file)
                .cloned()
                .map(Ok)
                .unwrap_or_else(|| read_source(&session.project.safe_path(file)?))?;
            if op == "completion" {
                return Ok(
                    json!({"items":editor::completions_at(&session.project,file,&source,request.get("line").and_then(Value::as_u64).unwrap_or(1) as u32,request.get("column").and_then(Value::as_u64).unwrap_or(u32::MAX as u64) as u32)}),
                );
            }
            if op == "imports" {
                return Ok(json!({"edits":editor::missing_imports(&session.project,file,&source)}));
            }
            if op == "ambiguous-options" {
                let line = request.get("line").and_then(Value::as_u64).unwrap_or(1) as u32;
                let mut options = Vec::new();
                for diagnostic in session.project.graph.diagnostics.iter().filter(|d| {
                    d.code == "S003" && d.location.file == file && d.location.line == line
                }) {
                    let target = source
                        .lines()
                        .nth(line.saturating_sub(1) as usize)
                        .unwrap_or("")
                        .chars()
                        .skip(diagnostic.location.column.saturating_sub(1) as usize)
                        .take(diagnostic.location.length as usize)
                        .collect::<String>();
                    for symbol in
                        session.project.graph.symbols.iter().filter(|s| {
                            s.parent.is_none() && s.kind != "module" && s.name == target
                        })
                    {
                        options.push(json!({"title":format!("Use {}",symbol.qualified),"range":editor::range(&diagnostic.location,&source),"text":symbol.qualified}));
                    }
                }
                return Ok(json!({"options":options}));
            }
            let line = request.get("line").and_then(Value::as_u64).unwrap_or(1) as u32;
            let column = request.get("column").and_then(Value::as_u64).unwrap_or(1) as u32;
            let symbol = editor::symbol_at(&session.project.graph, file, line, column, &source)
                .ok_or_else(|| Error::Message("No symbol at this position".into()))?;
            if op == "rename" {
                let edits = editor::rename(
                    &session.project,
                    &symbol.id,
                    string(&request, "name")?,
                    &sources(&session)?,
                )?;
                return Ok(json!({"edits":edits}));
            }
            let docs = sources(&session)?;
            let definition = editor::range(
                &symbol.location,
                docs.get(&symbol.location.file).map_or("", String::as_str),
            );
            let refs:Vec<_>=editor::references(&session.project.graph,&symbol.id).iter().map(|l|json!({"file":l.file,"range":editor::range(l,docs.get(&l.file).map_or("",String::as_str))})).collect();
            Ok(
                json!({"symbol":symbol,"range":definition,"context":editor::symbol_context(&session.project.graph,&symbol.id),"references":refs}),
            )
        }
        _ => Err(Error::Message(format!("Unknown operation: {op}"))),
    }
}
