# bugsplat-rs

Crash reporting for Rust applications with [BugSplat](https://www.bugsplat.com), built on
[bugsplat-native](https://github.com/BugSplat-Git/bugsplat-native) (Crashpad, out of process).

| Crate | What it is |
|---|---|
| [`bugsplat`](bugsplat) | Safe API: `init`, attributes, attachments, non-fatal captures, Rust panic reports, and (feature `js`) JavaScript errors from embedded webviews |
| [`tauri-plugin-bugsplat`](tauri-plugin-bugsplat) | Tauri plugin: catches uncaught JS errors and unhandled rejections in every webview |
| [`bugsplat-sys`](bugsplat-sys) | Raw FFI; builds bugsplat-native from source with CMake and links it statically |

## What gets reported

| Failure | How | Report |
|---|---|---|
| Segfault, illegal instruction, `abort()`, stack overflow | Crashpad in `BugSplatMonitor` (out of process) | Minidump, crash type `Crashpad` (5) |
| Rust panic | Panic hook → `bugsplat_capture_report` (live minidump at the panic) | Minidump with `rust.panic.message`, `rust.panic.location`, `rust.panic.thread` attributes |
| Panic that then aborts (`panic = "abort"`, unwinding into `extern "C"`) | Same; the abort's dump is tagged `panic-duplicate` and discarded | One report, not two |
| JavaScript error in a webview | `tauri-plugin-bugsplat` → Rust → `post/js` (bugsplat-js's fields) | JavaScript report with the same `session` attribute as native reports from that run |

## Quick start

```toml
[dependencies]
bugsplat = "0.1"
```

```rust
fn main() {
    // First thing in main, so crashes during startup are reported. Keep the guard alive.
    let _bugsplat = bugsplat::init(
        bugsplat::Options::new("my-database", "MyApp", env!("CARGO_PKG_VERSION")),
    )
    .expect("BugSplat failed to start");

    bugsplat::set_user("fred@example.com").ok();
    bugsplat::set_attribute("build", Some("nightly")).ok();
    // ...
}
```

Requirements: CMake ≥ 3.22 and Ninja (required on Windows), a C++20 compiler (Xcode 15+, MSVC
toolset 14.44+, clang 15+/gcc 11+). The pinned Crashpad prebuilt is downloaded and verified by
bugsplat-native's CMake; set `BUGSPLAT_CRASHPAD_CACHE` to share it across target directories.

### Symbols

Ship release builds with line tables, then upload with
[symbol-upload](https://github.com/BugSplat-Git/symbol-upload)'s `-m` flag, which converts the
debug info to Breakpad `.sym` with `dump_syms` (Rust names come out demangled):

```toml
[profile.release]
debug = "line-tables-only"
split-debuginfo = "packed"   # macOS: writes myapp.dSYM
```

| Platform | Debug info to upload | Note |
|---|---|---|
| macOS | `target/release/myapp.dSYM` | `split-debuginfo = "packed"` |
| Linux | `myapp.debug` from `objcopy --only-keep-debug` | Build with `CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO=off` (`dump_syms` does not read `.dwp`, so split DWARF loses your crate's file/line info), then `objcopy --only-keep-debug myapp myapp.debug && objcopy --strip-debug --add-gnu-debuglink=myapp.debug myapp`. Upload `myapp.debug`, ship the stripped `myapp`. |
| Windows | `target/release/myapp.pdb` | Always written by MSVC |

```sh
npx @bugsplat/symbol-upload -b my-database -a MyApp -v 1.2.3 -d target/release -f "myapp.dSYM" -m
```

`application` and `version` must match the values passed to `Options::new`.

### Packaging the helpers

`BugSplatMonitor` (captures crashes) and `BugSplatReporter` (dialog and upload) are separate
executables that ship with your app. `bugsplat-sys` builds them; their paths reach your build
script as `DEP_BUGSPLAT_MONITOR` / `DEP_BUGSPLAT_REPORTER` (or `DEP_TAURI_PLUGIN_BUGSPLAT_*`).
bugsplat-native looks next to the executable, in `Helpers/`, and on macOS in
`<App>.app/Contents/Helpers`. During development (`cargo run`) nothing needs packaging: when no
packaged helpers are found, `bugsplat` uses the ones built for this compilation.

## Tauri

```toml
[dependencies]
bugsplat = "0.1"
tauri-plugin-bugsplat = "0.1"
```

```rust
fn main() {
    let _bugsplat = bugsplat::init(bugsplat::Options::new("my-db", "MyApp", env!("CARGO_PKG_VERSION")))
        .expect("BugSplat failed to start");
    tauri::Builder::default()
        .plugin(tauri_plugin_bugsplat::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

Add `"bugsplat:default"` to your capability. To bundle the helpers on macOS, have `build.rs` copy
the paths in `DEP_TAURI_PLUGIN_BUGSPLAT_MONITOR` and `DEP_TAURI_PLUGIN_BUGSPLAT_REPORTER` into a
`bugsplat-helpers/` directory before calling `tauri_build::build()`, and map them in
`tauri.conf.json`:

```json
"bundle": { "macOS": { "files": {
  "Helpers/BugSplatMonitor": "bugsplat-helpers/BugSplatMonitor",
  "Helpers/BugSplatReporter.app": "bugsplat-helpers/BugSplatReporter.app"
} } }
```

JavaScript errors are posted from Rust, so the webview's Content Security Policy needs no
BugSplat origin. Call `window.BugSplat.post(error, { key: "value" })` to report handled errors.

## Trying it

```sh
git clone --recursive https://github.com/BugSplat-Git/bugsplat-rs && cd bugsplat-rs
BUGSPLAT_DATABASE=my-db cargo run -p crasher -- segv      # also: abort, stack-overflow, panic,
                                                          # panic-thread, panic-ffi, capture, js
```

## Third-party licenses

bugsplat-rs is MIT. An app that ships `BugSplatMonitor` and `BugSplatReporter` also redistributes
the code compiled into them, so include these notices with your app (e.g. in an "Acknowledgements"
file or About box):

| Component | License | Platforms |
|---|---|---|
| [bugsplat-native](https://github.com/BugSplat-Git/bugsplat-native) | MIT | all |
| [Crashpad](https://chromium.googlesource.com/crashpad/crashpad) | Apache-2.0 | all |
| mini_chromium | BSD-3-Clause | all |
| miniz, nlohmann/json | MIT | all |
| lss (linux-syscall-support) | BSD-3-Clause | Linux |
| zlib | Zlib | Windows |
| getopt | Public domain | Windows |
| WTL | MS-PL | Windows (reporter) |

The license texts for bugsplat-native and its vendored code are in `bugsplat-sys/bugsplat-native/`
(`LICENSE`, `third_party/miniz/LICENSE`, `third_party/wtl/MS-PL.txt`; nlohmann/json's MIT notice is
the header of `third_party/nlohmann/json.hpp`). Crashpad's `LICENSE` is in the prebuilt that `bugsplat-sys`
downloads; prebuilts published after the current pin also carry the other components' licenses in
`third_party_licenses/`. Until then, take mini_chromium's, lss's and zlib's from their
repositories.

The Rust crates `bugsplat` links are MIT or Apache-2.0. The `js` feature adds an HTTPS client whose
dependencies include ISC (`ring`, `rustls-webpki`, `untrusted`), BSD-3-Clause (`subtle`) and
CDLA-Permissive-2.0 (`webpki-roots`, Mozilla's CA certificates) code. A tool such as
[cargo-about](https://github.com/EmbarkStudios/cargo-about) can generate the notices for your
app's whole dependency tree.

## Development

> The `bugsplat-native` submodule is pinned to the head of
> [bugsplat-native#14](https://github.com/BugSplat-Git/bugsplat-native/pull/14) (a CMake fix that
> Linux builds need); repoint it at `feat/m1-core` once that PR is merged.

```sh
cargo test --workspace --all-features
node --test tauri-plugin-bugsplat/js/init.test.js
bugsplat-sys/scripts/generate-bindings.sh   # after changing bugsplat-native's header
```

`BUGSPLAT_NATIVE_SRC=/path/to/bugsplat-native` builds against a local checkout instead of the
submodule.

## License

MIT
