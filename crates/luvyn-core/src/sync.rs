//! Provider-independent, conservative three-way synchronization of documentation sources.
use crate::{
    Error, Result,
    workspace::{atomic_write, safe_path},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};
const MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteFile {
    pub id: String,
    pub path: String,
    pub version: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteDirectory {
    pub id: String,
    pub path: String,
    pub version: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteProject {
    pub id: String,
    pub name: String,
}
pub trait SyncProvider {
    fn projects(&mut self) -> Result<Vec<RemoteProject>>;
    fn create_project(&mut self, name: &str) -> Result<RemoteProject>;
    fn files(&mut self, project: &str) -> Result<Vec<RemoteFile>>;
    fn directories(&mut self, project: &str) -> Result<Vec<RemoteDirectory>>;
    fn create_directory(&mut self, project: &str, path: &str) -> Result<RemoteDirectory>;
    fn delete_directory(&mut self, directory: &RemoteDirectory) -> Result<()>;
    fn download(&mut self, file: &RemoteFile) -> Result<Vec<u8>>;
    /// Implementations must verify the expected remote version before changing it.
    fn upload(
        &mut self,
        project: &str,
        path: &str,
        data: &[u8],
        expected: Option<&RemoteFile>,
    ) -> Result<RemoteFile>;
    fn delete(&mut self, file: &RemoteFile) -> Result<()>;
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Tracked {
    remote: RemoteFile,
    hash: String,
    #[serde(default)]
    last_synced_version: Option<String>,
}
impl Tracked {
    fn new(remote: RemoteFile, hash: String) -> Self {
        let last_synced_version = Some(remote.version.clone());
        Self {
            remote,
            hash,
            last_synced_version,
        }
    }
    fn version(&self) -> &str {
        self.last_synced_version
            .as_deref()
            .unwrap_or(&self.remote.version)
    }
}
#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    #[serde(default)]
    schema_version: u32,
    files: BTreeMap<String, Tracked>,
    #[serde(default)]
    directories: BTreeMap<String, TrackedDirectory>,
}
impl Manifest {
    fn current() -> Self {
        Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            ..Self::default()
        }
    }
}
fn write_manifest(path: &Path, manifest: &Manifest) -> Result<()> {
    atomic_write(
        path,
        &serde_json::to_vec(manifest).map_err(|e| Error::Message(e.to_string()))?,
    )?;
    Ok(())
}
fn quarantine_manifest(path: &Path) -> Result<()> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    fs::rename(
        path,
        path.with_file_name(format!("sync.json.obsolete-{stamp}")),
    )?;
    Ok(())
}
fn load_manifest(path: &Path) -> Result<Manifest> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let manifest = Manifest::current();
            write_manifest(path, &manifest)?;
            return Ok(manifest);
        }
        Err(error) => return Err(error.into()),
    };
    let value = serde_json::from_slice::<serde_json::Value>(&bytes).ok();
    let version = value
        .as_ref()
        .and_then(|value| value.get("schema_version"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;
    let mut manifest = match value {
        Some(value) if version <= MANIFEST_SCHEMA_VERSION => {
            serde_json::from_value::<Manifest>(value).ok()
        }
        _ => None,
    }
    .unwrap_or_else(|| Manifest::current());
    if version > MANIFEST_SCHEMA_VERSION
        || serde_json::from_slice::<serde_json::Value>(&bytes).is_err()
        || serde_json::from_slice::<Manifest>(&bytes).is_err()
    {
        quarantine_manifest(path)?;
    }
    let migrated = manifest.schema_version != MANIFEST_SCHEMA_VERSION;
    manifest.schema_version = MANIFEST_SCHEMA_VERSION;
    if migrated || !path.exists() {
        write_manifest(path, &manifest)?;
    }
    Ok(manifest)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct TrackedDirectory {
    remote: RemoteDirectory,
    #[serde(default)]
    last_synced_version: Option<String>,
}
impl TrackedDirectory {
    fn new(remote: RemoteDirectory) -> Self {
        let last_synced_version = Some(remote.version.clone());
        Self {
            remote,
            last_synced_version,
        }
    }
    fn version(&self) -> &str {
        self.last_synced_version
            .as_deref()
            .unwrap_or(&self.remote.version)
    }
}
#[derive(Default, Debug, Serialize)]
pub struct SyncReport {
    pub downloaded: usize,
    pub uploaded: usize,
    pub deleted: usize,
    pub unchanged: usize,
    pub directories_created: usize,
    pub directories_deleted: usize,
    pub conflicts: Vec<String>,
}
/// Reopen an existing Cloud checkout with three-way synchronization.
/// Only a first checkout is staged; unsynchronized local data is never discarded.
pub fn open_cloud_cache(
    provider: &mut dyn SyncProvider,
    project: &str,
    cache_root: &Path,
) -> Result<SyncReport> {
    fs::create_dir_all(cache_root)?;
    let root = cache_root.canonicalize()?;
    let key = blake3::hash(project.as_bytes()).to_hex().to_string();
    let cache = root.join(&key);
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(format!("{key}.lock")))?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .map_err(|_| Error::Message("Cloud project already opening".into()))?;
    let staging = root.join(format!("{key}.download"));
    // These paths are derived solely from a hash, never from remote filenames.
    for path in [&cache, &staging] {
        if path.exists()
            && (fs::symlink_metadata(path)?.file_type().is_symlink()
                || path.canonicalize()?.parent() != Some(root.as_path()))
        {
            return Err(Error::Message("Unsafe Cloud cache directory".into()));
        }
    }
    if cache.exists() {
        return synchronize(provider, project, &cache);
    }
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(staging.join(".luvyn"))?;
    let download = (|| {
        let mut manifest = Manifest::current();
        let mut report = SyncReport::default();
        for directory in provider.directories(project)? {
            if directory
                .path
                .split(['/', '\\'])
                .any(|part| matches!(part, ".luvyn" | ".git"))
            {
                continue;
            }
            fs::create_dir_all(safe_path(&staging, &directory.path)?)?;
            manifest
                .directories
                .insert(directory.path.clone(), TrackedDirectory::new(directory));
        }
        for file in provider.files(project)? {
            if !is_source(&file.path) {
                continue;
            }
            let path = safe_path(&staging, &file.path)?;
            if manifest.files.contains_key(&file.path) {
                return Err(Error::Message("Duplicate remote document path".into()));
            }
            let data = provider.download(&file)?;
            atomic_write(&path, &data)?;
            let hash = blake3::hash(&data).to_hex().to_string();
            manifest
                .files
                .insert(file.path.clone(), Tracked::new(file, hash));
            report.downloaded += 1;
        }
        write_manifest(&staging.join(".luvyn/sync.json"), &manifest)?;
        Ok(report)
    })();
    match download {
        Ok(report) => {
            if cache.exists() {
                return Err(Error::Message("Cloud cache appeared during download; reopen to synchronize without replacing it".into()));
            }
            fs::rename(&staging, &cache)?;
            Ok(report)
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            Err(error)
        }
    }
}
pub fn is_source(path: &str) -> bool {
    !path
        .split(['/', '\\'])
        .any(|p| matches!(p, ".luvyn" | ".git" | "target" | "node_modules" | "dist"))
        && (path.ends_with(".lyn")
            || matches!(
                path.rsplit('/').next(),
                Some("luvyn.toml" | ".ignore.luvyn" | ".gitignore" | ".luvynignore")
            ))
}
enum Change {
    Download(RemoteFile, Vec<u8>),
    Upload(String, Vec<u8>, Option<RemoteFile>),
    DeleteLocal(String),
    DeleteRemote(RemoteFile),
    Same(String, RemoteFile, String),
    Forget(String),
}
enum DirectoryChange {
    CreateRemote(String),
    CreateLocal(String),
    DeleteLocal(String),
    DeleteRemote(RemoteDirectory),
    Same(String, RemoteDirectory),
    Forget(String),
}
fn directory_is_source(path: &str, roots: &[String]) -> bool {
    !path
        .split('/')
        .any(|p| matches!(p, ".luvyn" | ".git" | "target" | "node_modules" | "dist"))
        && roots.iter().any(|root| {
            let root = root.replace('\\', "/");
            let root = root.trim_end_matches('/');
            root == "." || path == root || path.starts_with(&format!("{root}/"))
        })
}
pub fn synchronize(
    provider: &mut dyn SyncProvider,
    project: &str,
    root: &Path,
) -> Result<SyncReport> {
    fs::create_dir_all(root.join(".luvyn"))?;
    let canonical_root = root.canonicalize()?;
    let root = canonical_root.as_path();
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(".luvyn/sync.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock)
        .map_err(|_| Error::Message("Synchronization already running".into()))?;
    let manifest_path = root.join(".luvyn/sync.json");
    let mut manifest = load_manifest(&manifest_path)?;
    let mut remote = BTreeMap::new();
    for file in provider.files(project)? {
        if is_source(&file.path) {
            safe_path(root, &file.path)?;
            if remote.insert(file.path.clone(), file).is_some() {
                return Err(Error::Message(
                    "Duplicate remote document path; rename files in Drive before synchronizing"
                        .into(),
                ));
            }
        }
    }
    let workspace = crate::workspace::Project::open(root)?;
    let mut remote_directories = BTreeMap::new();
    for directory in provider.directories(project)? {
        if directory_is_source(&directory.path, &workspace.config.sources) {
            safe_path(root, &directory.path)?;
            if remote_directories
                .insert(directory.path.clone(), directory)
                .is_some()
            {
                return Err(Error::Message(
                    "Duplicate remote folder path; rename folders in Drive before synchronizing"
                        .into(),
                ));
            }
        }
    }
    let mut local = BTreeMap::new();
    let mut local_directories = std::collections::BTreeSet::new();
    for entry in workspace.walk(root)?.build() {
        let entry = entry.map_err(|e| Error::Message(e.to_string()))?;
        if entry.file_type().is_some_and(|t| t.is_dir())
            && let Ok(relative) = entry.path().strip_prefix(root)
        {
            let path = relative.to_string_lossy().replace('\\', "/");
            if !path.is_empty()
                && directory_is_source(&path, &workspace.config.sources)
                && !workspace.is_ignored(&path, true)?
            {
                local_directories.insert(path);
            }
        }
    }
    for entry in ignore::WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(false)
        .git_exclude(false)
        .parents(false)
        .filter_entry(|e| {
            !matches!(
                e.file_name().to_str(),
                Some(".git" | ".luvyn" | "target" | "node_modules" | "dist")
            )
        })
        .build()
    {
        let entry = entry.map_err(|e| Error::Message(e.to_string()))?;
        if entry.file_type().is_some_and(|t| t.is_file()) {
            let path = entry
                .path()
                .strip_prefix(root)
                .map_err(|e| Error::Message(e.to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            if is_source(&path) {
                let data = fs::read(safe_path(root, &path)?)?;
                if data.len() > 2 * 1024 * 1024 {
                    return Err(Error::Message(format!(
                        "Sync document exceeds 2 MiB: {path}"
                    )));
                }
                local.insert(path, data);
            }
        }
    }
    let paths: std::collections::BTreeSet<_> = local
        .keys()
        .chain(remote.keys())
        .chain(manifest.files.keys())
        .cloned()
        .collect();
    let mut report = SyncReport::default();
    let mut plan = vec![];
    for path in paths {
        let disk = local.get(&path);
        let cloud = remote.get(&path);
        let base = manifest.files.get(&path);
        let hash = disk.map(|b| blake3::hash(b).to_hex().to_string());
        let local_changed = hash.as_deref() != base.map(|b| b.hash.as_str());
        let remote_changed = cloud.map(|f| (f.id.as_str(), f.version.as_str()))
            != base.map(|b| (b.remote.id.as_str(), b.version()));
        if !local_changed && !remote_changed {
            report.unchanged += 1;
            continue;
        }
        if local_changed && remote_changed {
            if disk.is_none() && cloud.is_none() {
                plan.push(Change::Forget(path));
                continue;
            }
            if let (Some(disk), Some(cloud)) = (disk, cloud) {
                let data = provider.download(cloud)?;
                if data == *disk {
                    plan.push(Change::Same(path, cloud.clone(), hash.unwrap_or_default()));
                    continue;
                }
            }
            report.conflicts.push(path);
            continue;
        }
        if remote_changed {
            if let Some(cloud) = cloud {
                plan.push(Change::Download(cloud.clone(), provider.download(cloud)?));
            } else {
                plan.push(Change::DeleteLocal(path));
            }
        } else if let Some(disk) = disk {
            plan.push(Change::Upload(path, disk.clone(), cloud.cloned()));
        } else if let Some(cloud) = cloud {
            plan.push(Change::DeleteRemote(cloud.clone()));
        }
    }
    let directory_paths: std::collections::BTreeSet<_> = local_directories
        .iter()
        .chain(remote_directories.keys())
        .chain(manifest.directories.keys())
        .cloned()
        .collect();
    let mut directory_plan = Vec::new();
    for path in directory_paths {
        let disk = local_directories.contains(&path);
        let cloud = remote_directories.get(&path);
        let base = manifest.directories.get(&path);
        let local_changed = !disk;
        let remote_changed = cloud.map(|d| (d.id.as_str(), d.version.as_str()))
            != base.map(|b| (b.remote.id.as_str(), b.version()));
        if base.is_none() {
            match (disk, cloud) {
                (true, Some(cloud)) => {
                    directory_plan.push(DirectoryChange::Same(path, cloud.clone()))
                }
                (true, None) => directory_plan.push(DirectoryChange::CreateRemote(path)),
                (false, Some(_)) => directory_plan.push(DirectoryChange::CreateLocal(path)),
                (false, None) => {}
            }
        } else {
            // The manifest records that both sides had this folder last time.
            match (local_changed, cloud, remote_changed) {
                (true, None, _) => directory_plan.push(DirectoryChange::Forget(path)),
                (true, Some(_), true) => report.conflicts.push(path),
                (true, Some(cloud), false) => {
                    directory_plan.push(DirectoryChange::DeleteRemote(cloud.clone()))
                }
                (false, None, true) => directory_plan.push(DirectoryChange::DeleteLocal(path)),
                (false, Some(cloud), true) => {
                    directory_plan.push(DirectoryChange::Same(path, cloud.clone()))
                }
                (false, Some(_), false) => {}
                (false, None, false) => directory_plan.push(DirectoryChange::DeleteLocal(path)),
            }
        }
    }
    // No partial writes when any conflict is detected. User reviews both versions explicitly.
    if !report.conflicts.is_empty() {
        return Ok(report);
    }
    // External editors may change the cache during network requests.
    for (path, initial) in &local {
        if fs::read(safe_path(root, path)?).ok().as_deref() != Some(initial.as_slice()) {
            report.conflicts.push(path.clone());
        }
    }
    for path in remote.keys().filter(|p| !local.contains_key(*p)) {
        if safe_path(root, path)?.exists() {
            report.conflicts.push(path.clone());
        }
    }
    if !report.conflicts.is_empty() {
        return Ok(report);
    }
    for change in plan {
        match change {
            Change::Forget(path) => {
                manifest.files.remove(&path);
                report.unchanged += 1;
            }
            Change::Download(file, data) => {
                if data.len() > 2 * 1024 * 1024 {
                    return Err(Error::Message("Remote document exceeds 2 MiB".into()));
                }
                atomic_write(&safe_path(root, &file.path)?, &data)?;
                manifest.files.insert(
                    file.path.clone(),
                    Tracked::new(file, blake3::hash(&data).to_hex().to_string()),
                );
                report.downloaded += 1;
            }
            Change::Upload(path, data, expected) => {
                let file = provider.upload(project, &path, &data, expected.as_ref())?;
                manifest.files.insert(
                    path,
                    Tracked::new(file, blake3::hash(&data).to_hex().to_string()),
                );
                report.uploaded += 1;
            }
            Change::DeleteLocal(path) => {
                fs::remove_file(safe_path(root, &path)?)?;
                manifest.files.remove(&path);
                report.deleted += 1;
            }
            Change::DeleteRemote(file) => {
                provider.delete(&file)?;
                manifest.files.remove(&file.path);
                report.deleted += 1;
            }
            Change::Same(path, remote, hash) => {
                manifest.files.insert(path, Tracked::new(remote, hash));
                report.unchanged += 1;
            }
        }
        write_manifest(&manifest_path, &manifest)?;
    }
    // File changes run first so a folder removed from the tree is empty by the
    // time its Drive directory is trashed. Parent removals run after children.
    directory_plan.sort_by(|a, b| {
        let path = |c: &DirectoryChange| match c {
            DirectoryChange::CreateRemote(p)
            | DirectoryChange::CreateLocal(p)
            | DirectoryChange::DeleteLocal(p)
            | DirectoryChange::Same(p, _)
            | DirectoryChange::Forget(p) => p.clone(),
            DirectoryChange::DeleteRemote(d) => d.path.clone(),
        };
        let (a_path, b_path) = (path(a), path(b));
        let a_deleting = matches!(
            a,
            DirectoryChange::DeleteLocal(_) | DirectoryChange::DeleteRemote(_)
        );
        let b_deleting = matches!(
            b,
            DirectoryChange::DeleteLocal(_) | DirectoryChange::DeleteRemote(_)
        );
        match (a_deleting, b_deleting) {
            (false, true) => std::cmp::Ordering::Less,
            (true, false) => std::cmp::Ordering::Greater,
            (true, true) => b_path
                .matches('/')
                .count()
                .cmp(&a_path.matches('/').count())
                .then_with(|| b_path.cmp(&a_path)),
            (false, false) => a_path
                .matches('/')
                .count()
                .cmp(&b_path.matches('/').count())
                .then_with(|| a_path.cmp(&b_path)),
        }
    });
    for change in directory_plan {
        match change {
            DirectoryChange::CreateRemote(path) => {
                let directory = provider.create_directory(project, &path)?;
                manifest
                    .directories
                    .insert(path, TrackedDirectory::new(directory));
                report.directories_created += 1;
            }
            DirectoryChange::CreateLocal(path) => {
                fs::create_dir_all(safe_path(root, &path)?)?;
                let directory = remote_directories
                    .get(&path)
                    .cloned()
                    .expect("planned remote directory");
                manifest
                    .directories
                    .insert(path, TrackedDirectory::new(directory));
                report.directories_created += 1;
            }
            DirectoryChange::DeleteLocal(path) => {
                fs::remove_dir(safe_path(root, &path)?)?;
                manifest.directories.remove(&path);
                report.directories_deleted += 1;
            }
            DirectoryChange::DeleteRemote(directory) => {
                provider.delete_directory(&directory)?;
                manifest.directories.remove(&directory.path);
                report.directories_deleted += 1;
            }
            DirectoryChange::Same(path, directory) => {
                manifest
                    .directories
                    .insert(path, TrackedDirectory::new(directory));
            }
            DirectoryChange::Forget(path) => {
                manifest.directories.remove(&path);
            }
        }
        write_manifest(&manifest_path, &manifest)?;
    }
    Ok(report)
}
