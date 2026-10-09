//! Google Drive storage adapter. OAuth lifecycle belongs to each platform host.
use luvyn_core::{
    Error, Result,
    sync::{RemoteDirectory, RemoteFile, RemoteProject, SyncProvider, is_source},
};
use serde_json::{Value, json};
use std::time::Duration;

const FILE_FIELDS: &str = "id,version,modifiedTime,md5Checksum,headRevisionId,trashed";

/// Drive v3 versions are opaque decimal strings, never HTTP entity tags.
#[derive(Debug, PartialEq, Eq)]
struct FileMetadata {
    id: String,
    version: String,
    modified_time: Option<String>,
    md5_checksum: Option<String>,
    head_revision_id: Option<String>,
}
impl FileMetadata {
    fn parse(value: &Value) -> Result<Self> {
        let required = |key: &str| {
            value[key]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(String::from)
                .ok_or_else(|| GoogleDriveProvider::error(format!("Missing file {key}")))
        };
        if value["trashed"] == true {
            return Err(GoogleDriveProvider::error("Remote file is trashed"));
        }
        let version = required("version")?;
        if !version.bytes().all(|b| b.is_ascii_digit()) {
            return Err(GoogleDriveProvider::error("Invalid remote file version"));
        }
        Ok(Self {
            id: required("id")?,
            version,
            modified_time: value["modifiedTime"].as_str().map(String::from),
            md5_checksum: value["md5Checksum"].as_str().map(String::from),
            // Only blob files expose this; absence is valid.
            head_revision_id: value["headRevisionId"].as_str().map(String::from),
        })
    }
    fn remote_file(self, path: &str) -> RemoteFile {
        RemoteFile {
            id: self.id,
            path: path.into(),
            version: self.version,
        }
    }
}
pub struct GoogleDriveProvider {
    agent: ureq::Agent,
    token: String,
    endpoint: String,
    upload_endpoint: String,
    tree_cache: Option<(String, Vec<RemoteFile>, Vec<RemoteDirectory>)>,
}
impl GoogleDriveProvider {
    pub fn new(token: String) -> Self {
        Self {
            agent: ureq::Agent::new_with_config(
                ureq::Agent::config_builder()
                    .timeout_global(Some(Duration::from_secs(30)))
                    .build(),
            ),
            token,
            endpoint: "https://www.googleapis.com/drive/v3".into(),
            upload_endpoint: "https://www.googleapis.com/upload/drive/v3".into(),
            tree_cache: None,
        }
    }
    fn error(e: impl std::fmt::Display) -> Error {
        Error::Message(format!("Google Drive: {e}"))
    }
    fn get(&self, path: &str) -> Result<Value> {
        self.agent
            .get(format!("{}{path}", self.endpoint))
            .header("Authorization", format!("Bearer {}", self.token))
            .call()
            .map_err(Self::error)?
            .body_mut()
            .read_json()
            .map_err(Self::error)
    }
    fn list(&self, query: &str) -> Result<Vec<Value>> {
        let mut results = vec![];
        let mut page = String::new();
        loop {
            let q = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("q", query)
                .append_pair(
                    "fields",
                    "nextPageToken,files(id,name,mimeType,version,modifiedTime,md5Checksum,headRevisionId,trashed,properties)",
                )
                .append_pair("pageSize", "1000")
                .append_pair("pageToken", &page)
                .finish();
            let data = self.get(&format!("/files?{q}"))?;
            results.extend(data["files"].as_array().cloned().unwrap_or_default());
            if results.len() > 20000 {
                return Err(Self::error("Project exceeds 20,000 remote entries"));
            }
            page = data["nextPageToken"].as_str().unwrap_or("").into();
            if page.is_empty() {
                break;
            }
        }
        Ok(results)
    }
    fn folder(&self, name: &str, parent: Option<&str>, project: bool) -> Result<String> {
        let mut value = json!({"name":name,"mimeType":"application/vnd.google-apps.folder"});
        if let Some(parent) = parent {
            value["parents"] = json!([parent]);
        }
        if project {
            value["properties"] = json!({"luvyn":"project","schema":"1"});
        }
        let data: Value = self
            .agent
            .post(format!("{}/files", self.endpoint))
            .header("Authorization", format!("Bearer {}", self.token))
            .send_json(&value)
            .map_err(Self::error)?
            .body_mut()
            .read_json()
            .map_err(Self::error)?;
        data["id"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| Self::error("Missing folder id"))
    }
    fn walk(
        &self,
        parent: &str,
        prefix: &str,
        depth: usize,
        result: &mut Vec<RemoteFile>,
        directories: &mut Vec<RemoteDirectory>,
    ) -> Result<()> {
        if depth > 32 {
            return Err(Self::error("Folder depth exceeds 32"));
        }
        for file in self.list(&format!(
            "'{}' in parents and trashed = false",
            escape(parent)
        ))? {
            let name = file["name"]
                .as_str()
                .ok_or_else(|| Self::error("Missing file name"))?;
            if name.contains(['/', '\\']) || matches!(name, "." | "..") {
                return Err(Self::error("Invalid remote filename"));
            }
            let path = format!("{prefix}{name}");
            let id = file["id"]
                .as_str()
                .ok_or_else(|| Self::error("Missing id"))?;
            if file["mimeType"] == "application/vnd.google-apps.folder" {
                if !matches!(name, ".luvyn" | ".git" | "target" | "node_modules" | "dist") {
                    let version = file["version"]
                        .as_str()
                        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
                        .ok_or_else(|| Self::error("Missing or invalid folder version"))?;
                    directories.push(RemoteDirectory {
                        id: id.into(),
                        path: path.clone(),
                        version: version.into(),
                    });
                    self.walk(id, &format!("{path}/"), depth + 1, result, directories)?;
                }
            } else if is_source(&path) {
                result.push(FileMetadata::parse(&file)?.remote_file(&path));
            }
            if result.len() > 10000 {
                return Err(Self::error("Project exceeds 10,000 documents"));
            }
        }
        Ok(())
    }
    fn collect_tree(&self, project: &str) -> Result<(Vec<RemoteFile>, Vec<RemoteDirectory>)> {
        let mut files = Vec::new();
        let mut directories = Vec::new();
        self.walk(project, "", 0, &mut files, &mut directories)?;
        if files.len() > 10000 || directories.len() > 10000 {
            return Err(Self::error("Project exceeds 10,000 documents or folders"));
        }
        Ok((files, directories))
    }
    fn folder_metadata(&self, id: &str, path: &str) -> Result<RemoteDirectory> {
        let value = self.get(&format!("/files/{id}?fields=id,version,mimeType,trashed"))?;
        if value["mimeType"] != "application/vnd.google-apps.folder" || value["trashed"] == true {
            return Err(Self::error(format!("Remote folder changed: {path}")));
        }
        let version = value["version"]
            .as_str()
            .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
            .ok_or_else(|| Self::error("Missing or invalid folder version"))?;
        Ok(RemoteDirectory {
            id: value["id"]
                .as_str()
                .ok_or_else(|| Self::error("Missing folder id"))?
                .into(),
            path: path.into(),
            version: version.into(),
        })
    }
    fn metadata(&self, id: &str) -> Result<FileMetadata> {
        FileMetadata::parse(&self.get(&format!("/files/{id}?fields={FILE_FIELDS}"))?)
    }
    fn verify(&self, file: &RemoteFile) -> Result<FileMetadata> {
        let current = self.metadata(&file.id)?;
        if file.version.is_empty() || current.id != file.id || current.version != file.version {
            return Err(Self::error(format!(
                "Remote conflict: {} (expected version {}, found {})",
                file.path, file.version, current.version
            )));
        }
        Ok(current)
    }
}
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}
impl SyncProvider for GoogleDriveProvider {
    fn projects(&mut self) -> Result<Vec<RemoteProject>> {
        Ok(self.list("trashed = false and mimeType = 'application/vnd.google-apps.folder' and properties has { key='luvyn' and value='project' }")?.into_iter().filter_map(|v|Some(RemoteProject{id:v["id"].as_str()?.into(),name:v["name"].as_str()?.into()})).collect())
    }
    fn create_project(&mut self, name: &str) -> Result<RemoteProject> {
        if name.trim().is_empty() || name.len() > 160 {
            return Err(Self::error("Project name must contain 1–160 characters"));
        }
        let folders=self.list("trashed = false and mimeType = 'application/vnd.google-apps.folder' and name = 'Luvyn'")?;
        let parent = match folders.first().and_then(|v| v["id"].as_str()) {
            Some(id) => id.to_string(),
            None => self.folder("Luvyn", None, false)?,
        };
        Ok(RemoteProject {
            id: self.folder(name, Some(&parent), true)?,
            name: name.into(),
        })
    }
    fn files(&mut self, project: &str) -> Result<Vec<RemoteFile>> {
        let (files, directories) = self.collect_tree(project)?;
        self.tree_cache = Some((project.into(), files.clone(), directories));
        Ok(files)
    }
    fn directories(&mut self, project: &str) -> Result<Vec<RemoteDirectory>> {
        if !self
            .tree_cache
            .as_ref()
            .is_some_and(|(cached, _, _)| cached == project)
        {
            let (files, directories) = self.collect_tree(project)?;
            self.tree_cache = Some((project.into(), files, directories));
        }
        Ok(self.tree_cache.as_ref().expect("tree cached").2.clone())
    }
    fn create_directory(&mut self, project: &str, path: &str) -> Result<RemoteDirectory> {
        let parts: Vec<_> = path.split('/').collect();
        if parts.is_empty()
            || parts.len() > 32
            || parts.iter().any(|part| {
                part.is_empty() || matches!(*part, "." | "..") || part.contains(['\\', '/'])
            })
        {
            return Err(Self::error("Invalid folder path"));
        }
        let mut parent = project.to_string();
        let mut relative = String::new();
        let mut result = None;
        for part in parts {
            if !relative.is_empty() {
                relative.push('/');
            }
            relative.push_str(part);
            let matches = self.list(&format!(
                "'{}' in parents and name = '{}' and trashed = false",
                escape(&parent),
                escape(part)
            ))?;
            let mut folders = matches
                .iter()
                .filter(|item| item["mimeType"] == "application/vnd.google-apps.folder");
            if let Some(existing) = folders.next() {
                if folders.next().is_some()
                    || matches
                        .iter()
                        .any(|item| item["mimeType"] != "application/vnd.google-apps.folder")
                {
                    return Err(Self::error(format!(
                        "Duplicate or conflicting folder path: {relative}"
                    )));
                }
                parent = existing["id"]
                    .as_str()
                    .ok_or_else(|| Self::error("Missing folder id"))?
                    .into();
                result = Some(self.folder_metadata(&parent, &relative)?);
            } else {
                if !matches.is_empty() {
                    return Err(Self::error(format!(
                        "A file already uses folder path: {relative}"
                    )));
                }
                parent = self.folder(part, Some(&parent), false)?;
                result = Some(self.folder_metadata(&parent, &relative)?);
            }
        }
        self.tree_cache = None;
        result.ok_or_else(|| Self::error("Invalid folder path"))
    }
    fn delete_directory(&mut self, directory: &RemoteDirectory) -> Result<()> {
        let current = self.folder_metadata(&directory.id, &directory.path)?;
        if current.id != directory.id || current.version != directory.version {
            return Err(Self::error(format!(
                "Remote folder conflict: {}",
                directory.path
            )));
        }
        if !self
            .list(&format!(
                "'{}' in parents and trashed = false",
                escape(&directory.id)
            ))?
            .is_empty()
        {
            return Err(Self::error(format!(
                "Remote folder is not empty: {}",
                directory.path
            )));
        }
        self.agent
            .patch(format!("{}/files/{}", self.endpoint, directory.id))
            .header("Authorization", format!("Bearer {}", self.token))
            .send_json(&json!({"trashed":true}))
            .map_err(Self::error)?;
        self.tree_cache = None;
        Ok(())
    }
    fn download(&mut self, file: &RemoteFile) -> Result<Vec<u8>> {
        let before = self.verify(file)?;
        let data = self
            .agent
            .get(format!("{}/files/{}?alt=media", self.endpoint, file.id))
            .header("Authorization", format!("Bearer {}", self.token))
            .call()
            .map_err(Self::error)?
            .body_mut()
            .with_config()
            .limit(2 * 1024 * 1024)
            .read_to_vec()
            .map_err(Self::error)?;
        // A download must represent the listed version, not a concurrent update.
        if self.verify(file)? != before {
            return Err(Self::error(format!(
                "Remote conflict during download: {}",
                file.path
            )));
        }
        Ok(data)
    }
    fn upload(
        &mut self,
        project: &str,
        path: &str,
        data: &[u8],
        expected: Option<&RemoteFile>,
    ) -> Result<RemoteFile> {
        self.tree_cache = None;
        if let Some(file) = expected {
            // Recheck immediately before mutation, using the sync base/listed version.
            self.verify(file)?;
            let value: Value = self
                .agent
                .patch(format!(
                    "{}/files/{}?uploadType=media&fields={FILE_FIELDS}",
                    self.upload_endpoint, file.id
                ))
                .header("Authorization", format!("Bearer {}", self.token))
                .header("Content-Type", "application/octet-stream")
                .send(data)
                .map_err(Self::error)?
                .body_mut()
                .read_json()
                .map_err(Self::error)?;
            // Use the mutation response, not a later GET that could adopt another writer's version.
            let uploaded = FileMetadata::parse(&value)?;
            if uploaded.id != file.id || uploaded.version == file.version {
                return Err(Self::error("Upload returned inconsistent file metadata"));
            }
            return Ok(uploaded.remote_file(path));
        }
        let mut parent = project.to_string();
        let mut parts = path.split('/').peekable();
        let mut name = "";
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                name = part;
                break;
            }
            let folders=self.list(&format!("'{}' in parents and name = '{}' and mimeType = 'application/vnd.google-apps.folder' and trashed = false",escape(&parent),escape(part)))?;
            parent = match folders.first().and_then(|v| v["id"].as_str()) {
                Some(id) => id.into(),
                None => self.folder(part, Some(&parent), false)?,
            };
        }
        // Recheck name immediately before create; never create a duplicate silently.
        if !self
            .list(&format!(
                "'{}' in parents and name = '{}' and trashed = false",
                escape(&parent),
                escape(name)
            ))?
            .is_empty()
        {
            return Err(Self::error(format!("Remote conflict: {path}")));
        }
        let boundary = format!("luvyn-{}", luvyn_core_hash(data));
        let metadata = json!({"name":name,"parents":[parent]});
        let mut body=format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata}\r\n--{boundary}\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes();
        body.extend_from_slice(data);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let meta: Value = self
            .agent
            .post(format!(
                "{}/files?uploadType=multipart&fields={FILE_FIELDS}",
                self.upload_endpoint
            ))
            .header("Authorization", format!("Bearer {}", self.token))
            .header(
                "Content-Type",
                format!("multipart/related; boundary={boundary}"),
            )
            .send(&body)
            .map_err(Self::error)?
            .body_mut()
            .read_json()
            .map_err(Self::error)?;
        Ok(FileMetadata::parse(&meta)?.remote_file(path))
    }
    fn delete(&mut self, file: &RemoteFile) -> Result<()> {
        self.tree_cache = None;
        self.verify(file)?;
        self.agent
            .patch(format!("{}/files/{}", self.endpoint, file.id))
            .header("Authorization", format!("Bearer {}", self.token))
            .send_json(json!({"trashed":true}))
            .map_err(Self::error)?;
        Ok(())
    }
}
fn luvyn_core_hash(data: &[u8]) -> String {
    // Boundary is not security-sensitive.
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut h);
    format!("{:016x}", h.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    fn server(
        responses: Vec<(&'static str, &'static str)>,
    ) -> (GoogleDriveProvider, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let mut requests = vec![];
            for (extra, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = vec![];
                let mut chunk = [0; 4096];
                loop {
                    let n = stream.read(&mut chunk).unwrap();
                    if n == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&chunk[..n]);
                    if let Some(split) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..split]);
                        let size = header
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= split + 4 + size {
                            break;
                        }
                    }
                }
                let request = String::from_utf8(bytes).unwrap();
                assert!(
                    request
                        .to_lowercase()
                        .contains("authorization: bearer test-token")
                );
                requests.push(request);
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",body.len()).unwrap();
            }
            requests
        });
        let mut provider = GoogleDriveProvider::new("test-token".into());
        provider.endpoint = url.clone();
        provider.upload_endpoint = url;
        (provider, worker)
    }
    fn file(version: &str) -> RemoteFile {
        RemoteFile {
            id: "f".into(),
            path: "main.lyn".into(),
            version: version.into(),
        }
    }
    const V7: &str = r#"{"id":"f","version":"7","modifiedTime":"2026-10-09T10:00:00Z","md5Checksum":"abc","headRevisionId":"r7"}"#;
    const V8: &str = r#"{"id":"f","version":"8"}"#;
    const V9: &str = r#"{"id":"f","version":"9"}"#;
    const LIST7: &str =
        r#"{"files":[{"id":"f","name":"main.lyn","version":"7","mimeType":"text/plain"}]}"#;
    const LIST8: &str =
        r#"{"files":[{"id":"f","name":"main.lyn","version":"8","mimeType":"text/plain"}]}"#;
    const LIST9: &str =
        r#"{"files":[{"id":"f","name":"main.lyn","version":"9","mimeType":"text/plain"}]}"#;
    #[test]
    fn drive_lists_downloads_uploads_and_trashes_without_etag() {
        let (mut provider, worker) = server(vec![
            ("", r#"{"files":[{"id":"p","name":"MyProject"}]}"#),
            ("", LIST7),
            ("", V7),
            ("", "cloud"),
            ("", V7),
            ("", V7),
            ("", V8),
            ("", V8),
            ("", "{}"),
        ]);
        assert_eq!(provider.projects().unwrap()[0].id, "p");
        let remote = provider.files("p").unwrap().remove(0);
        assert_eq!(provider.download(&remote).unwrap(), b"cloud");
        let uploaded = provider
            .upload("p", "main.lyn", b"updated", Some(&remote))
            .unwrap();
        assert_eq!(uploaded.version, "8");
        provider.delete(&uploaded).unwrap();
        let requests = worker.join().unwrap();
        assert!(requests[0].contains("properties"));
        assert!(requests[2].contains("modifiedTime,md5Checksum,headRevisionId"));
        assert!(requests[6].starts_with("PATCH"));
        assert!(requests[6].contains("updated"));
        assert!(requests[8].contains("trashed"));
        assert!(
            requests
                .iter()
                .all(|r| !r.to_lowercase().contains("if-match"))
        );
    }
    #[test]
    fn drive_rejects_stale_missing_or_wrong_metadata_before_mutation() {
        for metadata in [
            V9,
            r#"{"id":"f"}"#,
            r#"{"id":"other","version":"8"}"#,
            r#"{"id":"f","version":"8","trashed":true}"#,
        ] {
            let (mut provider, worker) = server(vec![("", metadata)]);
            assert!(
                provider
                    .upload("p", "main.lyn", b"local", Some(&file("8")))
                    .is_err()
            );
            assert_eq!(worker.join().unwrap().len(), 1);
            let (mut provider, worker) = server(vec![("", metadata)]);
            assert!(provider.delete(&file("8")).is_err());
            assert_eq!(worker.join().unwrap().len(), 1);
        }
    }
    #[test]
    fn drive_creates_nested_real_folders_with_parent_links() {
        let (mut provider, worker) = server(vec![
            ("", r#"{"files":[]}"#),
            ("", r#"{"id":"docs"}"#),
            (
                "",
                r#"{"id":"docs","version":"1","mimeType":"application/vnd.google-apps.folder"}"#,
            ),
            ("", r#"{"files":[]}"#),
            ("", r#"{"id":"player"}"#),
            (
                "",
                r#"{"id":"player","version":"1","mimeType":"application/vnd.google-apps.folder"}"#,
            ),
        ]);
        let created = provider.create_directory("project", "docs/player").unwrap();
        assert_eq!(created.path, "docs/player");
        assert_eq!(created.id, "player");
        let requests = worker.join().unwrap();
        assert!(requests[1].starts_with("POST /files"));
        assert!(requests[1].contains("application/vnd.google-apps.folder"));
        assert!(requests[3].contains("player"));
        assert!(requests[4].contains("application/vnd.google-apps.folder"));
        assert!(requests[4].contains("docs"), "{}", requests[4]);
    }
    #[test]
    fn drive_lists_nested_files_with_their_real_folder_paths() {
        let (mut provider, worker) = server(vec![
            (
                "",
                r#"{"files":[{"id":"docs","name":"docs","mimeType":"application/vnd.google-apps.folder","version":"1"}]}"#,
            ),
            (
                "",
                r#"{"files":[{"id":"player","name":"player","mimeType":"application/vnd.google-apps.folder","version":"2"}]}"#,
            ),
            (
                "",
                r#"{"files":[{"id":"f","name":"Player.lyn","mimeType":"text/plain","version":"7"}]}"#,
            ),
        ]);
        let files = provider.files("project").unwrap();
        let directories = provider.directories("project").unwrap();
        assert_eq!(files[0].path, "docs/player/Player.lyn");
        assert_eq!(
            directories
                .iter()
                .map(|dir| dir.path.as_str())
                .collect::<Vec<_>>(),
            vec!["docs", "docs/player"]
        );
        assert_eq!(worker.join().unwrap().len(), 3);
    }
    #[test]
    fn drive_rejects_remote_change_during_download() {
        let (mut provider, worker) = server(vec![("", V7), ("", "cloud"), ("", V8)]);
        assert!(provider.download(&file("7")).is_err());
        assert_eq!(worker.join().unwrap().len(), 3);
    }
    #[test]
    fn drive_sync_persists_versions_downloads_remote_changes_and_preserves_conflicts() {
        use luvyn_core::sync::synchronize;
        let root = tempfile::tempdir().unwrap();
        let (mut provider, worker) = server(vec![
            // Initial download establishes base 7.
            ("", LIST7),
            ("", V7),
            ("", "cloud"),
            ("", V7),
            // Local edit only: upload returns version 8, without a subsequent GET.
            ("", LIST7),
            ("", V7),
            ("", V8),
            // Next sync must not upload unchanged content.
            ("", LIST8),
            // Remote edit only: download version 9.
            ("", LIST9),
            ("", V9),
            ("", "remote update"),
            ("", V9),
            // Both sides changed: no writes, surface conflict.
            ("", LIST8),
            ("", V8),
            ("", "different remote"),
            ("", V8),
        ]);
        assert_eq!(
            synchronize(&mut provider, "p", root.path())
                .unwrap()
                .downloaded,
            1
        );
        let manifest_path = root.path().join(".luvyn/sync.json");
        let manifest = || -> Value {
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap()
        };
        assert_eq!(manifest()["files"]["main.lyn"]["last_synced_version"], "7");
        // Existing manifests without the explicit field use remote.version as base.
        let mut legacy = manifest();
        legacy["files"]["main.lyn"]
            .as_object_mut()
            .unwrap()
            .remove("last_synced_version");
        std::fs::write(&manifest_path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        std::fs::write(root.path().join("main.lyn"), b"local update").unwrap();
        assert_eq!(
            synchronize(&mut provider, "p", root.path())
                .unwrap()
                .uploaded,
            1
        );
        assert_eq!(manifest()["files"]["main.lyn"]["last_synced_version"], "8");
        assert_eq!(
            synchronize(&mut provider, "p", root.path())
                .unwrap()
                .unchanged,
            1
        );
        assert_eq!(
            synchronize(&mut provider, "p", root.path())
                .unwrap()
                .downloaded,
            1
        );
        assert_eq!(
            std::fs::read(root.path().join("main.lyn")).unwrap(),
            b"remote update"
        );
        assert_eq!(manifest()["files"]["main.lyn"]["last_synced_version"], "9");
        std::fs::write(root.path().join("main.lyn"), b"local conflict").unwrap();
        let before = std::fs::read(&manifest_path).unwrap();
        assert_eq!(
            synchronize(&mut provider, "p", root.path())
                .unwrap()
                .conflicts,
            vec!["main.lyn"]
        );
        assert_eq!(std::fs::read(&manifest_path).unwrap(), before);
        assert_eq!(
            std::fs::read(root.path().join("main.lyn")).unwrap(),
            b"local conflict"
        );
        let requests = worker.join().unwrap();
        assert_eq!(
            requests.iter().filter(|r| r.starts_with("PATCH")).count(),
            1
        );
    }
}
