use luvyn_core::{Error, Project, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, process::Command};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, clap::ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Server,
    Desktop,
    Android,
}
impl Target {
    pub fn name(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Desktop => "desktop",
            Self::Android => "android",
        }
    }
    pub fn available(self) -> Result<()> {
        match self {
        Self::Server => Ok(()),
        Self::Desktop if cfg!(feature="desktop") => Ok(()),
        Self::Desktop => Err(Error::Message("Desktop host is not included in this executable. Run luvyn ide build --desktop, then use its executable.".into())),
        Self::Android => Err(Error::Message("Build Android with luvyn ide build --android; install dist/android/luvyn-debug.apk and select Documentation/Target in the app.".into())),
    }
    }
}
pub fn remember_workspace(project: &Project) -> Result<()> {
    if !luvyn_core::projects::is_launcher(&project.root) {
        luvyn_core::projects::ProjectRegistry::new(luvyn_core::projects::data_directory()?)
            .remember(
                project
                    .root
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                project.root.to_string_lossy().into(),
                "local",
                None,
            )?;
    }
    Ok(())
}
pub fn build(target: Target, root: &Path, output: Option<&Path>) -> Result<()> {
    if target == Target::Android {
        return super::android::build(root, output);
    }
    let root = root.canonicalize()?;
    if !root.join("crates/luvyn/Cargo.toml").is_file() || !root.join("ui/package.json").is_file() {
        return Err(Error::Message(
            "IDE distribution build requires the Luvyn source repository; pass --source <path>"
                .into(),
        ));
    }
    fn step(command: &mut Command) -> Result<()> {
        let status = command.status()?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Message(format!(
                "Distribution build failed: {status}"
            )))
        }
    }
    #[cfg(windows)]
    let npm = "npm.cmd";
    #[cfg(not(windows))]
    let npm = "npm";
    step(Command::new(npm).args(["ci"]).current_dir(root.join("ui")))?;
    step(
        Command::new(npm)
            .args(["run", "build"])
            .current_dir(root.join("ui")),
    )?;
    let mut cargo = Command::new("cargo");
    cargo
        .args(["build", "--release", "--locked", "-p", "luvyn"])
        .current_dir(&root);
    if target == Target::Desktop {
        cargo.args(["--features", "desktop"]);
    }
    step(&mut cargo)?;
    let destination = output
        .map(Path::to_owned)
        .unwrap_or_else(|| root.join(".luvyn/dist").join(target.name()));
    fs::create_dir_all(&destination)?;
    let exe = if cfg!(windows) { "luvyn.exe" } else { "luvyn" };
    fs::copy(root.join("target/release").join(exe), destination.join(exe))?;
    fs::copy(root.join("LICENSE"), destination.join("LICENSE"))?;
    fs::copy(
        root.join("ui/node_modules/elkjs/LICENSE.md"),
        destination.join("ELK-LICENSE.md"),
    )?;
    fs::write(destination.join("manifest.json"),serde_json::to_vec_pretty(&serde_json::json!({"target":target.name(),"version":env!("CARGO_PKG_VERSION"),"executable":exe,"platform":std::env::consts::OS,"assets":"embedded","artifact_version":luvyn_core::binary::FORMAT_VERSION})).map_err(|e|Error::Message(e.to_string()))?)?;
    println!(
        "Built {} distribution: {}",
        target.name(),
        destination.display()
    );
    Ok(())
}
