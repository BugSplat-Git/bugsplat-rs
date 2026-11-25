#!/usr/bin/env bash
# Publishes the workspace's crates to crates.io, in dependency order. Used by
# .github/workflows/release.yml; see RELEASING.md.
#
#   scripts/publish-crates.sh check [TAG]   # print the version; fail if the crates disagree on it,
#                                           # Cargo.lock is stale, or TAG is not v<version>
#   scripts/publish-crates.sh plan          # which crates a publish would upload and which it
#                                           # skips, then `cargo publish --dry-run` for the former
#   scripts/publish-crates.sh publish       # upload the crates not yet on crates.io (needs
#                                           # CARGO_REGISTRY_TOKEN)
#
# plan and publish package bugsplat-sys, so they need the bugsplat-native submodule.
#
# A version already on crates.io is skipped, so a release that failed partway can be run again.
# `publish` does not build the crates (--no-verify): CI's `cargo package` step builds each one from
# its .crate on Linux, macOS and Windows, and the release workflow runs CI on the tagged commit
# before this runs.
set -euo pipefail
cd "$(dirname "$0")/.."

# Dependency order. Every other workspace member must be `publish = false` (`check` enforces it).
crates=(bugsplat-sys bugsplat tauri-plugin-bugsplat)

check() {
  local tag=${1:-}
  # --locked also fails when Cargo.lock was not updated after the version bump, which would leave
  # the published crates' lockfiles different from the one CI tested.
  local meta
  meta=$(cargo metadata --locked --format-version 1) || {
    echo "error: cargo metadata --locked failed; if Cargo.lock needs updating, run cargo update --workspace" >&2
    exit 1
  }
  python3 -c '
import json, sys
crates, tag = sys.argv[1].split(), sys.argv[2]
meta = json.load(sys.stdin)
members = [p for p in meta["packages"] if p["id"] in meta["workspace_members"]]
errors = []
versions = {p["name"]: p["version"] for p in members if p["name"] in crates}
for name in crates:
    if name not in versions:
        errors.append(f"{name} is not a workspace member")
if len(set(versions.values())) > 1:
    errors.append(f"the crates have different versions: {versions}")
version = versions.get(crates[0], "")
for p in members:
    name = p["name"]
    if name in crates:
        # Each crate must require the version published with it, not an older one.
        for d in p["dependencies"]:
            dep, req = d["name"], d["req"]
            if dep in crates and req != f"^{version}":
                errors.append(f"{name} requires {dep} {req}, not ^{version} "
                              "(update [workspace.dependencies] in Cargo.toml)")
    elif p["publish"] != []:
        errors.append(f"{name} is publishable but not in the release; "
                      "add it to scripts/publish-crates.sh or set publish = false")
if tag and tag != f"v{version}":
    errors.append(f"tag {tag} does not match the workspace version {version} (expected v{version})")
for e in errors:
    print(f"error: {e}", file=sys.stderr)
if errors:
    sys.exit(1)
print(version)
' "${crates[*]}" "$tag" <<<"$meta"
}

# Exit status 0 if crate $1 version $2 is on crates.io, 1 if not; exits the script on any other answer.
published() {
  local code
  code=$(curl -sS --retry 3 -o /dev/null -w '%{http_code}' \
    -A 'bugsplat-rs release workflow (https://docs.bugsplat.com/integrations/desktop/rust)' \
    "https://crates.io/api/v1/crates/$1/$2") || code=000
  case "$code" in
    200) return 0 ;;
    404) return 1 ;;
    *) echo "error: crates.io answered HTTP $code for $1 $2" >&2; exit 1 ;;
  esac
}

mode=${1:-}
case "$mode" in
  check)
    check "${2:-}"
    ;;
  plan|publish)
    version=$(check)
    pending=()
    for crate in "${crates[@]}"; do
      if published "$crate" "$version"; then
        echo "$crate $version is already on crates.io; skipping"
      elif [ "$mode" = plan ]; then
        echo "$crate $version would be published"
        pending+=(-p "$crate")
      else
        echo "Publishing $crate $version"
        # cargo waits until the new version is in the index before returning, so the next crate
        # can resolve it.
        cargo publish --locked --no-verify -p "$crate"
      fi
    done
    # One multi-package dry run: cargo resolves each crate against the others' unpublished
    # versions, which a per-crate dry run cannot (bugsplat-sys $version is not on crates.io yet).
    if [ "$mode" = plan ] && [ ${#pending[@]} -gt 0 ]; then
      cargo publish --dry-run --locked --no-verify "${pending[@]}"
    fi
    ;;
  *)
    sed -n '2,12p' "$0" >&2
    exit 2
    ;;
esac
