#!/usr/bin/env bash
# release-please cannot bump Cargo.lock with a toml jsonpath filter: array
# filters compare tagged values and match nothing
# (googleapis/release-please#2455). --fix rewrites only the coco-voice
# [[package]] version so it matches Cargo.toml.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

if [ "${1:-}" = "--fix" ]; then
  fix=1
elif [ $# -eq 0 ]; then
  fix=0
else
  echo "usage: scripts/check-versions.sh [--fix]" >&2
  exit 2
fi

json_version() {
  local file=$1
  local value
  if ! value=$(jq -r '.version' "$file"); then
    echo "failed to read .version from ${file}" >&2
    exit 1
  fi
  if [ "$value" = "null" ]; then
    value=
  fi
  printf '%s' "$value"
}

# Only the [package] table. A later table can also contain `version = "..."`.
cargo_version() {
  awk '
    /^\[/ { in_package = ($0 == "[package]"); next }
    in_package && /^version[[:space:]]*=/ {
      if (match($0, /"[^"]+"/)) {
        print substr($0, RSTART + 1, RLENGTH - 2)
      }
      exit
    }
  ' "$1"
}

require_version() {
  local label=$1
  local value=$2
  case $value in
    "")
      echo "${label} version is empty" >&2
      exit 1
      ;;
    [0-9]*) ;;
    *)
      echo "${label} version does not start with a digit: ${value}" >&2
      exit 1
      ;;
  esac
}

lock_version() {
  awk '
    $0 == "[[package]]" { in_block = 1; is_coco = 0; next }
    in_block && $0 == "name = \"coco-voice\"" { is_coco = 1; next }
    in_block && is_coco && /^version = "/ {
      if (match($0, /"[^"]+"/)) {
        print substr($0, RSTART + 1, RLENGTH - 2)
      }
      exit
    }
  ' "$1"
}

rewrite_lock() {
  local want=$1
  local src="src-tauri/Cargo.lock"
  local tmp="${src}.tmp.$$"
  if ! awk -v ver="$want" '
    $0 == "[[package]]" { in_block = 1; is_coco = 0; print; next }
    in_block && $0 == "name = \"coco-voice\"" { is_coco = 1; print; next }
    in_block && is_coco && /^version = "/ {
      print "version = \"" ver "\""
      replaced = 1
      is_coco = 0
      next
    }
    { print }
    END { if (!replaced) exit 2 }
  ' "$src" >"$tmp"; then
    rm -f "$tmp"
    echo "Cargo.lock has no coco-voice package entry" >&2
    exit 1
  fi
  mv "$tmp" "$src"
}

pkg=$(json_version package.json)
tauri=$(json_version src-tauri/tauri.conf.json)
cargo=$(cargo_version src-tauri/Cargo.toml)
lock=$(lock_version src-tauri/Cargo.lock)

require_version "package.json" "$pkg"
require_version "tauri.conf.json" "$tauri"
require_version "Cargo.toml" "$cargo"

if [ "$fix" -eq 1 ] && [ "$lock" != "$cargo" ]; then
  rewrite_lock "$cargo"
  lock=$(lock_version src-tauri/Cargo.lock)
  echo "updated Cargo.lock coco-voice version to ${cargo}"
fi

require_version "Cargo.lock" "$lock"

if [ "$pkg" = "$tauri" ] && [ "$pkg" = "$cargo" ] && [ "$pkg" = "$lock" ]; then
  echo "versions match: ${pkg}"
  exit 0
fi

echo "version files disagree:" >&2
echo "  package.json:    ${pkg:-<missing>}" >&2
echo "  tauri.conf.json: ${tauri:-<missing>}" >&2
echo "  Cargo.toml:      ${cargo:-<missing>}" >&2
echo "  Cargo.lock:      ${lock:-<missing>}" >&2
exit 1
