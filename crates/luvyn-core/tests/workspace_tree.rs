use luvyn_core::{
    Error, Project, Result, editor,
    ide::{self, IdeCore},
    sync::{self, RemoteDirectory, RemoteFile, RemoteProject, SyncProvider},
};
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn ide_tree_uses_nested_filesystem_and_confirmed_folder_delete() {
    let root = tempfile::tempdir().unwrap();
    let project = Project::open(root.path()).unwrap();
    let ide = IdeCore::new(project);

    ide::handle(&ide, json!({"op":"mkdir","file":"docs/player"})).unwrap();
    ide::handle(
        &ide,
        json!({"op":"create","file":"docs/player/Player.lyn","text":"class Player\n"}),
    )
    .unwrap();
    ide::handle(&ide, json!({"op":"mkdir","file":"docs/systems"})).unwrap();
    ide::handle(
        &ide,
        json!({"op":"create","file":"docs/systems/Movement.lyn","text":"class Movement\n"}),
    )
    .unwrap();
    ide::handle(
        &ide,
        json!({"op":"create","file":"main.lyn","text":"main:\n    depends:\n        Player\n"}),
    )
    .unwrap();
    ide::handle(
        &ide,
        json!({"op":"create","file":"docs/Unrelated.lyn","text":"class Unrelated\n"}),
    )
    .unwrap();

    let snapshot = ide::handle(&ide, json!({"op":"snapshot"})).unwrap();
    let files: Vec<_> = snapshot["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["path"].as_str().unwrap())
        .collect();
    let folders: Vec<_> = snapshot["folders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry.as_str().unwrap())
        .collect();
    assert!(files.contains(&"docs/player/Player.lyn"));
    assert!(files.contains(&"docs/systems/Movement.lyn"));
    assert!(folders.contains(&"docs/player"));
    assert!(folders.contains(&"docs/systems"));

    let graph = ide::handle(&ide, json!({"op":"graph","limit":80})).unwrap();
    let graph_names: Vec<_> = graph["result"]["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|symbol| symbol["name"].as_str().unwrap())
        .collect();
    assert!(graph_names.contains(&"Player"));
    assert!(graph_names.contains(&"Movement"));
    assert!(graph_names.contains(&"Unrelated"));

    ide::handle(
        &ide,
        json!({"op":"delete","file":"docs/player","recursive":true}),
    )
    .unwrap();
    assert!(!root.path().join("docs/player").exists());
    assert!(root.path().join("docs/systems/Movement.lyn").is_file());
}

#[test]
fn shared_completion_provider_covers_fields_depends_and_generic_return_types() {
    let root = tempfile::tempdir().unwrap();
    let source = "class User\npurpose: user\nfields:\n    id: i64\nmain:\n    depends:\n        User\nfunc test<T>(value: T) -> Option<User>\npurpose: test\n";
    std::fs::write(root.path().join("main.lyn"), source).unwrap();
    let mut project = Project::open(root.path()).unwrap();
    project.analyze(&BTreeMap::new()).unwrap();
    let at_line_end = |needle: &str| {
        let line = source.lines().position(|text| text == needle).unwrap() as u32 + 1;
        let column = needle.chars().count() as u32 + 1;
        editor::completions_at(&project, "main.lyn", source, line, column)
    };
    assert!(
        at_line_end("    id: i64")
            .iter()
            .any(|item| item.label == "i64")
    );
    assert!(
        at_line_end("        User")
            .iter()
            .any(|item| item.label == "User")
    );
    let signature = "func test<T>(value: T) -> Option<User>";
    let line = source.lines().position(|text| text == signature).unwrap() as u32 + 1;
    let column = signature.find("-> ").unwrap() as u32 + 4;
    let items = editor::completions_at(&project, "main.lyn", source, line, column);
    assert!(
        items
            .iter()
            .any(|item| item.label == "T" && item.detail.contains("Generic parameter"))
    );
    assert!(items.iter().any(|item| item.label == "Option"));
}

#[derive(Default)]
struct MemoryDrive {
    directories: BTreeMap<String, RemoteDirectory>,
    files: BTreeMap<String, (RemoteFile, Vec<u8>)>,
    fail_download: bool,
}
impl SyncProvider for MemoryDrive {
    fn projects(&mut self) -> Result<Vec<RemoteProject>> {
        Ok(vec![])
    }
    fn create_project(&mut self, name: &str) -> Result<RemoteProject> {
        Ok(RemoteProject {
            id: name.into(),
            name: name.into(),
        })
    }
    fn files(&mut self, _: &str) -> Result<Vec<RemoteFile>> {
        Ok(self.files.values().map(|(file, _)| file.clone()).collect())
    }
    fn directories(&mut self, _: &str) -> Result<Vec<RemoteDirectory>> {
        Ok(self.directories.values().cloned().collect())
    }
    fn create_directory(&mut self, _: &str, path: &str) -> Result<RemoteDirectory> {
        let directory = RemoteDirectory {
            id: path.into(),
            path: path.into(),
            version: "1".into(),
        };
        self.directories.insert(path.into(), directory.clone());
        Ok(directory)
    }
    fn delete_directory(&mut self, directory: &RemoteDirectory) -> Result<()> {
        if self
            .directories
            .keys()
            .any(|path| path.starts_with(&format!("{}/", directory.path)))
        {
            return Err(Error::Message(format!(
                "Folder is not empty: {}",
                directory.path
            )));
        }
        self.directories.remove(&directory.path);
        Ok(())
    }
    fn download(&mut self, file: &RemoteFile) -> Result<Vec<u8>> {
        if self.fail_download {
            return Err(Error::Message("Network unavailable".into()));
        }
        Ok(self.files[&file.path].1.clone())
    }
    fn upload(
        &mut self,
        _: &str,
        path: &str,
        data: &[u8],
        _: Option<&RemoteFile>,
    ) -> Result<RemoteFile> {
        let file = RemoteFile {
            id: path.into(),
            path: path.into(),
            version: "1".into(),
        };
        self.files
            .insert(path.into(), (file.clone(), data.to_vec()));
        Ok(file)
    }
    fn delete(&mut self, file: &RemoteFile) -> Result<()> {
        self.files.remove(&file.path);
        Ok(())
    }
}

#[test]
fn cloud_cache_failed_download_keeps_existing_checkout_and_other_projects() {
    let storage = tempfile::tempdir().unwrap();
    let cache = storage
        .path()
        .join(blake3::hash(b"project").to_hex().as_str());
    let other = storage.path().join("local-project");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(cache.join("main.lyn"), b"cached").unwrap();
    std::fs::write(other.join("main.lyn"), b"user document").unwrap();
    let mut drive = MemoryDrive::default();
    drive
        .upload("project", "main.lyn", b"remote", None)
        .unwrap();
    drive.fail_download = true;
    assert!(sync::open_cloud_cache(&mut drive, "project", storage.path()).is_err());
    assert_eq!(std::fs::read(cache.join("main.lyn")).unwrap(), b"cached");
    assert_eq!(
        std::fs::read(other.join("main.lyn")).unwrap(),
        b"user document"
    );
    assert_eq!(drive.files["main.lyn"].1, b"remote");
    assert!(
        !storage
            .path()
            .join(format!("{}.download", blake3::hash(b"project").to_hex()))
            .exists()
    );
}

#[test]
fn cloud_open_recreates_conflicting_and_corrupt_cache_without_changing_drive() {
    let storage = tempfile::tempdir().unwrap();
    let cache = storage
        .path()
        .join(blake3::hash(b"project").to_hex().as_str());
    let mut drive = MemoryDrive::default();
    drive
        .upload("project", "main.lyn", b"main:\n    purpose: remote\n", None)
        .unwrap();
    drive
        .upload(
            "project",
            "docs/player/Player.lyn",
            b"entity Player\n",
            None,
        )
        .unwrap();
    drive.create_directory("project", "docs/empty").unwrap();
    drive.create_directory("project", "docs").unwrap();
    drive.create_directory("project", "docs/player").unwrap();
    sync::open_cloud_cache(&mut drive, "project", storage.path()).unwrap();
    for manifest in ["valid", "corrupt", "future"] {
        std::fs::write(cache.join("main.lyn"), b"stale conflict").unwrap();
        std::fs::write(cache.join("obsolete.lyn"), b"stale only").unwrap();
        std::fs::write(cache.join("luvyn.toml"), b"invalid = [").unwrap();
        if manifest != "valid" {
            std::fs::write(
                cache.join(".luvyn/sync.json"),
                if manifest == "corrupt" {
                    "broken"
                } else {
                    "{\"schema_version\":999}"
                },
            )
            .unwrap();
        }
        let report = sync::open_cloud_cache(&mut drive, "project", storage.path()).unwrap();
        assert!(report.conflicts.is_empty());
        assert_eq!(report.downloaded, 2);
        assert_eq!(report.uploaded, 0);
        assert_eq!(
            std::fs::read(cache.join("main.lyn")).unwrap(),
            drive.files["main.lyn"].1
        );
        assert!(!cache.join("obsolete.lyn").exists());
        assert!(!cache.join("luvyn.toml").exists());
        assert!(cache.join("docs/player/Player.lyn").is_file());
        assert!(cache.join("docs/empty").is_dir());
        assert_eq!(drive.files.len(), 2);
        assert_eq!(drive.directories.len(), 3);
        assert_eq!(
            sync::synchronize(&mut drive, "project", &cache)
                .unwrap()
                .unchanged,
            2
        );
    }
}

#[test]
fn sync_preserves_nested_local_and_remote_folders_and_deletes_empty_tree() {
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(source.path().join("docs/player")).unwrap();
    std::fs::create_dir_all(source.path().join("docs/systems")).unwrap();
    std::fs::write(
        source.path().join("docs/player/Player.lyn"),
        "class Player\n",
    )
    .unwrap();
    std::fs::write(
        source.path().join("docs/systems/Movement.lyn"),
        "class Movement\n",
    )
    .unwrap();
    let mut drive = MemoryDrive::default();
    let report = sync::synchronize(&mut drive, "project", source.path()).unwrap();
    assert_eq!(report.uploaded, 2);
    assert!(drive.directories.contains_key("docs/player"));
    assert!(drive.directories.contains_key("docs/systems"));
    assert!(drive.files.contains_key("docs/player/Player.lyn"));

    let cache = tempfile::tempdir().unwrap();
    let downloaded = sync::synchronize(&mut drive, "project", cache.path()).unwrap();
    assert_eq!(downloaded.downloaded, 2);
    assert!(cache.path().join("docs/player/Player.lyn").is_file());
    assert!(cache.path().join("docs/systems/Movement.lyn").is_file());

    std::fs::remove_dir_all(cache.path().join("docs")).unwrap();
    let removed = sync::synchronize(&mut drive, "project", cache.path()).unwrap();
    assert_eq!(removed.deleted, 2);
    assert_eq!(removed.directories_deleted, 3);
    assert!(drive.directories.is_empty());
    assert!(drive.files.is_empty());
}

#[test]
fn obsolete_sync_manifest_is_quarantined_without_losing_documents() {
    let local = tempfile::tempdir().unwrap();
    let source = b"main:\n    purpose: kept\n";
    std::fs::write(local.path().join("main.lyn"), source).unwrap();
    std::fs::create_dir_all(local.path().join(".luvyn")).unwrap();
    std::fs::write(local.path().join(".luvyn/sync.json"), b"not-json").unwrap();
    let mut drive = MemoryDrive::default();
    drive.files.insert(
        "main.lyn".into(),
        (
            RemoteFile {
                id: "remote-main".into(),
                path: "main.lyn".into(),
                version: "1".into(),
            },
            source.to_vec(),
        ),
    );

    let report = sync::synchronize(&mut drive, "project", local.path()).unwrap();
    assert!(report.conflicts.is_empty());
    assert_eq!(report.unchanged, 1);
    assert_eq!(
        std::fs::read(local.path().join("main.lyn")).unwrap(),
        source
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(local.path().join(".luvyn/sync.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["schema_version"], 1);
    assert!(
        std::fs::read_dir(local.path().join(".luvyn"))
            .unwrap()
            .any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("sync.json.obsolete-")
            })
    );
}

#[test]
fn legacy_sync_manifest_is_versioned_and_keeps_its_tracking_data() {
    let local = tempfile::tempdir().unwrap();
    let source = b"main:\n    purpose: legacy\n";
    std::fs::write(local.path().join("main.lyn"), source).unwrap();
    std::fs::create_dir_all(local.path().join(".luvyn")).unwrap();
    std::fs::write(
        local.path().join(".luvyn/sync.json"),
        br#"{"files":{},"directories":{}}"#,
    )
    .unwrap();
    let mut drive = MemoryDrive::default();

    let report = sync::synchronize(&mut drive, "project", local.path()).unwrap();
    assert_eq!(report.uploaded, 1);
    assert_eq!(drive.files["main.lyn"].1, source);
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(local.path().join(".luvyn/sync.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(
        manifest["files"]["main.lyn"]["hash"],
        blake3::hash(source).to_hex().as_str()
    );
}
