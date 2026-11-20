//! Builds bugsplat-native with CMake (static SDK + BugSplatMonitor + BugSplatReporter, and on
//! Windows BugSplatWer.dll) and links it.
//!
//! On Windows, BugSplatWer.dll is also copied into the directories Cargo puts executables in
//! (`target/<profile>`, its `examples/` and `deps/`), because the statically linked SDK registers
//! it from next to the executable; see `copy_next_to_executables`.
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
    for dir in ["include", "src", "monitor", "reporter", "wer", "cmake", "CMakeLists.txt", "VERSION"] {
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
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        pin_msvc_tools(&mut config);
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
    // dependents' build scripts that package them (DEP_BUGSPLAT_MONITOR / DEP_BUGSPLAT_REPORTER /
    // DEP_BUGSPLAT_WER).
    if let Some(monitor) = info.get("monitor") {
        println!("cargo:rustc-env=BUGSPLAT_SYS_MONITOR={monitor}");
        println!("cargo:monitor={monitor}");
    }
    if let Some(reporter) = info.get("reporter") {
        println!("cargo:rustc-env=BUGSPLAT_SYS_REPORTER={reporter}");
        println!("cargo:reporter={reporter}");
    }
    // Windows only. The SDK is linked into the executable, so it looks for BugSplatWer.dll next to
    // the executable; there is no development fallback for it, so put it there for `cargo run`.
    if let Some(wer) = info.get("wer") {
        println!("cargo:rustc-env=BUGSPLAT_SYS_WER={wer}");
        println!("cargo:wer={wer}");
        copy_next_to_executables(Path::new(wer));
    }
    println!("cargo:include={}", native.join("include").display());
}

/// Copies `file` to where Cargo writes this build's executables: `<profile>/` (binaries, so
/// `cargo run` and `cargo tauri dev`), `<profile>/examples/` and `<profile>/deps/` (tests).
///
/// Cargo has no supported way to name those directories, so they are derived from OUT_DIR, which
/// is `<target-dir>/[<triple>/]<profile>/build/bugsplat-sys-<hash>/out`. With any other layout, or
/// if a copy fails, this warns and the build goes on: the app still runs, without WER coverage.
fn copy_next_to_executables(file: &Path) {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set for build scripts"));
    let mut ancestors = out_dir.ancestors().skip(2);
    let (Some(build), Some(profile_dir)) = (ancestors.next(), ancestors.next()) else { return };
    if build.file_name() != Some("build".as_ref()) {
        println!(
            "cargo:warning=unexpected OUT_DIR layout ({}); copy {} next to your executable yourself",
            out_dir.display(),
            file.display()
        );
        return;
    }
    let name = file.file_name().expect("helper path has a file name");
    for dir in [profile_dir.to_path_buf(), profile_dir.join("examples"), profile_dir.join("deps")] {
        let dest = dir.join(name);
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::copy(file, &dest)) {
            println!("cargo:warning=could not copy {} to {}: {e}", file.display(), dest.display());
        }
    }
}

/// Points CMake at the `link.exe` and `lib.exe` next to the `cl.exe` the cmake crate passes.
/// CMake keeps the linker it found in its cache, so a build directory configured with another
/// MSVC toolset (before a Visual Studio update, or a CI cache restored on a newer runner image)
/// would otherwise pair this compiler with a linker that may no longer exist.
fn pin_msvc_tools(config: &mut cmake::Config) {
    let Ok(compiler) = cc::Build::new().cargo_metadata(false).try_get_compiler() else {
        return;
    };
    let Some(bin) = compiler.path().parent() else {
        return;
    };
    for (var, exe) in [("CMAKE_LINKER", "link.exe"), ("CMAKE_AR", "lib.exe")] {
        let tool = bin.join(exe);
        if tool.exists() {
            config.define(var, tool.to_string_lossy().replace('\\', "/"));
        }
    }
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
