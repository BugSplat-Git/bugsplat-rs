# bugsplat-rs

Crash reporting for Rust applications with [BugSplat](https://www.bugsplat.com), built on
bugsplat-native, BugSplat's native SDK (Crashpad, out of process). Setup guide:
[docs.bugsplat.com](https://docs.bugsplat.com/integrations/desktop/rust).

| Crate | What it is |
|---|---|
| [`bugsplat`](https://crates.io/crates/bugsplat) | Safe API: `init`, attributes, attachments, non-fatal captures, Rust panic reports, and (feature `js`) JavaScript errors from embedded webviews |
| [`tauri-plugin-bugsplat`](https://crates.io/crates/tauri-plugin-bugsplat) | Tauri plugin: catches uncaught JS errors and unhandled rejections in every webview |
| [`bugsplat-sys`](https://crates.io/crates/bugsplat-sys) | Raw FFI; builds bugsplat-native from source with CMake and links it statically |

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
toolset 14.44+, clang 15+/gcc 13+). The pinned Crashpad prebuilt is downloaded and verified by
bugsplat-native's CMake; set `BUGSPLAT_CRASHPAD_CACHE` to share it across target directories.

On Linux:

- **glibc 2.35 or newer** (Ubuntu 22.04, Debian 12, Fedora 36, RHEL 10 and later), to build and to run.
  The Crashpad prebuilt is built on Ubuntu 22.04 and checked to need nothing newer than
  `GLIBC_2.35`. On older systems the link fails with undefined references to newer glibc symbols.
- **GCC 13 or newer, or Clang.** Crashpad's minidump headers declare
  `struct alignas(4) __attribute__((packed))`, which GCC 12 and older reject ("expected
  identifier before '__attribute__'"). Ubuntu 22.04 and Debian 12 ship GCC 11 and 12, so install
  Clang there (`apt install clang`) and set `CC=clang CXX=clang++`.

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
| Windows | `target/release/myapp.pdb` | Always written by MSVC. Hyphens in the binary name become underscores: `my-app.exe` has `my_app.pdb` |

```sh
# macOS
npx @bugsplat/symbol-upload -b my-database -a MyApp -v 1.2.3 -d target/release -f "myapp.dSYM" -m
# Windows
npx @bugsplat/symbol-upload -b my-database -a MyApp -v 1.2.3 -d target/release -f "myapp.pdb" -m
```

`application` and `version` must match the values passed to `Options::new`.

The crate posts every crash as a Crashpad minidump (`CrashType::Crashpad`), Windows included, so the
same `.sym` files work on all three platforms. To have Windows crashes read `.pdb` files instead,
set `.crash_type(bugsplat::CrashType::WindowsNative)` and upload the `.pdb` and `.exe` without `-m`.

### Packaging the helpers

`BugSplatMonitor` (captures crashes) and `BugSplatReporter` (dialog and upload) are separate
executables that ship with your app. On Windows, so does `BugSplatWer.dll`, the Windows Error
Reporting helper (see [Windows: fail-fast crashes](#windows-fail-fast-crashes)). `bugsplat-sys`
builds them. Cargo passes their paths only to the build scripts of crates that depend on
`bugsplat-sys` directly, so add it to your `[dependencies]` next to `bugsplat` and read
`DEP_BUGSPLAT_MONITOR` / `DEP_BUGSPLAT_REPORTER` / `DEP_BUGSPLAT_WER` (Windows only) in your
`build.rs`. Tauri apps read `DEP_TAURI_PLUGIN_BUGSPLAT_MONITOR` /
`DEP_TAURI_PLUGIN_BUGSPLAT_REPORTER` / `DEP_TAURI_PLUGIN_BUGSPLAT_WER` instead (see
[Tauri](#tauri)).

bugsplat-native looks for the monitor and reporter next to the executable, in `Helpers/`, and on
macOS in `<App>.app/Contents/Helpers`. During development (`cargo run`) nothing needs packaging:
when no packaged helpers are found, `bugsplat` uses the ones built for this compilation.

### Windows: fail-fast crashes

Crashes that never reach Crashpad's exception filter (`__fastfail`, `/GS` stack buffer overruns,
some heap corruption) are handed to Windows Error Reporting, which passes them to BugSplat only
through `BugSplatWer.dll`. `bugsplat-sys` links the SDK into your executable, so the SDK looks for
the DLL **next to the executable** and logs `BugSplatWer.dll not found next to the library` when
it is missing. Two things are needed:

1. **Ship `BugSplatWer.dll` next to your `.exe`.** `cargo build` and `cargo run` copy it into
   `target/<profile>/` (and its `examples/`) for you; your installer must put it next
   to the installed `.exe` (for Tauri, see below).
2. **Allow-list its full path.** WER calls only helpers listed under
   `SOFTWARE\Microsoft\Windows\Windows Error Reporting\RuntimeExceptionHelperModules` as a
   `REG_DWORD` value named after the DLL's path. A machine-wide installer (elevated, Program Files)
   writes it under `HKLM`; a per-user installer (`%LOCALAPPDATA%`, no elevation) writes it under
   `HKCU`. Remove it on uninstall. To try it from `target\debug`:

   ```bat
   reg add "HKCU\SOFTWARE\Microsoft\Windows\Windows Error Reporting\RuntimeExceptionHelperModules" /v "%CD%\target\debug\BugSplatWer.dll" /t REG_DWORD /d 0
   ```

`BugSplat.log` (in `%LOCALAPPDATA%\BugSplat\<app>-<version>\`) records the result on every
start: `WER helper ...: registered=1 allow-listed=1 -> coverage on`.
`unsafe { bugsplat_sys::bugsplat_has_capability(bugsplat_sys::bugsplat_capability::BUGSPLAT_CAP_WER) }`
returns 1 only when both steps are done. Hardware faults in your own code (access violations, stack overflows) do not need
WER.

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

Add `"bugsplat:default"` to your capability. To bundle the helpers, have `build.rs` copy the
paths in `DEP_TAURI_PLUGIN_BUGSPLAT_MONITOR`, `DEP_TAURI_PLUGIN_BUGSPLAT_REPORTER` and (Windows)
`DEP_TAURI_PLUGIN_BUGSPLAT_WER` into a `bugsplat-helpers/` directory before calling
`tauri_build::build()` (`examples/my-tauri-crasher/src-tauri/build.rs` does this), then map them
in the bundle configuration.

macOS, in `tauri.conf.json`:

```json
"bundle": { "macOS": { "files": {
  "Helpers/BugSplatMonitor": "bugsplat-helpers/BugSplatMonitor",
  "Helpers/BugSplatReporter.app": "bugsplat-helpers/BugSplatReporter.app"
} } }
```

Windows, in `tauri.windows.conf.json` next to it (Tauri merges it into `tauri.conf.json` on
Windows). On Windows, `bundle.resources` are installed next to the `.exe` by both the NSIS and MSI
bundles, and `tauri_build` also copies them into `target/<profile>` for `cargo tauri dev`:

```json
{
  "bundle": {
    "resources": {
      "bugsplat-helpers/BugSplatMonitor.exe": "BugSplatMonitor.exe",
      "bugsplat-helpers/BugSplatReporter.exe": "BugSplatReporter.exe",
      "bugsplat-helpers/BugSplatWer.dll": "BugSplatWer.dll"
    },
    "windows": {
      "nsis": { "installerHooks": "windows/bugsplat-hooks.nsh" },
      "wix": {
        "fragmentPaths": ["windows/bugsplat-wer.wxs"],
        "componentRefs": ["BugSplatWerAllowList"]
      }
    }
  }
}
```

The NSIS hooks and the WiX fragment write the WER allow-list entry for the installed
`BugSplatWer.dll` and remove it on uninstall: Tauri's default NSIS install is per user, so the hook
writes `HKCU` (`HKLM` with `"installMode": "perMachine"`), and the MSI, which installs per machine,
writes `HKLM`. Copy both files from `examples/my-tauri-crasher/src-tauri/windows/`; if you already
have NSIS hooks, add the two macros' lines to yours.

JavaScript errors are posted from Rust, so the webview's Content Security Policy needs no
BugSplat origin. Call `window.BugSplat.post(error, { key: "value" })` to report handled errors.

## Trying it

From a checkout of this repository:

```sh
BUGSPLAT_DATABASE=my-db cargo run -p crasher -- segv      # also: abort, stack-overflow, panic,
                                                          # panic-thread, panic-ffi, capture, js
```

`examples/my-tauri-crasher` is a sample Tauri app with a button for each kind of failure: a native
crash, a Rust panic, and JavaScript errors. See its README.

## Third-party licenses

bugsplat-rs is MIT. An app that ships `BugSplatMonitor`, `BugSplatReporter` and `BugSplatWer.dll` also redistributes
the code compiled into them, so include these notices with your app (e.g. in an "Acknowledgements"
file or About box):

| Component | License | Platforms |
|---|---|---|
| bugsplat-native | MIT | all |
| [Crashpad](https://chromium.googlesource.com/crashpad/crashpad) | Apache-2.0 | all |
| mini_chromium | BSD-3-Clause | all |
| miniz, nlohmann/json | MIT | all |
| lss (linux-syscall-support) | BSD-3-Clause | Linux |
| zlib | Zlib | Windows |
| getopt | Public domain | Windows |
| WTL | MS-PL | Windows (reporter) |

The license texts for bugsplat-native and its vendored code are in `bugsplat-sys/bugsplat-native/`
(`LICENSE`, `third_party/miniz/LICENSE`, `third_party/wtl/MS-PL.txt`; nlohmann/json's MIT notice is
the header of `third_party/nlohmann/json.hpp`). The Crashpad prebuilt that `bugsplat-sys` downloads
has Crashpad's `LICENSE` and, in `third_party_licenses/`, those of mini_chromium, lss, zlib and getopt
(each platform's prebuilt carries the ones it uses).

The Rust crates `bugsplat` links are MIT or Apache-2.0. The `js` feature adds an HTTPS client whose
dependencies include ISC (`ring`, `rustls-webpki`, `untrusted`), BSD-3-Clause (`subtle`) and
CDLA-Permissive-2.0 (`webpki-roots`, Mozilla's CA certificates) code. A tool such as
[cargo-about](https://github.com/EmbarkStudios/cargo-about) can generate the notices for your
app's whole dependency tree.

## Development

```sh
cargo test --workspace --all-features
node --test tauri-plugin-bugsplat/js/init.test.js
bugsplat-sys/scripts/generate-bindings.sh   # after changing bugsplat-native's header
```

`BUGSPLAT_NATIVE_SRC=/path/to/bugsplat-native` builds against a local checkout instead of the
submodule.

## License

MIT
