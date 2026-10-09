use luvyn_core::{Error, Project, Result, workspace::atomic_write};
use std::{fs, path::PathBuf};
const SKILL: &str = include_str!("../../../skills/luvyn/SKILL.md");

pub fn git(project: &Project) -> Result<()> {
    let path = project.safe_path(".gitignore")?;
    let mut content = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        String::new()
    };
    let mut additions = vec![
        "/.luvyn/cache.bin".into(),
        "/.luvyn/*.lock".into(),
        "/.luvyn/ide-*".into(),
        "/.luvyn/dist/".into(),
        "/.luvyn/*.zip".into(),
    ];
    if project.config.project.artifacts == "ignore" {
        additions.push("*.lu".into());
    } else {
        // Explicit negation works even when an earlier rule ignores the directory.
        additions.extend(["!/.luvyn/".into(), "!/.luvyn/project.lu".into()]);
    }
    if project.root == project.target_project_root
        && project.config.project.artifacts == "ignore"
        && !project.config.output.starts_with(".luvyn/")
    {
        additions.push(format!("/{}", project.config.output));
    }
    if !project.config.export.starts_with(".luvyn/") {
        additions.push(format!("/{}", project.config.export));
    }
    const BEGIN: &str = "# Luvyn generated artifacts";
    const END: &str = "# End Luvyn generated artifacts";
    if let Some(start) = content.find(BEGIN)
        && let Some(relative_end) = content[start..].find(END)
    {
        let end = start + relative_end + END.len();
        let end = if content[end..].starts_with('\n') {
            end + 1
        } else {
            end
        };
        content.replace_range(start..end, "");
    }
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(BEGIN);
    content.push('\n');
    content.push_str(&additions.join("\n"));
    content.push('\n');
    content.push_str(END);
    content.push('\n');
    let original = fs::read_to_string(&path).unwrap_or_default();
    if content != original {
        atomic_write(&path, content.as_bytes())?;
    }
    Ok(())
}
pub fn skill(project: &Project, local: bool, directory: Option<PathBuf>) -> Result<PathBuf> {
    let explicit = directory.is_some();
    let local_root = project.safe_path(".agents/skills")?;
    let global = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .map(|p| p.join("skills"))
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
                .map(|p| p.join(".agents/skills"))
        });
    let root = directory.unwrap_or_else(|| {
        if local {
            local_root.clone()
        } else {
            global.unwrap_or_else(|| local_root.clone())
        }
    });
    let install = |root: PathBuf| -> Result<PathBuf> {
        let target = root.join("luvyn/SKILL.md");
        if target.exists() && fs::read_to_string(&target)? != SKILL {
            return Err(Error::Message(format!(
                "Existing skill differs: {}; use --directory to choose another location",
                target.display()
            )));
        }
        atomic_write(&target, SKILL.as_bytes())?;
        Ok(target)
    };
    match install(root) {
        Ok(p) => Ok(p),
        Err(e) if !explicit && !local => {
            eprintln!("Global install unavailable ({e}); using workspace skill");
            install(local_root)
        }
        Err(e) => Err(e),
    }
}
