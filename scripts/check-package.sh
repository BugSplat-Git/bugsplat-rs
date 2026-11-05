#!/usr/bin/env bash
# Checks what the published crates contain. bugsplat-sys is how bugsplat-native's source is
# published, so it must carry only what the build reads (its `include` list), and no published
# file may link to a repository that is still private.
#
#   scripts/check-package.sh            # from the workspace root; needs the submodule
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
sys_files=$(cargo package --list -p bugsplat-sys --allow-dirty)

# Directories of bugsplat-native that are not part of the build.
unexpected=$(grep -E '^bugsplat-native/(\.github|bindings|docs|samples|tests|reporter/docs|third_party/miniz/examples|scripts)/' <<<"$sys_files" |
             grep -vxF 'bugsplat-native/scripts/apple/codesign.sh' || true)
if [ -n "$unexpected" ]; then
  echo "bugsplat-sys would publish files the build does not need:" >&2
  echo "$unexpected" >&2
  fail=1
fi

# License texts must ship with the code they cover.
for f in bugsplat-native/LICENSE bugsplat-native/third_party/miniz/LICENSE \
         bugsplat-native/third_party/wtl/MS-PL.txt bugsplat-native/third_party/nlohmann/LICENSE.MIT; do
  grep -qxF "$f" <<<"$sys_files" || { echo "bugsplat-sys is missing $f" >&2; fail=1; }
done

private='github\.com/BugSplat-Git/(bugsplat-rs|bugsplat-native)\b'
for crate in bugsplat-sys bugsplat tauri-plugin-bugsplat; do
  dir=$(cargo metadata --no-deps --format-version 1 |
        python3 -c "import json,sys; print([p['manifest_path'] for p in json.load(sys.stdin)['packages'] if p['name']=='$crate'][0].rsplit('/',1)[0])")
  while IFS= read -r f; do
    case "$f" in Cargo.toml.orig|.cargo_vcs_info.json|Cargo.lock) continue ;; esac
    # bugsplat-native's own README and CMake may name its repository; only bugsplat-rs's
    # files are shown on crates.io and docs.rs.
    case "$f" in bugsplat-native/*) continue ;; esac
    src="$dir/$f"
    [ "$f" = README.md ] && [ ! -f "$src" ] && src="$dir/../README.md"
    if [ -f "$src" ] && grep -nE "$private" "$src" >/dev/null; then
      echo "$crate: $f links to a private repository:" >&2
      grep -nE "$private" "$src" >&2
      fail=1
    fi
  done < <(cargo package --list -p "$crate" --allow-dirty)
done

# The manifest links crates.io shows (members inherit these from the workspace).
links=$(cargo metadata --no-deps --format-version 1 | python3 -c '
import json, re, sys
for p in json.load(sys.stdin)["packages"]:
    if p["publish"] == []:
        continue
    for key in ("repository", "homepage", "documentation"):
        if p.get(key) and re.search(r"github\.com/BugSplat-Git/(bugsplat-rs|bugsplat-native)\b", p[key]):
            print(p["name"], key, p[key])')
if [ -n "$links" ]; then
  echo "manifest fields link to a private repository:" >&2
  echo "$links" >&2
  fail=1
fi

[ "$fail" = 0 ] && echo "package contents OK"
exit "$fail"
