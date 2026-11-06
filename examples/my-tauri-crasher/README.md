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
and `src-tauri/build.rs` plus `tauri.conf.json` (bundling the helpers on macOS). The
[Rust and Tauri guide](https://docs.bugsplat.com/integrations/desktop/rust) walks through each step.

## Run it

Requirements: Rust, Node.js, CMake and Ninja, plus [Tauri's prerequisites](https://tauri.app/start/prerequisites/).

```sh
npm install
npm run dev     # debug build; crashes go to BugSplat's public fred database
```

Reports go to `fred` unless `BUGSPLAT_DATABASE` names your database. `BUGSPLAT_QUIET=1` uploads
without showing the crash dialog.

## Symbols

Build a release, then upload its symbols so BugSplat can show Rust function names, files and line
numbers:

```sh
BUGSPLAT_DATABASE=your-bugsplat-database npm run build
SYMBOL_UPLOAD_CLIENT_ID=... SYMBOL_UPLOAD_CLIENT_SECRET=... BUGSPLAT_DATABASE=your-bugsplat-database npm run symbols
```

`npm run symbols` uploads `my-tauri-crasher.dSYM` from the workspace's `target/release` (macOS).
See the guide for Windows (`.pdb`) and Linux (`.debug`). Create client credentials on the
[Integrations](https://app.bugsplat.com/v2/settings/database/integrations) page.
