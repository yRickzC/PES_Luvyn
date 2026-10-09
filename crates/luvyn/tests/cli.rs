use std::{fs, process::Command};
fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_luvyn"));
    command.env(
        "LUVYN_DATA_DIR",
        std::env::temp_dir().join(format!("luvyn-cli-tests-{}", std::process::id())),
    );
    command
}
#[test]
fn server_lifecycle_owns_its_process_without_pid_registry() {
    let dir = tempfile::tempdir().unwrap();
    let storage = tempfile::tempdir().unwrap();
    let log = dir.path().join("server.log");
    let mut child = command()
        .arg("ide")
        .arg(dir.path())
        .args(["--server", "--no-open"])
        .env("LUVYN_DATA_DIR", storage.path())
        .stdout(std::process::Stdio::null())
        .stderr(fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let url = loop {
            let text = fs::read_to_string(&log).unwrap();
            if let Some(url) = text
                .lines()
                .find_map(|line| line.strip_prefix("Luvyn IDE: "))
            {
                break url.trim_end_matches('/').to_owned();
            }
            assert!(child.try_wait().unwrap().is_none());
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(30));
        };
        assert!(!dir.path().join(".luvyn/ide-server.json").exists());
        assert!(child.try_wait().unwrap().is_none());
        let mut response = ureq::post(format!("{url}/api/action"))
            .send_json(serde_json::json!({"op":"projects"}))
            .unwrap();
        let state: serde_json::Value = response.body_mut().read_json().unwrap();
        assert_eq!(state["host"], "server");
        ureq::post(format!("{url}/api/action"))
            .send_json(serde_json::json!({"op":"shutdown"}))
            .unwrap();
        while child.try_wait().unwrap().is_none() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    }));
    if child.try_wait().unwrap().is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result.unwrap();
}
#[test]
fn complete_cli_workflow_and_exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("User.lyn"),"class User\npurpose: identity\nclass UserService\npurpose: manager\ndepends User\nexport:\n    func createUser(email: String) -> User?\n").unwrap();
    for sub in ["check", "build", "export", "git", "git"] {
        let result = command().arg(sub).arg(dir.path()).output().unwrap();
        assert!(
            result.status.success(),
            "{sub}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let result = command()
        .args(["get", "UserService", "--format", "json", "--workspace"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(result.status.success());
    let data: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(
        data["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "createUser")
    );
    assert!(result.stderr.is_empty());
    let ignore = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert_eq!(ignore.lines().filter(|l| *l == "*.lu").count(), 1);
    assert!(!ignore.contains("*.lyn"));
    let result = command()
        .args(["skill", "--local"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(dir.path().join(".agents/skills/luvyn/SKILL.md").exists());
    fs::write(
        dir.path().join("Bad.lyn"),
        "class Bad\npurpose: bad\ndepends Missing\n",
    )
    .unwrap();
    let result = command()
        .arg("check")
        .arg(dir.path())
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(serde_json::from_slice::<serde_json::Value>(&result.stdout).is_ok());
    let result = command()
        .args(["get", "nothing-exists", "--workspace"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    fs::write(dir.path().join(".luvyn/project.lu"), b"corrupted").unwrap();
    let result = command()
        .args(["get", "User", "--workspace"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Invalid .lu artifact"));
}

#[test]
fn init_target_build_query_and_git_policy_use_correct_repository() {
    let base = tempfile::tempdir().unwrap();
    let docs = base.path().join("documentation");
    let target = base.path().join("source");
    fs::create_dir(&docs).unwrap();
    fs::create_dir(&target).unwrap();
    let init = command()
        .args(["init", "--target", "../source"])
        .arg(&docs)
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    assert!(!command().arg("init").arg(&docs).status().unwrap().success());
    fs::write(docs.join("player.lyn"), "class Player\npurpose: player\nfields:\n    health: f32\nrules:\n    - self.health >= 0.0\nfunc damage(amount: f32) -> Result<(), DamageError>\nsource: src/player.rs::damage\nclass DamageError\n").unwrap();
    for operation in ["check", "build", "git"] {
        let result = command().arg(operation).arg(&docs).output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert!(target.join(".luvyn/project.lu").is_file());
    assert!(!docs.join(".luvyn/project.lu").exists());
    assert!(!target.join("player.lyn").exists());
    assert!(!target.join(".gitignore").exists());
    let result = command()
        .args(["get", "damage", "--workspace"])
        .arg(&target)
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("src/player.rs::damage"));
    for policy in ["ignore", "track", "track", "ignore"] {
        assert!(
            command()
                .arg("git")
                .arg(&target)
                .args(["--artifacts", policy])
                .output()
                .unwrap()
                .status
                .success()
        );
        let ignore = fs::read_to_string(target.join(".gitignore")).unwrap();
        assert_eq!(ignore.matches("# Luvyn generated artifacts").count(), 1);
        assert_eq!(ignore.contains("!/.luvyn/project.lu"), policy == "track");
        assert!(!ignore.contains("*.lyn"));
    }
}
