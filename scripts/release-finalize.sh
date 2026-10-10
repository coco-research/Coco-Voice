#!/usr/bin/env bash
# Publish a release-please draft.
# The by-tag releases endpoint 404s on drafts, so the body is read with
# `gh release view`. --verify-release-commit (workflow_dispatch) only flips
# an existing draft that already has its assets. It never uploads.
# Dispatch reads release notes from main on purpose (CTO decision, so notes
# fixed after tagging ship). Only a draft can be edited, so a published
# release is never rewritten. isDraft is read again immediately before edit.
set -euo pipefail

# One scratch dir (downloads + notes). main sets WORK; EXIT removes it.
WORK=""
cleanup() {
  if [[ -n "$WORK" ]]; then
    rm -rf "$WORK" || true
    WORK=""
  fi
}
trap cleanup EXIT

MARKER='<!-- coco-voice-release-notes -->'
PARAGRAPH='These builds are not signed or notarized. Verify downloads with SHA256SUMS. On macOS, right-click the app and choose Open the first time.'

usage() {
  printf '%s\n' 'usage: release-finalize.sh --tag vX.Y.Z [--verify-release-commit] [--dry-run] [--self-test]'
}

tag_ok() {
  [[ "$1" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]
}

version_of() {
  printf '%s\n' "${1#v}"
}

# Subject is exactly "chore: release <version>" or that plus " (#<digits>)".
subject_ok() {
  local subject=$1
  local version=$2
  local prefix="chore: release ${version}"
  local rest n
  if [[ "$subject" == "$prefix" ]]; then
    return 0
  fi
  rest=${subject#"${prefix} "}
  if [[ "$rest" == "$subject" ]]; then
    return 1
  fi
  case "$rest" in
    '(#'[0-9]*')') ;;
    *) return 1 ;;
  esac
  n=${rest#"(#"}
  n=${n%)}
  [[ -n "$n" ]] || return 1
  case "$n" in
    *[!0-9]*) return 1 ;;
  esac
  [[ "$rest" == "(#${n})" ]]
}

# List lines that share a key collapse to one line, kept at the first
# occurrence. The key drops release-please links of the form ([text](url))
# and a trailing ", closes [#N](url)" clause (one or more space-separated
# [#N](url) refs). A variant that contains a PR link ([#NN]( wins over one
# that does not. Case and punctuation stay in the key.
dedupe_changelog() {
  python3 -c "$(cat <<'PY'
import re
import sys

link_re = re.compile(r"\(\[[^]]*\]\([^)]*\)\)")
closes_re = re.compile(r",\s*closes(\s+\[[^\]]*\]\([^)]*\))+\s*$")
pr_re = re.compile(r"\(\[#\d+\]\(")


def is_item(line):
    return line.startswith("* ") or line.startswith("- ")


def key(line):
    text = closes_re.sub("", link_re.sub("", line))
    return " ".join(text.split())


text = sys.stdin.read()
if text.endswith("\n"):
    text = text[:-1]
lines = text.split("\n") if text else []

first = {}
chosen = {}
for i, line in enumerate(lines):
    if not is_item(line):
        continue
    k = key(line)
    if k not in first:
        first[k] = i
        chosen[k] = line
    elif pr_re.search(line) and not pr_re.search(chosen[k]):
        chosen[k] = line

out = []
for i, line in enumerate(lines):
    if not is_item(line):
        out.append(line)
        continue
    k = key(line)
    if i == first[k]:
        out.append(chosen[k])

if out:
    sys.stdout.write("\n".join(out) + "\n")
PY
)"
}

body_has_marker() {
  case "$1" in
    *"$MARKER"*) return 0 ;;
    *) return 1 ;;
  esac
}

# Print notes (if any), the unsigned-build paragraph, the deduped body, the marker.
assemble_body() {
  local version=$1
  local existing=$2
  local notes="src/content/release-notes/${version}.md"
  local text
  if [[ -s "$notes" ]]; then
    text=$(cat "$notes"; printf x)
    text=${text%x}
    while [[ -n "$text" && "${text: -1}" == $'\n' ]]; do
      text=${text%$'\n'}
    done
    if [[ -n "$text" ]]; then
      printf '%s\n\n' "$text"
    fi
  fi
  printf '%s\n\n' "$PARAGRAPH"
  if [[ -n "$existing" ]]; then
    printf '%s\n' "$existing" | dedupe_changelog
  fi
  printf '%s\n' "$MARKER"
}

# Offline completeness gate. SHA256SUMS may list extra names that are not
# assets: releases up to 0.9.5 also hashed repo files under assets/branding.
check_release_complete() {
  if [[ $# -ne 4 ]]; then
    printf 'usage: check_release_complete <assets_json> <sha256sums> <latest_json> <version>\n' >&2
    return 1
  fi
  if ! python3 - "$@" <<'PY'
import json
import re
import sys
from urllib.parse import unquote, urlsplit

def fail(msg):
    print(msg, file=sys.stderr)
    sys.exit(1)

def load_json(path, label):
    try:
        with open(path, encoding="utf-8") as handle:
            return json.load(handle)
    except (OSError, json.JSONDecodeError) as err:
        fail("%s is not readable json: %s" % (label, err))

def load_sums(path):
    try:
        with open(path, encoding="utf-8") as handle:
            text = handle.read()
    except OSError as err:
        fail("SHA256SUMS is not readable: %s" % err)
    sums = {}
    line_re = re.compile(r"^([0-9a-fA-F]{64})  (.+)$")
    for lineno, raw in enumerate(text.splitlines(), 1):
        line = raw.rstrip("\r")
        if not line:
            continue
        match = line_re.match(line)
        if not match:
            fail("SHA256SUMS line %d is not '<hex>  <name>'" % lineno)
        sums[match.group(2)] = match.group(1).lower()
    return sums

def path_of(url):
    return unquote(urlsplit(url).path).rstrip("/")

def asset_id(api_url):
    if not isinstance(api_url, str):
        return None
    match = re.search(r"/releases/assets/([^/]+)$", path_of(api_url))
    if not match:
        return None
    return match.group(1)

def references_release(url, ids, names, tag):
    if not isinstance(url, str) or not url:
        return False
    path = path_of(url)
    match = re.search(r"/releases/assets/([^/]+)$", path)
    if match:
        return match.group(1) in ids
    match = re.search(r"/releases/download/([^/]+)/([^/]+)$", path)
    if match:
        return match.group(1) == tag and match.group(2) in names
    return False

assets_path, sums_path, latest_path, version = sys.argv[1:5]
doc = load_json(assets_path, "assets json")
assets = doc.get("assets") if isinstance(doc, dict) else doc
if not isinstance(assets, list):
    fail("assets json has no assets array")

sums = load_sums(sums_path)
ids = set()
names = set()
for asset in assets:
    if not isinstance(asset, dict):
        fail("release asset is not an object")
    name = asset.get("name")
    if not isinstance(name, str) or name == "":
        fail("release asset has no name")
    names.add(name)
    found = asset_id(asset.get("apiUrl"))
    if found:
        ids.add(found)
    if name == "SHA256SUMS":
        continue
    digest = asset.get("digest")
    if not isinstance(digest, str) or digest.strip() == "":
        fail("asset %s has no digest in gh output (needs gh >= 2.75 and a GitHub-computed digest)" % name)
    parsed = re.fullmatch(r"(?i)sha256:([0-9a-f]{64})", digest.strip())
    if not parsed:
        fail("asset %s has a malformed digest: %s" % (name, digest.strip()))
    got = parsed.group(1).lower()
    want = sums.get(name)
    if want is None:
        fail("asset %s is missing from SHA256SUMS" % name)
    if want != got:
        fail("asset %s digest %s does not match SHA256SUMS %s" % (name, got, want))

latest = load_json(latest_path, "latest.json")
if not isinstance(latest, dict):
    fail("latest.json is not an object")
got_version = latest.get("version")
if got_version != version:
    fail("latest.json version %s does not equal %s" % (got_version, version))

platforms = latest.get("platforms")
if isinstance(platforms, dict):
    entries = list(platforms.values())
elif isinstance(platforms, list):
    entries = list(platforms)
else:
    entries = []
if not entries:
    fail("latest.json platforms is empty")

tag = version if str(version).startswith("v") else "v" + str(version)
for entry in entries:
    url = entry.get("url") if isinstance(entry, dict) else None
    if not isinstance(url, str) or url == "":
        fail("latest.json platform entry has no url")
    if not references_release(url, ids, names, tag):
        fail("latest.json url %s does not reference an asset of this release" % url)
PY
  then
    return 1
  fi
}

refuse_published() {
  local tag=$1
  local rel=$2
  local draft
  draft=$(printf '%s' "$rel" | jq -r '.isDraft')
  if [[ "$draft" != "true" ]]; then
    printf 'release %s is already published; not editing it\n' "$tag" >&2
    exit 1
  fi
}

# First vX.Y.Z in a newest-first list. Caller sorts; this does not.
pick_latest_tag() {
  local line
  while IFS= read -r line || [[ -n "$line" ]]; do
    if tag_ok "$line"; then
      printf '%s\n' "$line"
      return 0
    fi
  done
  return 1
}

# stdout: --latest or --latest=false. Note on stderr when tag is not highest.
latest_arg() {
  local tag=$1
  local highest=$2
  if [[ -n "$highest" && "$tag" == "$highest" ]]; then
    printf '%s\n' --latest
    return 0
  fi
  printf 'note: %s is not the highest vX.Y.Z tag (%s); passing --latest=false\n' "$tag" "${highest:-(none)}" >&2
  printf '%s\n' --latest=false
}

# Push path always passes --latest. Verify mode only when TAG is the first
# vX.Y.Z from: git tag -l 'v*' --sort=-v:refname
resolve_latest_flag() {
  local tag=$1
  local verify=$2
  local highest
  if [[ "$verify" -eq 0 ]]; then
    printf '%s\n' --latest
    return 0
  fi
  highest=$(git tag -l 'v*' --sort=-v:refname | pick_latest_tag || true)
  latest_arg "$tag" "$highest"
}

warn_notes() {
  local notes="src/content/release-notes/${1}.md"
  if [[ ! -s "$notes" ]]; then
    printf 'warning: no release notes at %s; publishing without them\n' "$notes" >&2
  fi
}

load_verified_body() {
  local tag=$1
  local version=$2
  local commit subject manifest rel
  if [[ "$(git rev-parse --is-shallow-repository)" == "true" ]]; then
    printf 'shallow checkout: run with fetch-depth: 0\n' >&2
    exit 1
  fi
  git fetch --tags --force origin
  git fetch origin main
  if ! commit=$(git rev-parse --verify "refs/tags/${tag}^{commit}"); then
    printf 'tag %s does not exist\n' "$tag" >&2
    exit 1
  fi
  if ! git merge-base --is-ancestor "$commit" origin/main; then
    printf 'tag %s (%s) is not an ancestor of origin/main\n' "$tag" "$commit" >&2
    exit 1
  fi
  subject=$(git log -1 --format=%s "$commit")
  if ! subject_ok "$subject" "$version"; then
    printf 'tag %s commit subject must be '\''chore: release %s'\'' or '\''chore: release %s (#N)'\'', got: %s\n' \
      "$tag" "$version" "$version" "$subject" >&2
    exit 1
  fi
  if ! manifest=$(git show "${commit}:.release-please-manifest.json" | jq -r '."."'); then
    printf 'cannot read .release-please-manifest.json from %s (%s)\n' "$tag" "$commit" >&2
    exit 1
  fi
  if [[ "$manifest" != "$version" ]]; then
    printf 'release manifest for %s is %s, expected %s\n' "$tag" "$manifest" "$version" >&2
    exit 1
  fi
  if ! rel=$(gh release view "$tag" --json isDraft,assets,body); then
    printf 'release %s could not be read\n' "$tag" >&2
    exit 1
  fi
  refuse_published "$tag" "$rel"
  if ! printf '%s' "$rel" | jq -e '(.assets // [] | map(.name) | index("SHA256SUMS")) != null and (.assets // [] | map(.name) | index("latest.json")) != null' >/dev/null; then
    printf 'release %s is missing SHA256SUMS or latest.json; not editing it\n' "$tag" >&2
    exit 1
  fi
  printf '%s\n' "$rel" | jq '{assets:.assets}' >"${WORK}/assets.json"
  if ! gh release download "$tag" --pattern SHA256SUMS --pattern latest.json --dir "$WORK"; then
    printf 'release %s: could not download SHA256SUMS and latest.json\n' "$tag" >&2
    exit 1
  fi
  if ! check_release_complete "${WORK}/assets.json" "${WORK}/SHA256SUMS" "${WORK}/latest.json" "$version"; then
    exit 1
  fi
  BODY=$(printf '%s' "$rel" | jq -r '.body // ""')
}

load_body() {
  local tag=$1
  local rel
  if ! rel=$(gh release view "$tag" --json body,isDraft); then
    printf 'release %s could not be read\n' "$tag" >&2
    exit 1
  fi
  refuse_published "$tag" "$rel"
  BODY=$(printf '%s' "$rel" | jq -r '.body // ""')
}

apply_edit() {
  local tag=$1
  local notes_file=$2
  local latest_flag=$3
  local rel
  if [[ "${DRY}" -eq 1 ]]; then
    if [[ -n "$notes_file" ]]; then
      cat "$notes_file"
      printf 'gh release edit %q --notes-file %q --draft=false %s\n' "$tag" "$notes_file" "$latest_flag"
    else
      printf 'gh release edit %q --draft=false %s\n' "$tag" "$latest_flag"
    fi
    return 0
  fi
  if ! rel=$(gh release view "$tag" --json isDraft); then
    printf 'release %s could not be read\n' "$tag" >&2
    exit 1
  fi
  refuse_published "$tag" "$rel"
  if [[ -n "$notes_file" ]]; then
    gh release edit "$tag" --notes-file "$notes_file" --draft=false "$latest_flag"
  else
    gh release edit "$tag" --draft=false "$latest_flag"
  fi
}

self_test() {
  local t ver got want
  local fail_tags=(
    ''
    '1.2.3'
    'v1.2'
    'v1.2.3-rc1'
    'v1.2.3 '
    'v1.2.3.4'
    'V1.2.3'
    'v1.2.3;rm'
  )
  for t in "${fail_tags[@]}"; do
    if tag_ok "$t"; then
      printf 'FAIL: accepted invalid tag: %s\n' "$t" >&2
      exit 1
    fi
  done
  for t in v0.9.5 v10.0.0 v01.02.03; do
    if ! tag_ok "$t"; then
      printf 'FAIL: rejected valid tag: %s\n' "$t" >&2
      exit 1
    fi
  done
  ver=$(version_of v0.9.5)
  if [[ "$ver" != "0.9.5" ]]; then
    printf 'FAIL: version_of v0.9.5 -> %s\n' "$ver" >&2
    exit 1
  fi

  if ! subject_ok 'chore: release 0.9.5' '0.9.5'; then
    printf 'FAIL: bare release subject rejected\n' >&2
    exit 1
  fi
  if ! subject_ok 'chore: release 0.9.5 (#67)' '0.9.5'; then
    printf 'FAIL: squash release subject rejected\n' >&2
    exit 1
  fi
  local bad_subject
  for bad_subject in \
    'chore: release 0.9.5 (#67) ' \
    'chore: release 0.9.5 (#x)' \
    'chore: release 0.9.50' \
    'chore: Release 0.9.5' \
    'chore: release 0.9.5 (#67) trailing' \
    'chore: release 0.9.5(#67)'
  do
    if subject_ok "$bad_subject" '0.9.5'; then
      printf 'FAIL: accepted subject: %s\n' "$bad_subject" >&2
      exit 1
    fi
  done

  mkdir -p "$WORK/src/content/release-notes"
  printf 'Note line' > "$WORK/src/content/release-notes/1.2.3.md"
  (
    cd "$WORK"
    assemble_body 1.2.3 $'### Bug Fixes\n* same ([c1](https://example.com/c))\n* same ([#4](https://example.com/i)) ([c1](https://example.com/c))'
  ) >"$WORK/out"
  want=$(printf 'Note line\n\n%s\n\n### Bug Fixes\n* same ([#4](https://example.com/i)) ([c1](https://example.com/c))\n%s\n' "$PARAGRAPH" "$MARKER"; printf x)
  want=${want%x}
  got=$(cat "$WORK/out"; printf x)
  got=${got%x}
  if [[ "$got" != "$want" ]]; then
    printf 'FAIL: assemble mismatch\n' >&2
    diff -u <(printf '%s' "$want") <(printf '%s' "$got") >&2 || true
    exit 1
  fi

  if ! body_has_marker "hello ${MARKER}"; then
    printf 'FAIL: marker not detected\n' >&2
    exit 1
  fi
  if body_has_marker 'no marker here'; then
    printf 'FAIL: marker false positive\n' >&2
    exit 1
  fi

  python3 - "$WORK" <<'PY'
import json
import pathlib
import sys

fx = pathlib.Path(sys.argv[1])
a, b = "a" * 64, "b" * 64


def asset(name, digest, n):
    return {
        "name": name,
        "digest": digest,
        "apiUrl": "https://api.github.com/repos/o/r/releases/assets/%d" % n,
        "url": "https://github.com/o/r/releases/download/v0.9.5/" + name,
    }


assets = [
    asset("coco-voice.dmg", "sha256:" + a, 111),
    asset("latest.json", "sha256:" + b, 222),
    asset("SHA256SUMS", None, 333),
]
(fx / "assets.json").write_text(json.dumps({"assets": assets}))
null_assets = json.loads(json.dumps(assets))
null_assets[0]["digest"] = None
(fx / "assets-null.json").write_text(json.dumps({"assets": null_assets}))
bad_assets = json.loads(json.dumps(assets))
bad_assets[0]["digest"] = "sha256:nope"
(fx / "assets-bad.json").write_text(json.dumps({"assets": bad_assets}))
(fx / "SHA256SUMS").write_text("%s  coco-voice.dmg\n%s  latest.json\n" % (a, b))
(fx / "SHA256SUMS-bad").write_text("%s  coco-voice.dmg\n%s  latest.json\n" % ("d" * 64, b))
(fx / "SHA256SUMS-missing").write_text("%s  latest.json\n" % b)
(fx / "SHA256SUMS-extra").write_text(
    "%s  coco-voice.dmg\n%s  latest.json\n%s  assets/branding/logo.png\n" % (a, b, "c" * 64)
)
download = "https://github.com/o/r/releases/download/v0.9.5/coco-voice.dmg"
api = "https://api.github.com/repos/o/r/releases/assets/111"
(fx / "latest.json").write_text(json.dumps({
    "version": "0.9.5",
    "platforms": {"darwin-aarch64": {"url": download}, "darwin-x86_64": {"url": api}},
}))
(fx / "latest-version.json").write_text(json.dumps({
    "version": "0.9.4",
    "platforms": {"darwin-aarch64": {"url": download}},
}))
(fx / "latest-unknown.json").write_text(json.dumps({
    "version": "0.9.5",
    "platforms": {
        "darwin-aarch64": {"url": "https://api.github.com/repos/o/r/releases/assets/999"},
    },
}))
(fx / "latest-empty.json").write_text(json.dumps({"version": "0.9.5", "platforms": {}}))
PY

  assert_ok() {
    local status=0 err
    err=$(check_release_complete "$@" 2>&1) || status=$?
    if [[ "$status" -ne 0 || -n "$err" ]]; then
      printf 'FAIL: expected pass (%s): %s\n' "$status" "$err" >&2
      exit 1
    fi
  }
  assert_fail() {
    local needle=$1
    shift
    local status=0 err
    err=$(check_release_complete "$@" 2>&1) || status=$?
    if [[ "$status" -eq 0 || "$err" != *"$needle"* ]]; then
      printf 'FAIL: expected %s: %s\n' "$needle" "$err" >&2
      exit 1
    fi
  }
  assert_ok "$WORK/assets.json" "$WORK/SHA256SUMS" "$WORK/latest.json" 0.9.5
  assert_ok "$WORK/assets.json" "$WORK/SHA256SUMS-extra" "$WORK/latest.json" 0.9.5
  assert_fail "does not match SHA256SUMS" "$WORK/assets.json" "$WORK/SHA256SUMS-bad" "$WORK/latest.json" 0.9.5
  assert_fail "missing from SHA256SUMS" "$WORK/assets.json" "$WORK/SHA256SUMS-missing" "$WORK/latest.json" 0.9.5
  assert_fail "no digest in gh output" "$WORK/assets-null.json" "$WORK/SHA256SUMS" "$WORK/latest.json" 0.9.5
  assert_fail "malformed digest" "$WORK/assets-bad.json" "$WORK/SHA256SUMS" "$WORK/latest.json" 0.9.5
  assert_fail "does not equal" "$WORK/assets.json" "$WORK/SHA256SUMS" "$WORK/latest-version.json" 0.9.5
  assert_fail "does not reference an asset of this release" "$WORK/assets.json" "$WORK/SHA256SUMS" "$WORK/latest-unknown.json" 0.9.5
  assert_fail "platforms is empty" "$WORK/assets.json" "$WORK/SHA256SUMS" "$WORK/latest-empty.json" 0.9.5

  printf 'release-finalize self-test: ok\n'
}

main() {
  local TAG=""
  local VERIFY=0
  local DRY=0
  local SELF=0
  local VERSION=""
  local notes_file=""
  local latest_flag=""
  BODY=""

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --tag)
        if [[ $# -lt 2 || -z "${2:-}" ]]; then
          usage >&2
          exit 1
        fi
        TAG=$2
        shift 2
        ;;
      --verify-release-commit)
        VERIFY=1
        shift
        ;;
      --dry-run)
        DRY=1
        shift
        ;;
      --self-test)
        SELF=1
        shift
        ;;
      -h | --help)
        usage
        exit 0
        ;;
      *)
        printf 'unknown argument: %s\n' "$1" >&2
        usage >&2
        exit 1
        ;;
    esac
  done

  WORK=$(mktemp -d)

  if [[ "$SELF" -eq 1 ]]; then
    self_test
    exit 0
  fi
  if [[ -z "$TAG" ]]; then
    usage >&2
    exit 1
  fi
  if ! tag_ok "$TAG"; then
    printf 'refusing tag %s: expected vX.Y.Z\n' "$TAG" >&2
    exit 1
  fi
  VERSION=$(version_of "$TAG")

  if [[ "$VERIFY" -eq 1 ]]; then
    load_verified_body "$TAG" "$VERSION"
  else
    load_body "$TAG"
  fi

  warn_notes "$VERSION"
  latest_flag=$(resolve_latest_flag "$TAG" "$VERIFY")

  if body_has_marker "$BODY"; then
    apply_edit "$TAG" "" "$latest_flag"
    return 0
  fi

  notes_file="${WORK}/notes.md"
  assemble_body "$VERSION" "$BODY" >"$notes_file"
  apply_edit "$TAG" "$notes_file" "$latest_flag"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
