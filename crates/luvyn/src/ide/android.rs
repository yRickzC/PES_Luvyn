//! Build a real Android distribution without coupling Android dependencies to Core.
use luvyn_core::{Error, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn step(command: &mut Command, requirement: &str) -> Result<()> {
    let status = command
        .status()
        .map_err(|e| Error::Message(format!("Android requires {requirement}: {e}")))?;
    if !status.success() {
        return Err(Error::Message(format!(
            "Android build failed ({requirement}): {status}; inspect the build output above"
        )));
    }
    Ok(())
}
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), to.join(entry.file_name()))?;
        }
    }
    Ok(())
}
pub fn build(root: &Path, output: Option<&Path>) -> Result<()> {
    let root = root.canonicalize()?;
    if !root.join("apps/ide-mobile/app/build.gradle").is_file() {
        return Err(Error::Message(
            "Android build requires the Luvyn source repository; use --source <path>".into(),
        ));
    }
    let sdk = std::env::var_os("ANDROID_HOME").or_else(|| std::env::var_os("ANDROID_SDK_ROOT")).map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Android/Sdk")))
        .filter(|p| p.is_dir()).ok_or_else(|| Error::Message("Android SDK missing. Install SDK platform 36/build-tools 36.0.0 and set ANDROID_HOME".into()))?;
    if !sdk.join("platforms/android-36/android.jar").is_file() {
        return Err(Error::Message("Android SDK platform 36 missing. Run sdkmanager \"platforms;android-36\" \"build-tools;36.0.0\"".into()));
    }
    let ndk = std::env::var_os("ANDROID_NDK_HOME").map(PathBuf::from).or_else(|| {
        let mut ndks: Vec<_> = fs::read_dir(sdk.join("ndk")).ok()?.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect();
        ndks.sort(); ndks.pop()
    }).ok_or_else(|| Error::Message("Android NDK missing. Run sdkmanager \"ndk;28.2.13676358\"; set ANDROID_NDK_HOME if installed elsewhere".into()))?;
    let abi = std::env::var("LUVYN_ANDROID_ABI").unwrap_or_else(|_| "arm64-v8a".into());
    let (triple, clang_name) = match abi.as_str() {
        "arm64-v8a" => ("aarch64-linux-android", "aarch64-linux-android26-clang"),
        "x86_64" => ("x86_64-linux-android", "x86_64-linux-android26-clang"),
        _ => {
            return Err(Error::Message(
                "LUVYN_ANDROID_ABI must be arm64-v8a or x86_64".into(),
            ));
        }
    };
    let targets = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map_err(|e| Error::Message(format!("Rustup missing: {e}")))?;
    if !String::from_utf8_lossy(&targets.stdout)
        .lines()
        .any(|t| t == triple)
    {
        return Err(Error::Message(format!(
            "Rust Android target missing. Run rustup target add {triple}"
        )));
    }
    let host = if cfg!(windows) {
        "windows-x86_64"
    } else if cfg!(target_os = "macos") {
        "darwin-x86_64"
    } else {
        "linux-x86_64"
    };
    let bin = ndk.join("toolchains/llvm/prebuilt").join(host).join("bin");
    let clang = bin.join(format!(
        "{clang_name}{}",
        if cfg!(windows) { ".cmd" } else { "" }
    ));
    if !clang.is_file() {
        return Err(Error::Message(format!(
            "NDK compiler missing: {}",
            clang.display()
        )));
    }
    let target_key = triple.replace('-', "_");
    let mut cargo = Command::new("cargo");
    cargo
        .args([
            "build",
            "--release",
            "--locked",
            "-p",
            "luvyn-mobile",
            "--target",
            triple,
        ])
        .current_dir(&root)
        .env(
            format!("CARGO_TARGET_{}_LINKER", target_key.to_uppercase()),
            &clang,
        )
        .env(format!("CC_{target_key}"), &clang)
        .env(
            format!("AR_{target_key}"),
            bin.join(if cfg!(windows) {
                "llvm-ar.exe"
            } else {
                "llvm-ar"
            }),
        )
        .env(
            "RUSTFLAGS",
            format!(
                "{} -C link-arg=-Wl,-z,max-page-size=16384",
                std::env::var("RUSTFLAGS").unwrap_or_default()
            ),
        );
    step(&mut cargo, "Rust Core + Android NDK")?;
    let app = root.join("apps/ide-mobile/app/src/main");
    fs::create_dir_all(app.join("jniLibs").join(&abi))?;
    fs::copy(
        root.join("target")
            .join(triple)
            .join("release/libluvyn_mobile.so"),
        app.join("jniLibs").join(&abi).join("libluvyn_mobile.so"),
    )?;
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    step(
        Command::new(npm).arg("ci").current_dir(root.join("ui")),
        "Node.js/npm dependencies",
    )?;
    step(
        Command::new(npm)
            .args(["run", "build"])
            .current_dir(root.join("ui")),
        "shared frontend",
    )?;
    copy_tree(&root.join("ui/dist"), &app.join("assets"))?;
    let gradle = root.join("apps/ide-mobile").join(if cfg!(windows) {
        "gradlew.bat"
    } else {
        "gradlew"
    });
    let mut command = if cfg!(windows) {
        Command::new(&gradle)
    } else {
        let mut c = Command::new("sh");
        c.arg(&gradle);
        c
    };
    step(
        command
            .args(["assembleDebug", "--no-daemon"])
            .env("ANDROID_HOME", &sdk)
            .current_dir(root.join("apps/ide-mobile")),
        "JDK 17+, Gradle wrapper, Android SDK (first build needs network)",
    )?;
    let destination = output
        .map(Path::to_owned)
        .unwrap_or_else(|| root.join("dist/android"));
    fs::create_dir_all(&destination)?;
    fs::copy(
        root.join("apps/ide-mobile/app/build/outputs/apk/debug/app-debug.apk"),
        destination.join("luvyn-debug.apk"),
    )?;
    println!(
        "Android debug APK: {} (ABI {abi})",
        destination.join("luvyn-debug.apk").display()
    );
    Ok(())
}
