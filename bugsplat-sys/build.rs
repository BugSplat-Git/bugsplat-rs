//! Builds bugsplat-native with CMake (static SDK + BugSplatMonitor + BugSplatReporter) and links it.
//!
//! Environment:
//! - `BUGSPLAT_NATIVE_SRC`: use this bugsplat-native checkout instead of the bundled submodule.
//! - `BUGSPLAT_CRASHPAD_CACHE`: where bugsplat-native caches the pinned Crashpad prebuilt
//!   (read by its CMake; set it to share one download across target directories).
//! - `DOCS_RS`: set by docs.rs, which builds without network access; skips the native build,
//!   since documentation needs no library to link.

use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    println!("cargo:rerun-if-env-changed=BUGSPLAT_NATIVE_SRC");
    println!("cargo:rerun-if-env-changed=BUGSPLAT_CRASHPAD_CACHE");
    println!("cargo:rerun-if-changed=cmake/CMakeLists.txt");
    println!("cargo:rerun-if-env-changed=DOCS_RS");
    if env::var_os("DOCS_RS").is_some() {
        return;
    }

    let native = env::var_os("BUGSPLAT_NATIVE_SRC")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir.join("bugsplat-native"));
    if !native.join("CMakeLists.txt").exists() {
        panic!(
            "bugsplat-native not found at {}.\n\
             Run `git submodule update --init` or set BUGSPLAT_NATIVE_SRC.",
            native.display()
        );
    }
    for dir in ["include", "src", "monitor", "reporter", "cmake", "CMakeLists.txt", "VERSION"] {
        println!("cargo:rerun-if-changed={}", native.join(dir).display());
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let mut config = cmake::Config::new(manifest_dir.join("cmake"));
    config
        .define("BUGSPLAT_NATIVE_DIR", &native)
        // Crashpad prebuilts are release-only (and /MD on Windows), whatever Cargo's profile.
        .profile("Release")
        .build_target("bugsplat_rs");
    // bugsplat-native requires Ninja on Windows (the Visual Studio generator can pick a toolset
    // older than the prebuilt's); use it everywhere it is available.
    if which("ninja").is_some() {
        config.generator("Ninja");
    } else if target_os == "windows" {
        panic!("bugsplat-sys needs Ninja on Windows (e.g. `winget install Ninja-build.Ninja`).");
    }
    if target_os == "macos" {
        // The Crashpad prebuilt targets macOS 11.5.
        let floor = env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| "11.5".into());
        config.define("CMAKE_OSX_DEPLOYMENT_TARGET", floor);
    }
    let dst = config.build();
    let build_dir = dst.join("build");

    let info = read_link_info(&build_dir.join("bugsplat-link.txt"));
    let archive = PathBuf::from(&info["archive"]);
    println!("cargo:rustc-link-search=native={}", archive.parent().unwrap().display());
    println!("cargo:rustc-link-lib=static=bugsplat_rs");

    for item in info.get("system").map(String::as_str).unwrap_or("").split('|') {
        emit_system_lib(item.trim());
    }
    match target_os.as_str() {
        "macos" | "ios" => println!("cargo:rustc-link-lib=dylib=c++"),
        "linux" | "android" => println!("cargo:rustc-link-lib=dylib=stdc++"),
        _ => {}
    }

    // Helper locations: for the safe crate's development fallback (rustc-env) and for
    // dependents' build scripts that package them (DEP_BUGSPLAT_MONITOR / DEP_BUGSPLAT_REPORTER).
    if let Some(monitor) = info.get("monitor") {
        println!("cargo:rustc-env=BUGSPLAT_SYS_MONITOR={monitor}");
        println!("cargo:monitor={monitor}");
    }
    if let Some(reporter) = info.get("reporter") {
        println!("cargo:rustc-env=BUGSPLAT_SYS_REPORTER={reporter}");
        println!("cargo:reporter={reporter}");
    }
    println!("cargo:include={}", native.join("include").display());
}

fn read_link_info(path: &Path) -> HashMap<String, String> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} not written by CMake: {e}", path.display()));
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn emit_system_lib(item: &str) {
    if item.is_empty() {
        return;
    }
    if let Some(framework) = item.strip_prefix("-framework ") {
        println!("cargo:rustc-link-lib=framework={}", framework.trim());
    } else if item.starts_with("-Wl,") {
        // Linker-group flags (Linux); the merged archive makes them unnecessary.
    } else if let Some(name) = item.strip_prefix("-l") {
        println!("cargo:rustc-link-lib=dylib={name}");
    } else {
        let name = item.trim_end_matches(".lib");
        println!("cargo:rustc-link-lib=dylib={name}");
    }
}

fn which(program: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{program}.exe") } else { program.to_string() };
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(&exe))
        .find(|p| p.is_file())
}
