use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::post,
};
use luvyn_core::{Error, Project, Result};
use notify::Watcher;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
};

#[derive(rust_embed::RustEmbed)]
#[folder = "../../ui/dist/"]
struct Assets;
struct App {
    core: crate::ide::session::IdeCore,
    projects: std::sync::Mutex<crate::ide::projects::Projects>,
    watcher: std::sync::Mutex<Option<notify::RecommendedWatcher>>,
    watch_root: Arc<std::sync::Mutex<PathBuf>>,
    shutdown: tokio::sync::watch::Sender<bool>,
    target: crate::ide::host::Target,
}
impl std::ops::Deref for App {
    type Target = crate::ide::session::IdeCore;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}
type Shared = Arc<App>;

pub async fn run(root: PathBuf, no_open: bool, port: u16) -> Result<()> {
    run_host(root, no_open, port, crate::ide::host::Target::Server, None).await
}
pub async fn run_host(
    root: PathBuf,
    no_open: bool,
    port: u16,
    target: crate::ide::host::Target,
    ready: Option<std::sync::mpsc::Sender<(String, tokio::sync::watch::Sender<bool>)>>,
) -> Result<()> {
    let mut project = Project::open(&root)?;
    project.analyze(&BTreeMap::new())?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let address = listener.local_addr()?;
    let base_url = format!("http://{address}");
    let (shutdown, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let state = Arc::new(App {
        projects: std::sync::Mutex::new(if cfg!(test) {
            crate::ide::projects::Projects::with_storage(
                &project.root,
                project.root.join(".luvyn/host-test"),
            )?
        } else {
            crate::ide::projects::Projects::new(&project.root)?
        }),
        watcher: std::sync::Mutex::new(None),
        watch_root: Arc::new(std::sync::Mutex::new(project.root.clone())),
        core: crate::ide::session::IdeCore::new(project),
        shutdown: shutdown.clone(),
        target,
    });
    let watch_state = state.clone();
    let watch_root = root.canonicalize()?;
    let event_root = state.watch_root.clone();
    let mut watcher = notify::recommended_watcher(
        move |event: std::result::Result<notify::Event, notify::Error>| {
            if let Ok(event) = event {
                // Reads made by snapshot/search must never trigger another refresh.
                if matches!(
                    event.kind,
                    notify::EventKind::Access(_)
                        | notify::EventKind::Modify(notify::event::ModifyKind::Metadata(
                            notify::event::MetadataKind::AccessTime
                        ))
                ) {
                    return;
                }
                if event.paths.iter().any(|p| {
                    let active_root = match event_root.lock() {
                        Ok(root) => root.clone(),
                        Err(_) => return false,
                    };
                    if p.strip_prefix(&active_root)
                        .unwrap_or(p)
                        .components()
                        .any(|c| {
                            matches!(
                                c.as_os_str().to_str(),
                                Some(
                                    ".git"
                                        | ".luvyn"
                                        | "target"
                                        | "node_modules"
                                        | "dist"
                                        | "graphify-out"
                                )
                            )
                        })
                    {
                        return false;
                    }
                    p.extension().is_some_and(|e| e == "lyn" || e == "toml")
                        || p.file_name().is_some_and(|n| {
                            n == ".ignore.luvyn" || n == ".luvynignore" || n == ".gitignore"
                        })
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
        .watch(&watch_root, notify::RecursiveMode::Recursive)
        .map_err(|e| Error::Message(format!("File watcher: {e}")))?;
    *state
        .watcher
        .lock()
        .map_err(|e| Error::Message(e.to_string()))? = Some(watcher);
    let watch_owner = state.clone();
    let app = router(state);
    let view = if luvyn_core::projects::is_launcher(&root) {
        "#view=projects"
    } else {
        ""
    };
    let url = format!("{base_url}/{view}");
    if target == crate::ide::host::Target::Server {
        eprintln!(
            "Luvyn IDE: {url}\nWorkspace: {}\nPress Ctrl+C to stop.",
            root.display()
        );
    }
    if !no_open && let Err(e) = webbrowser::open(&url) {
        eprintln!("Browser launch failed: {e}; open the URL above");
    }
    if let Some(ready) = ready {
        ready
            .send((url, shutdown))
            .map_err(|e| Error::Message(e.to_string()))?;
    }
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::select! { _=tokio::signal::ctrl_c()=>{}, _=shutdown_rx.changed()=>{} }
        })
        .await?;
    watch_owner
        .watcher
        .lock()
        .map_err(|e| Error::Message(e.to_string()))?
        .take();
    Ok(())
}

fn router(state: Shared) -> Router {
    Router::new()
        .route("/api/action", post(action))
        .fallback(asset)
        .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
        .with_state(state)
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
async fn action(State(state): State<Shared>, Json(request): Json<Value>) -> Response {
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
fn handle(state: &App, mut request: Value) -> Result<Value> {
    let op = request.get("op").and_then(Value::as_str).unwrap_or("");
    if matches!(op, "shutdown" | "show-projects") {
        if op == "shutdown" {
            let _ = state.shutdown.send(true);
        }
        if op == "show-projects" {
            state
                .projects
                .lock()
                .map_err(|e| Error::Message(e.to_string()))?
                .view_revision += 1;
        }
        return Ok(json!({"ok":true}));
    }
    if op == "projects" || op.starts_with("project-") || op.starts_with("drive-") {
        let mut projects = state
            .projects
            .lock()
            .map_err(|e| Error::Message(e.to_string()))?;
        let before = projects.current.clone();
        if op == "drive-connect" {
            request["open_browser"] = json!(state.target == crate::ide::host::Target::Desktop);
        }
        let mut result = projects.action(&state.core, &request)?;
        if let Some(result) = result.as_object_mut() {
            result.insert("host".into(), json!(state.target.name()));
        }
        if projects.current != before
            && let Some(root) = &projects.current
        {
            let watched = {
                let mut active = state
                    .watch_root
                    .lock()
                    .map_err(|e| Error::Message(e.to_string()))?;
                std::mem::replace(&mut *active, root.clone())
            };
            if let Some(watcher) = state
                .watcher
                .lock()
                .map_err(|e| Error::Message(e.to_string()))?
                .as_mut()
            {
                let _ = watcher.unwatch(&watched);
                watcher
                    .watch(root, notify::RecursiveMode::Recursive)
                    .map_err(|e| Error::Message(e.to_string()))?;
            }
        }
        return Ok(result);
    }
    if !matches!(op, "language" | "tokens")
        && state
            .projects
            .lock()
            .map_err(|e| Error::Message(e.to_string()))?
            .current
            .is_none()
    {
        return Err(Error::Message("Open a project first".into()));
    }
    crate::ide::session::handle(&state.core, request)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn app(root: &std::path::Path) -> App {
        let mut project = Project::open(root).unwrap();
        project.analyze(&BTreeMap::new()).unwrap();
        App {
            projects: std::sync::Mutex::new(
                crate::ide::projects::Projects::with_storage(
                    &project.root,
                    root.join(".luvyn/projects-test"),
                )
                .unwrap(),
            ),
            watcher: std::sync::Mutex::new(None),
            watch_root: Arc::new(std::sync::Mutex::new(project.root.clone())),
            core: crate::ide::session::IdeCore::new(project),
            shutdown: tokio::sync::watch::channel(false).0,
            target: crate::ide::host::Target::Server,
        }
    }
    #[test]
    fn projects_open_create_switch_and_preserve_files() {
        let original = tempfile::tempdir().unwrap();
        let next = tempfile::tempdir().unwrap();
        let state = app(original.path());
        std::fs::write(next.path().join("User.lyn"), "class User\n").unwrap();
        let opened = handle(&state, json!({"op":"project-open","path":next.path()})).unwrap();
        assert_eq!(
            opened["current"],
            next.path()
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .as_ref()
        );
        let snapshot = handle(&state, json!({"op":"snapshot"})).unwrap();
        assert!(
            snapshot["symbols"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["name"] == "User")
        );
        let created = original.path().join("Created");
        handle(&state, json!({"op":"project-create","path":created})).unwrap();
        assert!(created.join("main.lyn").is_file());
        assert!(created.join("luvyn.toml").is_file());
        let snapshot = handle(&state, json!({"op":"snapshot"})).unwrap();
        assert!(
            snapshot["diagnostics"].as_array().unwrap().is_empty(),
            "{snapshot}"
        );
        let registry = state.projects.lock().unwrap().registry.list().unwrap();
        let entry = registry
            .iter()
            .find(|p| p.path == created.canonicalize().unwrap().to_string_lossy())
            .unwrap();
        handle(&state, json!({"op":"project-forget","id":entry.id})).unwrap();
        assert!(created.join("main.lyn").exists());
    }
    #[test]
    fn editor_session_live_overlay_save_conflict_and_safe_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let text = "class S\npurpose: original\n";
        std::fs::write(dir.path().join("S.lyn"), text).unwrap();
        let state = app(dir.path());
        let data = handle(&state, json!({"op":"snapshot"})).unwrap();
        assert_eq!(data["symbol_count"], 2);
        assert_eq!(
            handle(&state, json!({"op":"snapshot","revision":1})).unwrap()["unchanged"],
            true
        );
        handle(&state, json!({"op":"edit","file":"S.lyn","text":text})).unwrap();
        assert!(state.core.overlays_empty());
        handle(&state,json!({"op":"edit","file":"S.lyn","text":"class S\npurpose: modified\ndepends Missing\n"})).unwrap();
        let check = handle(&state, json!({"op":"check"})).unwrap();
        assert_eq!(check["ok"], false);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("S.lyn")).unwrap(),
            text
        );
        assert!(handle(&state, json!({"op":"build"})).is_err());
        let old_hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        std::fs::write(dir.path().join("S.lyn"), "class S\npurpose: external\n").unwrap();
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
            json!({"op":"create","file":"new/New.lyn","text":"class New\npurpose: created\n"}),
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
    async fn server_accepts_manual_local_api_requests_without_session_or_origin_checks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("main.lyn"),
            "class Main\npurpose: local access\n",
        )
        .unwrap();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let root = dir.path().to_owned();
        let server = tokio::spawn(async move {
            run_host(
                root,
                true,
                0,
                crate::ide::host::Target::Server,
                Some(ready_tx),
            )
            .await
        });
        let (url, shutdown) = tokio::task::spawn_blocking(move || {
            ready_rx.recv_timeout(std::time::Duration::from_secs(5))
        })
        .await
        .unwrap()
        .unwrap();
        assert!(!url.contains("token"), "{url}");
        let response = tokio::task::spawn_blocking(move || {
            let config = ureq::Agent::config_builder()
                .timeout_global(Some(std::time::Duration::from_secs(5)))
                .build();
            ureq::Agent::new_with_config(config)
                .post(format!("{}/api/action", url.trim_end_matches('/')))
                .header("Origin", "https://untrusted.example")
                .send_json(json!({"op":"snapshot"}))
                .map_err(|error| error.to_string())?
                .body_mut()
                .read_json::<Value>()
                .map_err(|error| error.to_string())
        })
        .await
        .unwrap()
        .unwrap();
        assert!(response["workspace"].as_str().is_some());
        let _ = shutdown.send(true);
        server.await.unwrap().unwrap();
    }
    #[test]
    fn unicode_text_search_never_slices_mid_character() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("unicode.lyn"),
            "class É\npurpose: İ exemplo\n",
        )
        .unwrap();
        let state = app(dir.path());
        let result = handle(&state, json!({"op":"search","query":"exemplo"})).unwrap();
        assert_eq!(result["matches"].as_array().unwrap().len(), 1);
        assert_eq!(result["matches"][0]["column"], 12);
    }
    #[test]
    fn real_build_progress_revision_and_concurrency_guard() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("A.lyn"),
            "class A\npurpose: a\nfields:\n    id: i64\nrules:\n    - self.id imutável\n",
        )
        .unwrap();
        let state = app(dir.path());
        state.building.store(true, Ordering::Release);
        assert!(handle(&state, json!({"op":"build"})).is_err());
        assert_eq!(
            handle(&state, json!({"op":"build-status"})).unwrap()["running"],
            true
        );
        state.building.store(false, Ordering::Release);
        let data = handle(&state, json!({"op":"build"})).unwrap();
        assert!(
            data["logs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|l| l.as_str().unwrap().contains("[build] wrote "))
        );
        assert_eq!(
            luvyn_core::LuReader::open(&dir.path().join(".luvyn/project.lu"))
                .unwrap()
                .nodes()
                .len(),
            3
        );
        assert_eq!(
            handle(&state, json!({"op":"snapshot"})).unwrap()["revision"],
            2
        );
        let dict = handle(&state, json!({"op":"language"})).unwrap();
        assert_eq!(
            dict["entries"],
            serde_json::to_value(luvyn_core::language::dictionary()).unwrap()
        );
        assert!(handle(&state, json!({"op":"shutdown"})).is_ok());
    }
}
