//! Android JNI adapter. Language, editor, workspace, query and build stay in Rust Core.
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::jstring,
};
use luvyn_core::{
    Project,
    ide::{self, IdeCore},
};
use std::{
    path::Path,
    sync::{Mutex, OnceLock},
};

static SESSION: OnceLock<Mutex<Option<(String, IdeCore)>>> = OnceLock::new();

pub fn request(root: &str, target: &str, input: &str) -> String {
    let result = std::panic::catch_unwind(|| -> luvyn_core::Result<serde_json::Value> {
        if input.len() > 3 * 1024 * 1024 {
            return Err(luvyn_core::Error::Message("Request exceeds 3 MiB".into()));
        }
        let value: serde_json::Value =
            serde_json::from_str(input).map_err(|e| luvyn_core::Error::Message(e.to_string()))?;
        let op = value["op"].as_str().unwrap_or("");
        let storage = value["_storage"]
            .as_str()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| Path::new(target).join("projects"));
        let registry = luvyn_core::projects::ProjectRegistry::new(storage.clone());
        let state = || -> luvyn_core::Result<serde_json::Value> {
            Ok(
                serde_json::json!({"current":root,"recent":registry.list()?,"cloud":{"connected":value["_token"].as_str().is_some_and(|t|!t.is_empty()),"configured":true,"authorization":""}}),
            )
        };
        match op {
            "project-view" => return Ok(serde_json::json!({"revision":0})),
            "projects" => return state(),
            "project-forget" => {
                registry.forget(value["id"].as_str().unwrap_or(""))?;
                return state();
            }
            "project-remember" => {
                registry.remember(
                    value["name"].as_str().unwrap_or("Project").into(),
                    value["path"].as_str().unwrap_or(root).into(),
                    "local",
                    None,
                )?;
                return state();
            }
            "drive-projects" | "drive-open" | "drive-create" | "project-sync" => {
                use luvyn_core::sync::{SyncProvider, synchronize};
                let token = value["_token"]
                    .as_str()
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| {
                        luvyn_core::Error::Message("Connect Google Drive first".into())
                    })?;
                let mut provider = luvyn_drive::GoogleDriveProvider::new(token.into());
                if op == "drive-projects" {
                    return Ok(serde_json::json!({"projects":provider.projects()?}));
                }
                if op == "project-sync" {
                    let project = registry
                        .list()?
                        .into_iter()
                        .find(|p| p.kind == "cloud" && p.path == root)
                        .and_then(|p| p.cloud_id)
                        .ok_or_else(|| {
                            luvyn_core::Error::Message("Current project is local".into())
                        })?;
                    let guard = SESSION
                        .get_or_init(|| Mutex::new(None))
                        .lock()
                        .map_err(|_| {
                            luvyn_core::Error::Message("Mobile session unavailable".into())
                        })?;
                    let report = if let Some((_, core)) = guard.as_ref() {
                        core.with_saved_workspace(|| {
                            synchronize(&mut provider, &project, Path::new(root))
                        })?
                    } else {
                        synchronize(&mut provider, &project, Path::new(root))?
                    };
                    if let Some((_, core)) = guard.as_ref() {
                        core.external
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        core.revision
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    return Ok(serde_json::json!({"report":report}));
                }
                let project = if op == "drive-create" {
                    provider.create_project(value["name"].as_str().unwrap_or("Project"))?
                } else {
                    let id = value["id"].as_str().unwrap_or("");
                    provider
                        .projects()?
                        .into_iter()
                        .find(|p| p.id == id)
                        .ok_or_else(|| {
                            luvyn_core::Error::Message("Drive project unavailable".into())
                        })?
                };
                let cache = storage.join("cloud").join(blake3_id(&project.id));
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
                    return Ok(serde_json::json!({"conflicts":report.conflicts,"cache":cache}));
                }
                registry.remember(
                    project.name,
                    cache.to_string_lossy().into(),
                    "cloud",
                    Some(project.id),
                )?;
                return Ok(
                    serde_json::json!({"_workspace":cache,"current":cache,"recent":registry.list()?,"cloud":{"connected":true,"configured":true,"authorization":""}}),
                );
            }
            _ => {}
        }
        let mut session = SESSION
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| luvyn_core::Error::Message("Mobile session unavailable".into()))?;
        let key = format!("{root}\n{target}");
        if session.as_ref().is_none_or(|(path, _)| path != &key) || value["op"] == "reload" {
            let mut project = Project::open_with_target(Path::new(root), Some(Path::new(target)))?;
            project.analyze(&Default::default())?;
            *session = Some((key, IdeCore::new(project)));
        }
        let core = &session.as_ref().unwrap().1;
        if value["op"] == "reload" {
            return ide::handle(core, serde_json::json!({"op":"snapshot"}));
        }
        ide::handle(core, value)
    });
    match result {
        Ok(Ok(value)) => value.to_string(),
        Ok(Err(error)) => serde_json::json!({"error": error.to_string()}).to_string(),
        Err(_) => serde_json::json!({"error":"Rust Core failed; reopen workspace"}).to_string(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_luvyn_mobile_CoreBridge_request(
    mut env: JNIEnv,
    _: JClass,
    root: JString,
    target: JString,
    input: JString,
) -> jstring {
    let result = (|| -> jni::errors::Result<_> {
        let root: String = env.get_string(&root)?.into();
        let target: String = env.get_string(&target)?.into();
        let input: String = env.get_string(&input)?.into();
        env.new_string(request(&root, &target, &input))
    })();
    match result {
        Ok(value) => value.into_raw(),
        Err(_) => {
            let _ = env.throw_new(
                "java/lang/IllegalStateException",
                "Invalid JNI string or allocation failure",
            );
            std::ptr::null_mut()
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn mobile_projects_persist_provider_uris_without_desktop_paths() {
        let storage = tempfile::tempdir().unwrap();
        let docs = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let request = |value: serde_json::Value| -> serde_json::Value {
            serde_json::from_str(&super::request(
                docs.path().to_str().unwrap(),
                target.path().to_str().unwrap(),
                &value.to_string(),
            ))
            .unwrap()
        };
        let base = storage.path().to_string_lossy().to_string();
        let remembered = request(
            serde_json::json!({"op":"project-remember","name":"Mobile","path":"content://provider/tree/docs","_storage":base}),
        );
        assert!(remembered.get("error").is_none());
        let state = request(serde_json::json!({"op":"projects","_storage":base}));
        assert_eq!(state["recent"][0]["path"], "content://provider/tree/docs");
        assert_eq!(state["recent"].as_array().unwrap().len(), 1);
        let removed = request(
            serde_json::json!({"op":"project-forget","id":state["recent"][0]["id"],"_storage":base}),
        );
        assert!(removed["recent"].as_array().unwrap().is_empty());
    }
    #[test]
    fn adapter_uses_real_dictionary_and_compiler() {
        let docs = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        std::fs::write(docs.path().join("player.lyn"), "@entity\nclass Player\nfields:\n    health: f32\nrules:\n    - self.health >= 0.0\nfunc damage(amount: f32) -> Result<(), DamageError>\n@error\nclass DamageError\n").unwrap();
        let root = docs.path().to_str().unwrap();
        let destination = target.path().to_str().unwrap();
        let dictionary: serde_json::Value =
            serde_json::from_str(&super::request(root, destination, r#"{"op":"language"}"#))
                .unwrap();
        assert!(
            dictionary["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["keyword"] == "bool")
        );
        let result: serde_json::Value =
            serde_json::from_str(&super::request(root, destination, r#"{"op":"build"}"#)).unwrap();
        assert!(result.get("error").is_none(), "{result}");
        assert!(target.path().join(".luvyn/project.lu").is_file());
        assert!(!docs.path().join(".luvyn/project.lu").exists());
        std::fs::write(
            docs.path().join("luvyn.toml"),
            "output = \".luvyn/custom.lu\"\n",
        )
        .unwrap();
        let result: serde_json::Value =
            serde_json::from_str(&super::request(root, destination, r#"{"op":"build"}"#)).unwrap();
        assert!(result.get("error").is_none(), "{result}");
        assert_eq!(result["output_relative"], ".luvyn/custom.lu");
        assert!(target.path().join(".luvyn/custom.lu").is_file());
    }
}

fn blake3_id(id: &str) -> String {
    blake3::hash(id.as_bytes()).to_hex().to_string()
}
