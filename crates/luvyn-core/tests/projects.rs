use luvyn_core::{
    Result,
    projects::ProjectRegistry,
    sync::{RemoteDirectory, RemoteFile, RemoteProject, SyncProvider, synchronize},
};
use std::{collections::BTreeMap, fs};
#[test]
fn recent_projects_persist_deduplicate_and_forget_without_deleting() {
    let store = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let registry = ProjectRegistry::new(store.path().into());
    let path = project.path().to_string_lossy().to_string();
    let first = registry
        .remember("First".into(), path.clone(), "local", None)
        .unwrap();
    registry
        .remember("Renamed".into(), path.clone(), "local", None)
        .unwrap();
    let reopened = ProjectRegistry::new(store.path().into());
    let rows = reopened.list().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, first.id);
    assert_eq!(rows[0].name, "Renamed");
    fs::remove_dir(project.path()).unwrap();
    assert!(reopened.list().unwrap()[0].unavailable);
    reopened.forget(&first.id).unwrap();
    assert!(reopened.list().unwrap().is_empty());
    let live = tempfile::tempdir().unwrap();
    let entry = reopened
        .remember(
            "Keep".into(),
            live.path().to_string_lossy().into(),
            "local",
            None,
        )
        .unwrap();
    reopened.forget(&entry.id).unwrap();
    assert!(live.path().is_dir());
}

#[test]
fn legacy_recent_project_array_migrates_and_keeps_project_locations() {
    let store = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    fs::write(project.path().join("main.lyn"), "main:\n").unwrap();
    let path = project.path().to_string_lossy();
    let legacy = serde_json::json!([{
        "id": "legacy-local-id",
        "name": "Legacy",
        "path": path,
        "kind": "local",
        "last_access": 1,
        "cloud_id": null,
        "unavailable": false
    }]);
    fs::write(
        store.path().join("projects.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();

    let rows = ProjectRegistry::new(store.path().into()).list().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "legacy-local-id");
    assert!(project.path().join("main.lyn").is_file());
    let migrated: serde_json::Value =
        serde_json::from_slice(&fs::read(store.path().join("projects.json")).unwrap()).unwrap();
    assert_eq!(migrated["schema_version"], 1);
    assert_eq!(migrated["projects"][0]["id"], "legacy-local-id");
}
#[derive(Default)]
struct Provider {
    files: BTreeMap<String, (RemoteFile, Vec<u8>)>,
    writes: usize,
}
impl Provider {
    fn put(&mut self, path: &str, data: &[u8]) {
        let version = self
            .files
            .get(path)
            .map_or(1, |(f, _)| f.version.parse::<u64>().unwrap() + 1);
        self.files.insert(
            path.into(),
            (
                RemoteFile {
                    id: format!("id:{path}"),
                    path: path.into(),
                    version: version.to_string(),
                },
                data.into(),
            ),
        );
    }
}
impl SyncProvider for Provider {
    fn projects(&mut self) -> Result<Vec<RemoteProject>> {
        Ok(vec![])
    }
    fn create_project(&mut self, name: &str) -> Result<RemoteProject> {
        Ok(RemoteProject {
            id: "p".into(),
            name: name.into(),
        })
    }
    fn files(&mut self, _: &str) -> Result<Vec<RemoteFile>> {
        Ok(self.files.values().map(|(f, _)| f.clone()).collect())
    }
    fn directories(&mut self, _: &str) -> Result<Vec<RemoteDirectory>> {
        Ok(vec![])
    }
    fn create_directory(&mut self, _: &str, path: &str) -> Result<RemoteDirectory> {
        Ok(RemoteDirectory {
            id: path.into(),
            path: path.into(),
            version: "1".into(),
        })
    }
    fn delete_directory(&mut self, _: &RemoteDirectory) -> Result<()> {
        Ok(())
    }
    fn download(&mut self, file: &RemoteFile) -> Result<Vec<u8>> {
        Ok(self.files[&file.path].1.clone())
    }
    fn upload(
        &mut self,
        _: &str,
        path: &str,
        data: &[u8],
        expected: Option<&RemoteFile>,
    ) -> Result<RemoteFile> {
        assert_eq!(
            expected.map(|f| &f.version),
            self.files.get(path).map(|(f, _)| &f.version)
        );
        self.writes += 1;
        self.put(path, data);
        Ok(self.files[path].0.clone())
    }
    fn delete(&mut self, file: &RemoteFile) -> Result<()> {
        assert_eq!(self.files[&file.path].0.version, file.version);
        self.files.remove(&file.path);
        self.writes += 1;
        Ok(())
    }
}
#[test]
fn sync_three_way_hashes_conflicts_deletions_and_artifact_exclusion() {
    let local = tempfile::tempdir().unwrap();
    let mut provider = Provider::default();
    provider.put("main.lyn", b"main:\n    purpose: remote\n");
    provider.put(".ignore.luvyn", b"legacy/**\n");
    provider.put("project.lu", b"never download");
    let first = synchronize(&mut provider, "p", local.path()).unwrap();
    assert_eq!(first.downloaded, 2);
    assert!(!local.path().join("project.lu").exists());
    assert_eq!(
        synchronize(&mut provider, "p", local.path())
            .unwrap()
            .unchanged,
        2
    );
    assert_eq!(provider.writes, 0);
    fs::write(
        local.path().join("main.lyn"),
        b"main:\n    purpose: local\n",
    )
    .unwrap();
    let uploaded = synchronize(&mut provider, "p", local.path()).unwrap();
    assert_eq!(uploaded.uploaded, 1);
    let persisted = synchronize(&mut provider, "p", local.path()).unwrap();
    assert_eq!(persisted.unchanged, 2);
    assert_eq!(provider.writes, 1);
    provider.put("main.lyn", b"main:\n    purpose: new remote\n");
    fs::write(
        local.path().join("main.lyn"),
        b"main:\n    purpose: changed local\n",
    )
    .unwrap();
    let conflict = synchronize(&mut provider, "p", local.path()).unwrap();
    assert_eq!(conflict.conflicts, vec!["main.lyn"]);
    assert_eq!(provider.writes, 1);
    assert!(
        fs::read_to_string(local.path().join("main.lyn"))
            .unwrap()
            .contains("changed local")
    );
    fs::write(
        local.path().join("main.lyn"),
        b"main:\n    purpose: new remote\n",
    )
    .unwrap();
    assert!(
        synchronize(&mut provider, "p", local.path())
            .unwrap()
            .conflicts
            .is_empty()
    );
    provider.files.remove(".ignore.luvyn");
    assert_eq!(
        synchronize(&mut provider, "p", local.path())
            .unwrap()
            .deleted,
        1
    );
    assert!(!local.path().join(".ignore.luvyn").exists());
    fs::remove_file(local.path().join("main.lyn")).unwrap();
    assert_eq!(
        synchronize(&mut provider, "p", local.path())
            .unwrap()
            .deleted,
        1
    );
    assert!(!provider.files.contains_key("main.lyn"));
}
#[test]
fn corrupt_registry_is_quarantined_without_touching_projects() {
    let local = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    fs::write(source.path().join("main.lyn"), "main:\n").unwrap();
    fs::write(local.path().join("projects.json"), b"invalid").unwrap();
    let registry = ProjectRegistry::new(local.path().into());
    assert!(registry.list().unwrap().is_empty());
    let stored: serde_json::Value =
        serde_json::from_slice(&fs::read(local.path().join("projects.json")).unwrap()).unwrap();
    assert_eq!(stored["schema_version"], 1);
    assert!(stored["projects"].as_array().unwrap().is_empty());
    assert!(fs::read_dir(local.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("projects.json.obsolete-")
    }));
    assert!(source.path().join("main.lyn").is_file());
}

#[test]
fn remote_traversal_is_rejected_without_mutation() {
    let local = tempfile::tempdir().unwrap();
    let mut provider = Provider::default();
    provider.put("../escape.lyn", b"class Escape");
    assert!(synchronize(&mut provider, "p", local.path()).is_err());
    assert!(!local.path().parent().unwrap().join("escape.lyn").exists());
}
