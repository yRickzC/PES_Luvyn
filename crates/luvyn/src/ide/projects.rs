//! Project selection and desktop OAuth. The compiler sees only opened local/cache workspaces.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use luvyn_core::{
    Error, Result,
    projects::{ProjectRegistry, data_directory, is_launcher},
    sync::{SyncProvider, synchronize},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub struct Projects {
    pub registry: ProjectRegistry,
    pub current: Option<PathBuf>,
    cloud: Option<String>,
    oauth: Arc<Mutex<String>>,
    pub view_revision: u64,
}
fn message(e: impl std::fmt::Display) -> Error {
    Error::Message(e.to_string())
}
fn vault() -> Result<keyring::Entry> {
    keyring::Entry::new("dev.luvyn.drive", "desktop-oauth").map_err(message)
}
fn config() -> Result<Value> {
    let path = std::env::var_os("LUVYN_GOOGLE_OAUTH_FILE")
        .map(PathBuf::from)
        .unwrap_or(data_directory()?.join("google-oauth.json"));
    let bytes=std::fs::read(path).map_err(|_|message("Configure a Google Desktop OAuth client in LUVYN_GOOGLE_OAUTH_FILE (see docs/PROJECTS.md)."))?;
    let data: Value = serde_json::from_slice(&bytes)
        .map_err(|_| message("Invalid Desktop OAuth client configuration"))?;
    let installed = data.get("installed").cloned().ok_or_else(|| {
        message("OAuth client must be Desktop (installed); Android uses Google Play authorization")
    })?;
    if installed["client_id"].as_str().is_none() {
        return Err(message("Desktop OAuth client_id missing"));
    }
    Ok(installed)
}
fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .build(),
    )
}
fn token() -> Result<String> {
    let stored = vault()?
        .get_password()
        .map_err(|_| message("Connect Google Drive first"))?;
    let saved: Value = serde_json::from_str(&stored)
        .map_err(|_| message("Invalid Google credentials in secure storage"))?;
    let client = config()?;
    if saved["client_id"] != client["client_id"] {
        return Err(message("OAuth client changed; reconnect Google Drive"));
    }
    let data: Value = agent()
        .post("https://oauth2.googleapis.com/token")
        .send_form([
            ("grant_type", "refresh_token"),
            (
                "refresh_token",
                saved["refresh_token"].as_str().unwrap_or(""),
            ),
            ("client_id", client["client_id"].as_str().unwrap_or("")),
            (
                "client_secret",
                client["client_secret"].as_str().unwrap_or(""),
            ),
        ])
        .map_err(|_| message("Google authorization expired or unavailable; reconnect Drive"))?
        .body_mut()
        .read_json()
        .map_err(message)?;
    data["access_token"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| message("Google did not return an access token"))
}
impl Projects {
    pub fn new(root: &Path) -> Result<Self> {
        Self::with_storage(root, data_directory()?)
    }
    pub fn with_storage(root: &Path, storage: PathBuf) -> Result<Self> {
        let registry = ProjectRegistry::new(storage);
        let current = (!is_launcher(root)).then(|| root.to_path_buf());
        if let Some(path) = &current {
            registry.remember(
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                path.to_string_lossy().into(),
                "local",
                None,
            )?;
        }
        Ok(Self {
            registry,
            current,
            cloud: None,
            oauth: Arc::new(Mutex::new(String::new())),
            view_revision: 0,
        })
    }
    pub fn state(&self) -> Result<Value> {
        Ok(
            json!({"current":self.current,"recent":self.registry.list()?,"cloud":{"connected":vault().and_then(|v|v.get_password().map_err(message)).is_ok(),"configured":config().is_ok(),"authorization":*self.oauth.lock().map_err(message)?}}),
        )
    }
    pub fn synchronize_after_save(
        &mut self,
        core: &luvyn_core::ide::IdeCore,
    ) -> Result<Option<Value>> {
        if self.cloud.is_none() {
            return Ok(None);
        }
        self.action(core, &json!({"op":"project-sync"})).map(Some)
    }
    pub fn action(&mut self, core: &luvyn_core::ide::IdeCore, request: &Value) -> Result<Value> {
        let op = request["op"].as_str().unwrap_or("");
        let text = |key: &str| {
            request[key]
                .as_str()
                .ok_or_else(|| message(format!("Missing {key}")))
        };
        match op {
            "project-view" => Ok(json!({"revision":self.view_revision})),
            "projects" => self.state(),
            "project-forget" => {
                self.registry.forget(text("id")?)?;
                self.state()
            }
            "project-open" | "project-create" => {
                let path = PathBuf::from(text("path")?);
                if !path.is_absolute() {
                    return Err(message("Project path must be absolute"));
                }
                if op == "project-create" {
                    std::fs::create_dir(&path)?;
                    std::fs::write(
                        path.join("main.lyn"),
                        "main:\n    purpose:\n        descrever o projeto\n",
                    )?;
                    std::fs::write(path.join("luvyn.toml"), "sources = [\".\"]\n")?;
                }
                let path = path.canonicalize()?;
                core.open_workspace(&path)?;
                self.registry.remember(
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into(),
                    path.to_string_lossy().into(),
                    "local",
                    None,
                )?;
                self.current = Some(path);
                self.cloud = None;
                self.state()
            }
            "drive-connect" => self.connect(request["open_browser"].as_bool().unwrap_or(true)),
            "drive-disconnect" => {
                vault()?.delete_credential().map_err(message)?;
                self.state()
            }
            "drive-projects" => {
                let mut provider = luvyn_drive::GoogleDriveProvider::new(token()?);
                Ok(json!({"projects":provider.projects()?}))
            }
            "drive-open" | "drive-create" => {
                if !core.overlays_empty() {
                    return Err(message("Save documents before switching projects"));
                }
                let mut provider = luvyn_drive::GoogleDriveProvider::new(token()?);
                let project = if op == "drive-create" {
                    provider.create_project(text("name")?)?
                } else {
                    let id = text("id")?;
                    provider
                        .projects()?
                        .into_iter()
                        .find(|p| p.id == id)
                        .ok_or_else(|| message("Drive project unavailable or not authorized"))?
                };
                let cache = self
                    .registry
                    .directory()
                    .join("cloud")
                    .join(blake3::hash(project.id.as_bytes()).to_hex().as_str());
                std::fs::create_dir_all(&cache)?;
                if op == "drive-create" {
                    std::fs::write(
                        cache.join("main.lyn"),
                        "main:\n    purpose:\n        descrever o projeto\n",
                    )?;
                    std::fs::write(cache.join("luvyn.toml"), "sources = [\".\"]\n")?;
                }
                let report = if op == "drive-create" {
                    synchronize(&mut provider, &project.id, &cache)?
                } else {
                    luvyn_core::sync::open_cloud_cache(
                        &mut provider,
                        &project.id,
                        cache.parent().expect("Cloud cache parent"),
                    )?
                };
                if !report.conflicts.is_empty() {
                    return Ok(json!({"conflicts":report.conflicts,"cache":cache}));
                }
                core.open_workspace_with_target(&cache, Some(&cache))?;
                self.registry.remember(
                    project.name,
                    cache.to_string_lossy().into(),
                    "cloud",
                    Some(project.id.clone()),
                )?;
                self.current = Some(cache);
                self.cloud = Some(project.id);
                self.state()
            }
            "project-sync" => {
                let cloud = self
                    .cloud
                    .as_ref()
                    .ok_or_else(|| message("Current project is local"))?;
                let root = self
                    .current
                    .as_ref()
                    .ok_or_else(|| message("No project open"))?;
                let report = core.with_saved_workspace(|| {
                    let mut provider = luvyn_drive::GoogleDriveProvider::new(token()?);
                    synchronize(&mut provider, cloud, root)
                })?;
                core.external
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                core.revision
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(json!({"report":report}))
            }
            _ => Err(message("Unknown project operation")),
        }
    }
    fn connect(&self, open_browser: bool) -> Result<Value> {
        let client = config()?;
        let _ = vault()?;
        self.connect_with_client(client, open_browser)
    }
    fn connect_with_client(&self, client: Value, open_browser: bool) -> Result<Value> {
        let mut status = self.oauth.lock().map_err(message)?;
        if *status == "pending" {
            return Err(message("Google authorization already pending"));
        }
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let redirect = format!(
            "http://127.0.0.1:{}/callback",
            listener.local_addr()?.port()
        );
        let state = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
        let verifier = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("client_id", client["client_id"].as_str().unwrap_or(""))
            .append_pair("redirect_uri", &redirect)
            .append_pair("response_type", "code")
            .append_pair("scope", "https://www.googleapis.com/auth/drive.file")
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent")
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .finish();
        let url = format!("https://accounts.google.com/o/oauth2/v2/auth?{query}");
        if open_browser {
            webbrowser::open(&url).map_err(message)?;
        }
        *status = "pending".into();
        drop(status);
        let result_state = self.oauth.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                let deadline = Instant::now() + Duration::from_secs(180);
                loop {
                    if Instant::now() > deadline {
                        return Err(message("Google authorization timed out"));
                    }
                    let (mut stream, _) = match listener.accept() {
                        Ok(v) => v,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(100));
                            continue;
                        }
                        Err(e) => return Err(e.into()),
                    };
                    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
                    let mut bytes = [0; 8192];
                    let count = stream.read(&mut bytes)?;
                    let header = String::from_utf8_lossy(&bytes[..count]);
                    let target = header
                        .lines()
                        .next()
                        .unwrap_or("")
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("");
                    let callback =
                        url::Url::parse(&format!("http://localhost{target}")).map_err(message)?;
                    let params: std::collections::HashMap<_, _> =
                        callback.query_pairs().into_owned().collect();
                    if callback.path() != "/callback" || params.get("state") != Some(&state) {
                        let _=stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        continue;
                    }
                    if params.contains_key("error") {
                        let body = "Luvyn: Google login cancelled. Return to the IDE to try again.";
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )?;
                        return Err(message("Google login cancelled; you can connect again"));
                    }
                    let code = params
                        .get("code")
                        .ok_or_else(|| message("Google authorization declined"))?;
                    let tokens: Value = agent()
                        .post("https://oauth2.googleapis.com/token")
                        .send_form([
                            ("grant_type", "authorization_code"),
                            ("code", code.as_str()),
                            ("redirect_uri", redirect.as_str()),
                            ("client_id", client["client_id"].as_str().unwrap_or("")),
                            (
                                "client_secret",
                                client["client_secret"].as_str().unwrap_or(""),
                            ),
                            ("code_verifier", verifier.as_str()),
                        ])
                        .map_err(|_| message("Google token exchange failed"))?
                        .body_mut()
                        .read_json()
                        .map_err(message)?;
                    let refresh = tokens["refresh_token"]
                        .as_str()
                        .ok_or_else(|| message("Google did not grant offline access"))?;
                    vault()?.set_password(&json!({"refresh_token":refresh,"client_id":client["client_id"]}).to_string()).map_err(|_|message("Secure credential storage unavailable; authorization was not persisted"))?;
                    let body = "Luvyn: Google Drive connected. You can close this tab.";
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )?;
                    return Ok(());
                }
            })();
            if let Ok(mut status) = result_state.lock() {
                *status = match result {
                    Ok(()) => "connected".into(),
                    Err(e) => e.to_string(),
                };
            }
        });
        Ok(json!({"pending":true,"authorization_url":if open_browser { None } else { Some(url) }}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_save_preserves_local_data_when_other_buffers_block_sync() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("main.lyn"), "class Main\n").unwrap();
        std::fs::write(root.path().join("other.lyn"), "class Other\n").unwrap();
        let core = luvyn_core::ide::IdeCore::new(luvyn_core::Project::open(root.path()).unwrap());
        let mut projects =
            Projects::with_storage(root.path(), root.path().join("registry")).unwrap();
        projects.cloud = Some("test-cloud".into());
        luvyn_core::ide::handle(
            &core,
            json!({"op":"edit","file":"other.lyn","text":"class Other\npurpose: unsaved\n"}),
        )
        .unwrap();
        let old = luvyn_core::ide::handle(&core, json!({"op":"file","file":"main.lyn"})).unwrap();
        let saved = "class Main\npurpose: preserved\n";
        luvyn_core::ide::handle(
            &core,
            json!({"op":"save","file":"main.lyn","text":saved,"hash":old["hash"]}),
        )
        .unwrap();
        let error = projects.synchronize_after_save(&core).unwrap_err();
        assert!(error.to_string().contains("Save documents"));
        assert_eq!(
            std::fs::read_to_string(root.path().join("main.lyn")).unwrap(),
            saved
        );
        assert!(!core.overlays_empty());
    }

    #[test]
    fn browser_oauth_returns_url_and_accepts_cancel_without_launching_external_browser() {
        let root = tempfile::tempdir().unwrap();
        let projects = Projects::with_storage(root.path(), root.path().join("registry")).unwrap();
        let result = projects
            .connect_with_client(json!({"client_id":"test-client"}), false)
            .unwrap();
        assert_eq!(result["pending"], true);
        let url = url::Url::parse(result["authorization_url"].as_str().unwrap()).unwrap();
        assert_eq!(url.host_str(), Some("accounts.google.com"));
        let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(
            params["scope"],
            "https://www.googleapis.com/auth/drive.file"
        );
        assert_eq!(params["code_challenge_method"], "S256");
        assert!(
            projects
                .connect_with_client(json!({"client_id":"test-client"}), false)
                .is_err()
        );
        let mut callback = url::Url::parse(&params["redirect_uri"]).unwrap();
        callback
            .query_pairs_mut()
            .append_pair("state", &params["state"])
            .append_pair("error", "access_denied");
        let body = agent()
            .get(callback.as_str())
            .call()
            .unwrap()
            .body_mut()
            .read_to_string()
            .unwrap();
        assert!(body.contains("cancelled"));
        let deadline = Instant::now() + Duration::from_secs(3);
        while *projects.oauth.lock().unwrap() == "pending" && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(projects.oauth.lock().unwrap().contains("cancelled"));
    }
}
