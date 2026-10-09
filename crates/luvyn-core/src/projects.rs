//! Small, platform-neutral project registry. Hosts supply their private storage directory.
use crate::{Error, Result, workspace::atomic_write};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecentProject {
    pub id: String,
    pub name: String,
    pub path: String,
    pub kind: String,
    pub last_access: u64,
    pub cloud_id: Option<String>,
    #[serde(default)]
    pub unavailable: bool,
}
pub struct ProjectRegistry {
    directory: PathBuf,
}
const REGISTRY_SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct RegistryFile {
    schema_version: u32,
    projects: Vec<RecentProject>,
}

fn quarantine(path: &Path, label: &str) -> Result<()> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let destination = path.with_file_name(format!("{}.obsolete-{stamp}", label));
    fs::rename(path, destination)?;
    Ok(())
}

fn read_registry(path: &Path) -> Result<(Vec<RecentProject>, bool)> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), true));
        }
        Err(error) => return Err(error.into()),
    };
    let parsed = if bytes.len() <= 1024 * 1024 {
        serde_json::from_slice::<serde_json::Value>(&bytes).ok()
    } else {
        None
    };
    let Some(value) = parsed else {
        quarantine(path, "projects.json")?;
        return Ok((Vec::new(), true));
    };
    if value.is_array() {
        return match serde_json::from_value::<Vec<RecentProject>>(value) {
            Ok(projects) => Ok((projects, true)),
            Err(_) => {
                quarantine(path, "projects.json")?;
                Ok((Vec::new(), true))
            }
        };
    }
    let version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;
    if version > REGISTRY_SCHEMA_VERSION {
        quarantine(path, "projects.json")?;
        return Ok((Vec::new(), true));
    }
    match serde_json::from_value::<RegistryFile>(value) {
        Ok(mut registry) => {
            registry.schema_version = REGISTRY_SCHEMA_VERSION;
            Ok((registry.projects, version != REGISTRY_SCHEMA_VERSION))
        }
        Err(_) => {
            quarantine(path, "projects.json")?;
            Ok((Vec::new(), true))
        }
    }
}

fn registry_bytes(projects: &[RecentProject]) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(&RegistryFile {
        schema_version: REGISTRY_SCHEMA_VERSION,
        projects: projects.to_vec(),
    })
    .map_err(|e| Error::Message(e.to_string()))
}
impl ProjectRegistry {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    fn update<T>(&self, action: impl FnOnce(&mut Vec<RecentProject>) -> Result<T>) -> Result<T> {
        fs::create_dir_all(&self.directory)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.directory.join("projects.lock"))?;
        fs2::FileExt::lock_exclusive(&lock)?;
        let path = self.directory.join("projects.json");
        let (mut projects, migrated) = read_registry(&path)?;
        let before = registry_bytes(&projects)?;
        let result = action(&mut projects)?;
        projects.sort_by_key(|p| std::cmp::Reverse(p.last_access));
        projects.truncate(200);
        let after = registry_bytes(&projects)?;
        if migrated || before != after {
            atomic_write(&path, &after)?;
        }
        Ok(result)
    }
    pub fn list(&self) -> Result<Vec<RecentProject>> {
        self.update(|projects| {
            let mut result = projects.clone();
            for p in &mut result {
                p.unavailable = p.kind == "local"
                    && !p.path.starts_with("content://")
                    && !Path::new(&p.path).is_dir();
            }
            Ok(result)
        })
    }
    pub fn remember(
        &self,
        name: String,
        path: String,
        kind: &str,
        cloud_id: Option<String>,
    ) -> Result<RecentProject> {
        let identity = format!("{kind}:{}", cloud_id.as_deref().unwrap_or(&path));
        let id = blake3::hash(identity.as_bytes()).to_hex().to_string();
        let entry = RecentProject {
            id: id.clone(),
            name,
            path,
            kind: kind.into(),
            last_access: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            cloud_id,
            unavailable: false,
        };
        self.update(|projects| {
            projects.retain(|p| p.id != id);
            projects.push(entry.clone());
            Ok(entry)
        })
    }
    pub fn forget(&self, id: &str) -> Result<()> {
        self.update(|projects| {
            projects.retain(|p| p.id != id);
            Ok(())
        })
    }
}

/// Desktop/server host storage, independent of any documentation workspace.
pub fn data_directory() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("LUVYN_DATA_DIR") {
        return Ok(path.into());
    }
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base =
        std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")));
    base.map(|p| p.join("Luvyn")).ok_or_else(|| {
        Error::Message(
            "No platform application storage directory available; set LUVYN_DATA_DIR".into(),
        )
    })
}
pub fn launcher_directory() -> Result<PathBuf> {
    let path = data_directory()?.join("launcher");
    fs::create_dir_all(&path)?;
    Ok(path)
}
pub fn is_launcher(root: &Path) -> bool {
    launcher_directory().is_ok_and(|p| p.canonicalize().ok() == root.canonicalize().ok())
}
