# my-tauri-crasher

A sample [Tauri](https://tauri.app) 2 app integrated with BugSplat through
[`bugsplat`](https://crates.io/crates/bugsplat) and
[`tauri-plugin-bugsplat`](https://crates.io/crates/tauri-plugin-bugsplat). Each button triggers a
different kind of failure:

| Button | What happens | Arrives at BugSplat as |
| --- | --- | --- |
| Native Crash | A Tauri command writes through a null pointer | A minidump captured by `BugSplatMonitor`, out of process |
| Rust Panic | A Tauri command indexes past the end of a vector | A minidump with the `rust.panic.*` attributes |
| JavaScript Error | An uncaught exception in the page | A JavaScript report with the `Error.cause` chain |
| Unhandled Rejection | A rejected promise nobody awaits | A JavaScript report |
| Handled Error | An error caught and posted with `window.BugSplat.post` | A JavaScript report with a `screen` attribute |

The integration is in `src-tauri/src/main.rs` (`bugsplat::init` before Tauri starts),
`src-tauri/src/lib.rs` (the plugin), `src-tauri/capabilities/default.json` (`bugsplat:default`),
and `src-tauri/build.rs` plus `tauri.conf.json` and `tauri.windows.conf.json` (bundling the helpers
on macOS and Windows). The
[Rust and Tauri guide](https://docs.bugsplat.com/integrations/desktop/rust) walks through each step.

## Run it

Requirements: Rust, Node.js, CMake and Ninja, plus [Tauri's prerequisites](https://tauri.app/start/prerequisites/).

```sh
npm install
npm run dev     # debug build; crashes go to BugSplat's public fred database
```

Reports go to `fred` unless `BUGSPLAT_DATABASE` names your database. The app reads it when it starts
(`bugsplat_options()` in `src-tauri/src/lib.rs`); it is not built in, so set it where you launch the
app. `BUGSPLAT_QUIET=1` uploads without showing the crash dialog.

## Windows

`npm run build` makes an NSIS installer and an MSI in `../../target/release/bundle/`. Both install
`BugSplatMonitor.exe`, `BugSplatReporter.exe` and `BugSplatWer.dll` next to `my-tauri-crasher.exe`
(`bundle.resources` in `src-tauri/tauri.windows.conf.json`), and both allow-list
`BugSplatWer.dll` for Windows Error Reporting so fail-fast crashes are reported:
`src-tauri/windows/bugsplat-hooks.nsh` writes the entry under HKCU for the default per-user NSIS
install, and `src-tauri/windows/bugsplat-wer.wxs` writes it under HKLM for the MSI. The uninstallers
remove it. `npm run dev` gets the same files in `../../target/debug`; to allow-list that copy, see
[Windows: fail-fast crashes](../../README.md#windows-fail-fast-crashes).

## Symbols

Build a release, upload its symbols so BugSplat can show Rust function names, files and line
numbers, then run it against the same database:

```sh
export BUGSPLAT_DATABASE=your-bugsplat-database
npm run build
SYMBOL_UPLOAD_CLIENT_ID=... SYMBOL_UPLOAD_CLIENT_SECRET=... npm run symbols
../../target/release/bundle/macos/my-tauri-crasher.app/Contents/MacOS/my-tauri-crasher
```

Launched from Finder instead, the app gets no `BUGSPLAT_DATABASE` and reports to `fred`, which won't
have your symbols.

`npm run symbols` uploads `my-tauri-crasher.dSYM` from the workspace's `target/release` (macOS).
On Linux, point `-f` at `my-tauri-crasher.debug` and keep `-m`. On Windows, convert
`my-tauri-crasher.exe` (not the `.pdb`) and upload the result as `my_tauri_crasher.sym`, since
Cargo names the PDB with underscores: follow
[Windows: convert the `.exe`](../../README.md#windows-convert-the-exe) in the repository README.
Create client credentials on
the [Integrations](https://app.bugsplat.com/v2/settings/database/integrations) page.
