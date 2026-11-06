use std::path::{Path, PathBuf};

fn main() {
    stage_bugsplat_helpers();
    tauri_build::build()
}

/// Copies BugSplatMonitor and BugSplatReporter, which bugsplat-sys builds and
/// tauri-plugin-bugsplat passes on as DEP_TAURI_PLUGIN_BUGSPLAT_*, to bugsplat-helpers/. From
/// there tauri.conf.json's bundle.macOS.files puts them in my-tauri-crasher.app/Contents/Helpers.
/// `cargo tauri dev` needs none of this: with no packaged helpers, bugsplat uses the ones built
/// for this compilation.
fn stage_bugsplat_helpers() {
    let dest = PathBuf::from("bugsplat-helpers");
    for key in ["MONITOR", "REPORTER"] {
        let var = format!("DEP_TAURI_PLUGIN_BUGSPLAT_{key}");
        println!("cargo:rerun-if-env-changed={var}");
        let Ok(src) = std::env::var(&var) else {
            println!("cargo:warning={var} not set; BugSplat helpers will not be bundled");
            continue;
        };
        let src = PathBuf::from(src);
        let target = dest.join(src.file_name().expect("helper path has a file name"));
        let _ = std::fs::remove_dir_all(&target);
        let _ = std::fs::remove_file(&target);
        copy_recursive(&src, &target).unwrap_or_else(|e| panic!("staging {}: {e}", src.display()));
    }
}

fn copy_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        #[cfg(unix)]
        std::os::unix::fs::symlink(std::fs::read_link(src)?, dest)?;
        return Ok(());
    }
    if meta.is_dir() {
        std::fs::create_dir_all(dest)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dest.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, dest).map(|_| ())
}
