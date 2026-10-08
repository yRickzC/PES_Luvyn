use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::post,
};
use luvyn_core::{
    Error, Project, Result, editor,
    query::{self, QueryOptions},
    workspace::{atomic_write, read_source},
};
use notify::Watcher;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

#[derive(rust_embed::RustEmbed)]
#[folder = "../../ui/dist/"]
struct Assets;
struct Session {
    project: Project,
    overlays: BTreeMap<String, String>,
    dirty: bool,
}
struct App {
    session: Mutex<Session>,
    token: String,
    origin: String,
    external: AtomicBool,
    revision: AtomicU64,
}
type Shared = Arc<App>;

pub async fn run(root: PathBuf, no_open: bool, port: u16) -> Result<()> {
    let mut project = Project::open(&root)?;
    project.analyze(&BTreeMap::new())?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let address = listener.local_addr()?;
    let origin = format!("http://{address}");
    let token = rand::random::<[u8; 32]>()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let state = Arc::new(App {
        session: Mutex::new(Session {
            project,
            overlays: BTreeMap::new(),
            dirty: false,
        }),
        token: token.clone(),
        origin: origin.clone(),
        external: AtomicBool::new(false),
        revision: AtomicU64::new(1),
    });
    let watch_state = state.clone();
    let mut watcher = notify::recommended_watcher(
        move |event: std::result::Result<notify::Event, notify::Error>| {
            if let Ok(event) = event {
                if event.paths.iter().any(|p| {
                    p.extension().is_some_and(|e| e == "lyn" || e == "toml")
                        || p.extension().is_none()
                }) {
                    watch_state.external.store(true, Ordering::Relaxed);
                    watch_state.revision.fetch_add(1, Ordering::Relaxed);
                }
            } else {
                watch_state.external.store(true, Ordering::Relaxed);
            }
        },
    )
    .map_err(|e| Error::Message(format!("File watcher: {e}")))?;
    watcher
        .watch(&root.canonicalize()?, notify::RecursiveMode::Recursive)
        .map_err(|e| Error::Message(format!("File watcher: {e}")))?;
    let app = Router::new()
        .route("/api/action", post(action))
        .fallback(asset)
        .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
        .with_state(state);
    let url = format!("{origin}/#token={token}");
    eprintln!(
        "Luvyn IDE: {url}\nWorkspace: {}\nPress Ctrl+C to stop.",
        root.display()
    );
    if !no_open && let Err(e) = webbrowser::open(&url) {
        eprintln!("Browser launch failed: {e}; open the URL above");
    }
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    drop(watcher);
    Ok(())
}
async fn asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Assets::get(path) {
        Some(data) => {
            let mime = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();
            (
                [
                    (axum::http::header::CONTENT_TYPE, mime),
                    (axum::http::header::CACHE_CONTROL, "no-cache".into()),
                ],
                data.data.to_vec(),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}
async fn action(
    State(state): State<Shared>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    if headers.get("x-luvyn-token").and_then(|h| h.to_str().ok()) != Some(state.token.as_str())
        || headers
            .get("origin")
            .and_then(|h| h.to_str().ok())
            .is_some_and(|o| o != state.origin)
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"Invalid IDE session token or origin"})),
        )
            .into_response();
    }
    let result = tokio::task::spawn_blocking(move || handle(&state, request)).await;
    match result {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err(error)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":error.to_string()})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":format!("Background task: {error}")})),
        )
            .into_response(),
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
            .filter(|(file, _)| file.ends_with(".lyn"))
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
fn handle(state: &App, request: Value) -> Result<Value> {
    let mut session = state
        .session
        .lock()
        .map_err(|_| Error::Message("Workspace session unavailable".into()))?;
    if state.external.swap(false, Ordering::Relaxed) {
        session.dirty = true;
    }
    let op = string(&request, "op")?;
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
            let diagnostics:Vec<_>=session.project.graph.diagnostics.iter().map(|d|json!({"diagnostic":d,"range":editor::range(&d.location,docs.get(&d.location.file).map_or("",String::as_str))})).collect();
            let symbols:Vec<_>=session.project.graph.symbols.iter().take(10_000).map(|s|json!({"id":s.id,"name":s.name,"qualified":s.qualified,"kind":s.kind,"parent":s.parent,"location":s.location,"end_line":s.end_line})).collect();
            let mut folders = std::collections::BTreeSet::new();
            let mut walk = ignore::WalkBuilder::new(&session.project.root);
            walk.hidden(false)
                .require_git(false)
                .follow_links(false)
                .filter_entry(|e| {
                    !matches!(
                        e.file_name().to_string_lossy().as_ref(),
                        ".git" | ".luvyn" | "target" | "node_modules" | "dist" | "graphify-out"
                    )
                });
            for entry in walk.build().flatten().take(20_000) {
                if entry.file_type().is_some_and(|t| t.is_dir())
                    && let Ok(p) = entry.path().strip_prefix(&session.project.root)
                {
                    let p = p.to_string_lossy().replace('\\', "/");
                    if !p.is_empty() {
                        folders.insert(p);
                    }
                }
            }
            Ok(
                json!({"workspace":session.project.root.display().to_string(),"files":entries,"folders":folders,"diagnostics":diagnostics,"symbols":symbols,"symbol_count":session.project.graph.symbols.len(),"edge_count":session.project.graph.edges.len(),"stats":session.project.stats,"revision":state.revision.load(Ordering::Relaxed),"autosave":session.project.config.autosave}),
            )
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
            session.project.safe_path(file)?;
            let text = string(&request, "text")?;
            if text.len() > luvyn_core::workspace::MAX_SOURCE {
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
            if text.len() > luvyn_core::workspace::MAX_SOURCE {
                return Err(Error::Message("Document exceeds 2 MiB".into()));
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
                std::fs::create_dir_all(path)?;
            } else {
                if !file.ends_with(".lyn") {
                    return Err(Error::Message("New documents must end in .lyn".into()));
                }
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut handle = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)?;
                use std::io::Write;
                handle.write_all(
                    request
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .as_bytes(),
                )?;
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
            // Folders must be empty; recursive deletion is intentionally unavailable.
            if path.is_dir() {
                std::fs::remove_dir(path)?;
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
            let stats = session.project.build()?;
            if op == "export" {
                let destination = session.project.safe_path(&session.project.config.export)?;
                luvyn_core::export::write(&destination, &session.project.graph)?;
                Ok(json!({"output":destination.display().to_string(),"stats":stats}))
            } else {
                Ok(json!({"stats":stats}))
            }
        }
        "format" => Ok(json!({"text":luvyn_core::formatter::format(string(&request,"text")?)})),
        "query" | "graph" => {
            analyze(&mut session)?;
            let input = request.get("query").and_then(Value::as_str).unwrap_or("");
            let options = QueryOptions {
                depth: request.get("depth").and_then(Value::as_u64).unwrap_or(1) as usize,
                limit: request
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(if op == "graph" { 120 } else { 12 })
                    as usize,
                budget: 1800,
            };
            let result = if input.is_empty() && op == "graph" {
                let symbols: Vec<_> = session
                    .project
                    .graph
                    .symbols
                    .iter()
                    .take(120)
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
                    truncated: session.project.graph.symbols.len() > 120,
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
            let mut walk = ignore::WalkBuilder::new(&session.project.root);
            walk.hidden(false)
                .require_git(false)
                .follow_links(false)
                .filter_entry(|e| {
                    !matches!(
                        e.file_name().to_string_lossy().as_ref(),
                        ".git" | ".luvyn" | "target" | "node_modules" | "dist" | "graphify-out"
                    )
                });
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
        "completion" | "imports" | "symbol" | "rename" => {
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
                    json!({"items":editor::completions(&session.project,file,&source,request.get("line").and_then(Value::as_u64).unwrap_or(1) as u32)}),
                );
            }
            if op == "imports" {
                return Ok(json!({"edits":editor::missing_imports(&session.project,file,&source)}));
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

#[cfg(test)]
mod tests {
    use super::*;
    fn app(root: &std::path::Path) -> App {
        let mut project = Project::open(root).unwrap();
        project.analyze(&BTreeMap::new()).unwrap();
        App {
            session: Mutex::new(Session {
                project,
                overlays: BTreeMap::new(),
                dirty: false,
            }),
            token: "test-session".into(),
            origin: "http://127.0.0.1:7878".into(),
            external: AtomicBool::new(false),
            revision: AtomicU64::new(1),
        }
    }
    #[test]
    fn editor_session_live_overlay_save_conflict_and_safe_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let text = "service S\npurpose: original\n";
        std::fs::write(dir.path().join("S.lyn"), text).unwrap();
        let state = app(dir.path());
        let data = handle(&state, json!({"op":"snapshot"})).unwrap();
        assert_eq!(data["symbol_count"], 1);
        assert_eq!(
            handle(&state, json!({"op":"snapshot","revision":1})).unwrap()["unchanged"],
            true
        );
        handle(&state, json!({"op":"edit","file":"S.lyn","text":text})).unwrap();
        assert!(state.session.lock().unwrap().overlays.is_empty());
        handle(&state,json!({"op":"edit","file":"S.lyn","text":"service S\npurpose: modified\ndepends Missing\n"})).unwrap();
        let check = handle(&state, json!({"op":"check"})).unwrap();
        assert_eq!(check["ok"], false);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("S.lyn")).unwrap(),
            text
        );
        assert!(handle(&state, json!({"op":"build"})).is_err());
        let old_hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        std::fs::write(dir.path().join("S.lyn"), "service S\npurpose: external\n").unwrap();
        assert!(
            handle(
                &state,
                json!({"op":"save","file":"S.lyn","text":text,"hash":old_hash})
            )
            .is_err()
        );
        assert!(
            handle(
                &state,
                json!({"op":"create","file":"../outside.lyn","text":text})
            )
            .is_err()
        );
        assert!(handle(&state, json!({"op":"delete","file":"."})).is_err());
        handle(&state, json!({"op":"discard","file":"S.lyn"})).unwrap();
        handle(&state, json!({"op":"mkdir","file":"new"})).unwrap();
        handle(
            &state,
            json!({"op":"create","file":"new/New.lyn","text":"entity New\npurpose: created\n"}),
        )
        .unwrap();
        handle(
            &state,
            json!({"op":"move","file":"new/New.lyn","to":"new/Renamed.lyn"}),
        )
        .unwrap();
        assert!(dir.path().join("new/Renamed.lyn").exists());
        assert!(handle(&state, json!({"op":"delete","file":"new"})).is_err());
        handle(&state, json!({"op":"delete","file":"new/Renamed.lyn"})).unwrap();
        handle(&state, json!({"op":"delete","file":"new"})).unwrap();
    }
    #[tokio::test]
    async fn ide_requires_session_capability_and_same_origin() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(app(dir.path()));
        let result = action(
            State(state.clone()),
            HeaderMap::new(),
            Json(json!({"op":"snapshot"})),
        )
        .await;
        assert_eq!(result.status(), StatusCode::FORBIDDEN);
        let mut headers = HeaderMap::new();
        headers.insert("x-luvyn-token", "test-session".parse().unwrap());
        headers.insert("origin", "https://untrusted.example".parse().unwrap());
        let result = action(
            State(state.clone()),
            headers.clone(),
            Json(json!({"op":"snapshot"})),
        )
        .await;
        assert_eq!(result.status(), StatusCode::FORBIDDEN);
        headers.insert("origin", "http://127.0.0.1:7878".parse().unwrap());
        let result = action(State(state), headers, Json(json!({"op":"snapshot"}))).await;
        assert_eq!(result.status(), StatusCode::OK);
    }
    #[test]
    fn unicode_text_search_never_slices_mid_character() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("unicode.lyn"),
            "concept É\npurpose: İ exemplo\n",
        )
        .unwrap();
        let state = app(dir.path());
        let result = handle(&state, json!({"op":"search","query":"exemplo"})).unwrap();
        assert_eq!(result["matches"].as_array().unwrap().len(), 1);
        assert_eq!(result["matches"][0]["column"], 12);
    }
}
