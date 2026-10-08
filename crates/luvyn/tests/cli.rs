use std::{fs, process::Command};
fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_luvyn"))
}
#[test]
fn complete_cli_workflow_and_exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("User.lyn"),"entity User\npurpose: identity\nservice UserService\npurpose: manager\ndepends User\nexposes:\n    createUser(email: String) -> User?\n").unwrap();
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
        "service Bad\npurpose: bad\ndepends Missing\n",
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
