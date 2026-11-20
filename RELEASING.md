# Releasing

Pushing a `v*` tag runs `.github/workflows/release.yml`, which publishes `bugsplat-sys`,
`bugsplat` and `tauri-plugin-bugsplat` to crates.io, in that order, and creates the GitHub
release. `crasher` and `my-tauri-crasher` are `publish = false` and are never published.

## Steps

1. On a branch, set the new version in the workspace `Cargo.toml`, in three places:
   `[workspace.package] version` and the `version` of `bugsplat-sys` and `bugsplat` under
   `[workspace.dependencies]`. For a minor or major release, also update the `bugsplat = "0.x"`
   and `tauri-plugin-bugsplat = "0.x"` lines in `README.md`.
2. Run `cargo update --workspace` to update `Cargo.lock`, then `scripts/publish-crates.sh check`,
   which prints the version and fails if the crates or `Cargo.lock` disagree.
3. Open a pull request and merge it once CI passes.
4. Optional: rehearse from the Actions tab. Run the Release workflow on `main` with dry-run on.
   It runs every check and `cargo publish --dry-run`, and publishes nothing.
5. Tag the merge commit and push the tag:

   ```sh
   git tag v0.3.0 <merge-commit>
   git push origin v0.3.0
   ```

The workflow then:

1. Fails unless the tag is `v` followed by the workspace version, and runs
   `cargo publish --dry-run` for the crates not yet on crates.io.
2. Runs CI (`ci.yml`) on the tagged commit: tests, clippy and `cargo package` on Linux, macOS and
   Windows.
3. Publishes each crate whose version is not on crates.io yet, using a short-lived token from
   crates.io trusted publishing. This job runs in the `release` environment.
4. Creates the GitHub release for the tag with generated notes. A version with a pre-release
   suffix (`0.3.0-rc.1`) is marked as a pre-release.

## When a release fails partway

Publishing skips every version that is already on crates.io, so a failed release can be run
again from the same tag: use "Re-run failed jobs" on the run, or run the Release workflow on the
tag with dry-run off. A crates.io version cannot be replaced; if a published crate is broken,
yank it (`cargo yank --version 0.3.0 <crate>`) and release a new patch version.

## One-time setup

- **crates.io trusted publishing**, for each of `bugsplat-sys`, `bugsplat` and
  `tauri-plugin-bugsplat`: on the crate's Settings page on crates.io, under Trusted Publishing,
  add a GitHub publisher with repository owner `BugSplat-Git`, repository name `bugsplat-rs`,
  workflow filename `release.yml` and environment `release`.
- **The `release` environment** in the repository's Settings > Environments. GitHub creates it on
  the first run if it does not exist. Limit its deployment tags to `v*` and, if you want a manual
  approval before anything is published, add a required reviewer.
- **`BUGSPLAT_NATIVE_TOKEN`**, the repository secret CI already uses to clone the private
  `bugsplat-native` submodule.

A new crate has to be published once with an API token before trusted publishing can be set up
for it. For that release, add a `CARGO_REGISTRY_TOKEN` secret to the `release` environment (a
publish-scoped token from crates.io's API Tokens page); while the secret exists, the workflow
uses it instead of trusted publishing for every crate. Delete the secret and the token
afterwards.
