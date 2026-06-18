#[cfg(all(windows, target_env = "msvc"))]
fn windows_sdk_arch() -> Option<&'static str> {
    match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => Some("x64"),
        Ok("aarch64") => Some("arm64"),
        Ok("x86") => Some("x86"),
        _ => None,
    }
}

#[cfg(all(windows, target_env = "msvc"))]
fn add_windows_sdk_link_paths() {
    use std::path::{Path, PathBuf};

    println!("cargo:rerun-if-env-changed=ProgramFiles(x86)");
    println!("cargo:rerun-if-env-changed=WindowsSdkDir");
    println!("cargo:rerun-if-env-changed=WindowsSDKVersion");

    let Some(arch) = windows_sdk_arch() else {
        return;
    };

    let sdk_root = std::env::var_os("WindowsSdkDir")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("ProgramFiles(x86)")
                .map(PathBuf::from)
                .map(|path| path.join("Windows Kits").join("10"))
        });

    let Some(sdk_root) = sdk_root else {
        return;
    };

    let lib_root = sdk_root.join("Lib");
    let sdk_version = std::env::var("WindowsSDKVersion")
        .ok()
        .map(|version| version.trim_matches('\\').to_string())
        .filter(|version| !version.is_empty());

    let version_dir = sdk_version
        .map(|version| lib_root.join(version))
        .filter(|path| path.is_dir())
        .or_else(|| newest_sdk_lib_dir(&lib_root));

    let Some(version_dir) = version_dir else {
        return;
    };

    for segment in ["um", "ucrt"] {
        let lib_path = version_dir.join(segment).join(arch);
        if lib_path.is_dir() {
            println!("cargo:rustc-link-search=native={}", lib_path.display());
        }
    }

    fn newest_sdk_lib_dir(lib_root: &Path) -> Option<PathBuf> {
        std::fs::read_dir(lib_root)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("10."))
            })
            .max_by(|left, right| left.file_name().cmp(&right.file_name()))
    }
}

fn main() {
    #[cfg(all(windows, target_env = "msvc"))]
    add_windows_sdk_link_paths();

    tauri_build::build();
}
