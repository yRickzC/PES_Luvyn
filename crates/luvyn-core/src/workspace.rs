use crate::{Error, Result, binary, model::*, parser, resolver};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const MAX_SOURCE: usize = 2 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    pub project: ProjectSettings,
    pub sources: Vec<String>,
    pub ignore: Vec<String>,
    pub output: String,
    pub export: String,
    pub autosave: bool,
}
impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            project: ProjectSettings::default(),
            sources: vec![".".into()],
            ignore: vec![],
            output: ".luvyn/project.lu".into(),
            export: ".luvyn/context.zip".into(),
            autosave: false,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSettings {
    pub target: Option<String>,
    /// Artifact Git policy in the repository where `luvyn git` runs.
    pub artifacts: String,
}
impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            target: None,
            artifacts: "ignore".into(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct CachedFile {
    hash: String,
    parsed: ParsedFile,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Cache {
    version: u32,
    files: BTreeMap<String, CachedFile>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BuildStats {
    pub files: usize,
    pub parsed: usize,
    pub reused: usize,
    pub symbols: usize,
    pub edges: usize,
}
pub struct Project {
    /// Compatibility name for documentation_root; source/editor paths always use this root.
    pub root: PathBuf,
    pub target_project_root: PathBuf,
    pub artifact_root: PathBuf,
    pub config: ProjectConfig,
    cache: Cache,
    pub graph: Graph,
    pub stats: BuildStats,
    target_override: Option<PathBuf>,
}

impl Project {
    pub fn open(root: &Path) -> Result<Self> {
        Self::open_with_target(root, None)
    }
    /// Portable hosts stage provider documents locally and supply a platform-managed target.
    /// This does not alter the persisted configuration or add platform dependencies to Core.
    pub fn open_with_target(root: &Path, target_override: Option<&Path>) -> Result<Self> {
        let root = root
            .canonicalize()
            .map_err(|e| Error::Message(format!("Workspace {}: {e}", root.display())))?;
        if !root.is_dir() {
            return Err(Error::Message("Workspace must be a directory".into()));
        }
        let config_path = safe_path(&root, "luvyn.toml")?;
        let config = if config_path.exists() {
            toml::from_str(&read_source(&config_path)?)
                .map_err(|e| Error::Message(format!("luvyn.toml: {e}")))?
        } else {
            ProjectConfig::default()
        };
        let cache_path = safe_path(&root, ".luvyn/cache.bin")?;
        let cache = fs::metadata(&cache_path)
            .ok()
            .filter(|m| m.len() < 64 * 1024 * 1024)
            .and_then(|_| fs::read(cache_path).ok())
            .and_then(|b| postcard::from_bytes::<Cache>(&b).ok())
            .filter(|c| c.version == 4)
            .unwrap_or_default();
        let target_project_root = if let Some(target) = target_override {
            target.canonicalize()?
        } else {
            target_root(&root, &config)?
        };
        let artifact_root = safe_path(&target_project_root, ".luvyn")?;
        let project = Self {
            root,
            target_project_root,
            artifact_root,
            config,
            cache,
            graph: Graph::default(),
            stats: BuildStats::default(),
            target_override: target_override.map(Path::to_owned),
        };
        // Reject unsafe configured destinations before any write.
        project.output_path()?;
        project.safe_path(&project.config.export)?;
        Ok(project)
    }
    pub fn safe_path(&self, relative: &str) -> Result<PathBuf> {
        safe_path(&self.root, relative)
    }
    pub fn documentation_root(&self) -> &Path {
        &self.root
    }
    pub fn output_path(&self) -> Result<PathBuf> {
        safe_path(&self.target_project_root, &self.config.output)
    }
    pub fn parse_document(&self, path: &str, source: &str) -> ParsedFile {
        let relative = self
            .config
            .sources
            .iter()
            .filter_map(|root| {
                let root = root.trim_end_matches('/').trim_end_matches('\\');
                if root == "." {
                    Some(path)
                } else {
                    path.strip_prefix(root).and_then(|p| p.strip_prefix('/'))
                }
            })
            .min_by_key(|p| p.len())
            .unwrap_or(path);
        parser::parse_in_module(path, source, &parser::module_name(relative))
    }
    pub fn walk(&self, start: impl AsRef<Path>) -> Result<ignore::WalkBuilder> {
        let mut builder = ignore::WalkBuilder::new(start);
        builder
            .hidden(false)
            .follow_links(false)
            .require_git(false)
            .add_custom_ignore_filename(".ignore.luvyn")
            .add_custom_ignore_filename(".luvynignore");
        let root = self.root.clone();
        let patterns = self.config.ignore.clone();
        let mut glob = ignore::gitignore::GitignoreBuilder::new(&root);
        let ignore_file = root.join(".ignore.luvyn");
        if ignore_file.exists()
            && let Some(error) = glob.add(ignore_file)
        {
            return Err(Error::Message(error.to_string()));
        }
        for p in &patterns {
            glob.add_line(None, p)
                .map_err(|e| Error::Message(format!("ignore pattern: {e}")))?;
        }
        let glob = glob.build().map_err(|e| Error::Message(e.to_string()))?;
        builder.filter_entry(move |e| {
            let name = e.file_name().to_string_lossy();
            !matches!(
                name.as_ref(),
                ".git" | ".luvyn" | "target" | "node_modules" | "graphify-out" | "dist"
            ) && !glob
                .matched_path_or_any_parents(e.path(), e.file_type().is_some_and(|f| f.is_dir()))
                .is_ignore()
        });
        Ok(builder)
    }
    pub fn discover(&self) -> Result<Vec<String>> {
        let mut found = Vec::new();
        for source in &self.config.sources {
            let start = self.safe_path(source)?;
            if !start.exists() {
                return Err(Error::Message(format!("Source root not found: {source}")));
            }
            let builder = self.walk(start)?;
            for entry in builder.build() {
                let entry = entry.map_err(|e| Error::Message(e.to_string()))?;
                if entry.file_type().is_some_and(|f| f.is_file())
                    && entry.path().extension().is_some_and(|e| e == "lyn")
                {
                    let relative = entry
                        .path()
                        .strip_prefix(&self.root)
                        .map_err(|e| Error::Message(e.to_string()))?
                        .to_string_lossy()
                        .replace('\\', "/");
                    found.push(relative);
                    if found.len() > 100_000 {
                        return Err(Error::Message(
                            "Workspace exceeds 100,000 documents; narrow source roots".into(),
                        ));
                    }
                }
            }
        }
        found.sort();
        found.dedup();
        Ok(found)
    }
    pub fn is_source(&self, path: &str) -> bool {
        let path = path.replace('\\', "/");
        self.config.sources.iter().any(|root| {
            let root = root.replace('\\', "/");
            let root = root.trim_end_matches('/');
            root == "." || path == root || path.starts_with(&format!("{root}/"))
        })
    }
    /// Same mature gitignore matcher for overlay eligibility and IDE writes.
    pub fn is_ignored(&self, relative: &str, is_dir: bool) -> Result<bool> {
        let path = self.safe_path(relative)?;
        if Path::new(relative).components().any(|c| {
            matches!(
                c.as_os_str().to_str(),
                Some(".git" | ".luvyn" | "target" | "node_modules" | "dist" | "graphify-out")
            )
        }) {
            return Ok(true);
        }
        let mut builder = ignore::gitignore::GitignoreBuilder::new(&self.root);
        for pattern in &self.config.ignore {
            builder
                .add_line(None, pattern)
                .map_err(|e| Error::Message(e.to_string()))?;
        }
        let mut directories = Vec::new();
        let mut parent = path.parent();
        while let Some(dir) = parent {
            if !dir.starts_with(&self.root) {
                break;
            }
            directories.push(dir.to_path_buf());
            parent = dir.parent();
        }
        for dir in directories.into_iter().rev() {
            for name in [".gitignore", ".luvynignore", ".ignore.luvyn"] {
                let file = dir.join(name);
                if file.exists()
                    && let Some(error) = builder.add(file)
                {
                    return Err(Error::Message(error.to_string()));
                }
            }
        }
        let matcher = builder.build().map_err(|e| Error::Message(e.to_string()))?;
        Ok(matcher
            .matched_path_or_any_parents(path, is_dir)
            .is_ignore())
    }
    /// Only changed documents are parsed. Resolution uses cached semantic documents.
    pub fn analyze(&mut self, overlays: &BTreeMap<String, String>) -> Result<&Graph> {
        let config_path = self.safe_path("luvyn.toml")?;
        let config: ProjectConfig = if config_path.exists() {
            toml::from_str(&read_source(&config_path)?)
                .map_err(|e| Error::Message(format!("luvyn.toml: {e}")))?
        } else {
            ProjectConfig::default()
        };
        let target = if let Some(target) = &self.target_override {
            target.canonicalize()?
        } else {
            target_root(&self.root, &config)?
        };
        safe_path(&target, &config.output)?;
        self.safe_path(&config.export)?;
        self.config = config;
        self.target_project_root = target;
        self.artifact_root = safe_path(&self.target_project_root, ".luvyn")?;
        let files = self.discover()?;
        let mut active = std::collections::BTreeSet::new();
        let mut sources = BTreeMap::new();
        let mut diagnostics = Vec::new();
        self.stats = BuildStats::default();
        for path in files
            .into_iter()
            .chain(overlays.keys().filter(|p| p.ends_with(".lyn")).cloned())
        {
            if overlays.contains_key(&path)
                && (!self.is_source(&path) || self.is_ignored(&path, false)?)
            {
                continue;
            }
            if !active.insert(path.clone()) {
                continue;
            }
            let full = self.safe_path(&path)?;
            let content = match overlays.get(&path) {
                Some(text) => Ok(text.clone()),
                None => read_source(&full),
            };
            let content = match content {
                Ok(c) if c.len() <= MAX_SOURCE => c,
                Ok(_) => {
                    diagnostics.push(Diagnostic::error(
                        "I001",
                        "Document exceeds 2 MiB",
                        Location {
                            file: path,
                            line: 1,
                            column: 1,
                            length: 1,
                        },
                    ));
                    continue;
                }
                Err(e) => {
                    diagnostics.push(Diagnostic::error(
                        "I002",
                        e.to_string(),
                        Location {
                            file: path,
                            line: 1,
                            column: 1,
                            length: 1,
                        },
                    ));
                    continue;
                }
            };
            let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
            let cache_hash =
                blake3::hash(format!("{}:{hash}", self.config.sources.join(";")).as_bytes())
                    .to_hex()
                    .to_string();
            sources.insert(path.clone(), hash.clone());
            self.stats.files += 1;
            if self
                .cache
                .files
                .get(&path)
                .is_some_and(|cached| cached.hash == cache_hash)
            {
                self.stats.reused += 1;
            } else {
                self.cache.files.insert(
                    path.clone(),
                    CachedFile {
                        hash: cache_hash,
                        parsed: self.parse_document(&path, &content),
                    },
                );
                self.stats.parsed += 1;
            }
        }
        self.cache.files.retain(|path, _| active.contains(path));
        let parsed: Vec<_> = self
            .cache
            .files
            .iter()
            .filter(|(p, _)| sources.contains_key(*p))
            .map(|(_, f)| f.parsed.clone())
            .collect();
        self.graph = resolver::resolve(&parsed, sources);
        self.graph.diagnostics.extend(diagnostics);
        self.stats.symbols = self.graph.symbols.len();
        self.stats.edges = self.graph.edges.len();
        Ok(&self.graph)
    }
    pub fn build(&mut self) -> Result<BuildStats> {
        self.build_with_progress(|_| {})
    }
    pub fn build_with_progress(&mut self, mut progress: impl FnMut(String)) -> Result<BuildStats> {
        // Refresh target before locking: two documentation roots may share one target.
        self.analyze(&BTreeMap::new())?;
        let lock_path = safe_path(&self.target_project_root, ".luvyn/build.lock")?;
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| {
            Error::Message(
                "Another build is writing this workspace artifact; retry after it finishes".into(),
            )
        })?;
        let started = std::time::Instant::now();
        progress(format!("Docs: {}", self.root.display()));
        progress(format!("Target: {}", self.target_project_root.display()));
        progress("[build] scanning, parsing changed files, resolving and validating".into());
        progress(format!(
            "[build] {} .lyn files; {} parsed / {} reused",
            self.stats.files, self.stats.parsed, self.stats.reused
        ));
        if self.graph.has_errors() {
            progress("[build] validation failed; last good artifact preserved".into());
            return Err(Error::Message(
                "Build failed; fix reported diagnostics".into(),
            ));
        }
        progress(format!(
            "[build] graph: {} nodes / {} edges",
            self.stats.symbols, self.stats.edges
        ));
        binary::write(&self.output_path()?, &self.graph)?;
        progress(format!("[build] wrote {}", self.output_path()?.display()));
        self.cache.version = 4;
        let cache =
            postcard::to_allocvec(&self.cache).map_err(|e| Error::Message(e.to_string()))?;
        atomic_write(&self.safe_path(".luvyn/cache.bin")?, &cache)?;
        progress(format!(
            "[build] finished in {}ms",
            started.elapsed().as_millis()
        ));
        Ok(self.stats.clone())
    }
    pub fn compiled(&self) -> Result<Graph> {
        binary::read(&self.output_path()?)
    }
    pub fn parsed(&self, path: &str) -> Option<&ParsedFile> {
        self.cache.files.get(path).map(|c| &c.parsed)
    }
    pub fn stale(&self, graph: &Graph) -> Result<bool> {
        let paths = self.discover()?;
        if paths.len() != graph.sources.len() {
            return Ok(true);
        }
        for path in paths {
            let text = read_source(&self.safe_path(&path)?)?;
            if graph
                .sources
                .get(&path)
                .is_none_or(|hash| hash != &blake3::hash(text.as_bytes()).to_hex().to_string())
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
fn target_root(root: &Path, config: &ProjectConfig) -> Result<PathBuf> {
    if !matches!(config.project.artifacts.as_str(), "ignore" | "track") {
        return Err(Error::Message(
            "project.artifacts must be ignore or track".into(),
        ));
    }
    let path = config
        .project
        .target
        .as_ref()
        .map_or_else(|| root.to_path_buf(), |target| root.join(target));
    let target = path.canonicalize().map_err(|e| {
        Error::Message(format!(
            "Target project {}: {e}; configure an existing directory",
            path.display()
        ))
    })?;
    if !target.is_dir() {
        return Err(Error::Message("Target project must be a directory".into()));
    }
    Ok(target)
}
pub fn read_source(path: &Path) -> Result<String> {
    if fs::metadata(path)?.len() > MAX_SOURCE as u64 {
        return Err(Error::Message(format!("{} exceeds 2 MiB", path.display())));
    }
    let mut data = Vec::new();
    fs::File::open(path)?
        .take(MAX_SOURCE as u64 + 1)
        .read_to_end(&mut data)?;
    if data.len() > MAX_SOURCE {
        return Err(Error::Message(format!("{} exceeds 2 MiB", path.display())));
    }
    String::from_utf8(data)
        .map_err(|_| Error::Message(format!("{}: expected UTF-8 encoding", path.display())))
}
pub fn safe_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return Err(Error::Message("Path must stay inside the workspace".into()));
    }
    if relative.components().any(|c| c.as_os_str() == ".git") {
        return Err(Error::Message("Git internals are protected".into()));
    }
    let path = root.join(relative);
    let mut existing = path.as_path();
    while !existing.exists() {
        existing = existing
            .parent()
            .ok_or_else(|| Error::Message("Invalid path".into()))?;
    }
    let canonical = existing.canonicalize()?;
    if !canonical.starts_with(root) {
        return Err(Error::Message("Symlink escapes the workspace".into()));
    }
    // A dangling symlink must not be treated as a new file/directory.
    let mut cursor = root.to_path_buf();
    for component in relative.components() {
        cursor.push(component.as_os_str());
        if fs::symlink_metadata(&cursor).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::Message("Symlink editing is not supported".into()));
        }
    }
    Ok(path)
}
static TEMP_ID: AtomicU64 = AtomicU64::new(0);
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Message("Invalid output path".into()))?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| Error::Message("Invalid file name".into()))?
        .to_string_lossy();
    let temp = parent.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let write = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if write.is_err() {
        let _ = fs::remove_file(&temp);
    }
    write
}
