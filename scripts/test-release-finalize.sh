#!/usr/bin/env bash
# Offline checks for scripts/release-finalize.sh. No network and no gh.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
script="$root/scripts/release-finalize.sh"

bash "$script" --self-test

status=0
err=$(bash "$script" --tag 'not-a-tag' --verify-release-commit --dry-run 2>&1 >/dev/null) || status=$?
if [[ "$status" -eq 0 ]]; then
  printf 'FAIL: invalid tag exited 0\n' >&2
  exit 1
fi
if [[ "$err" != 'refusing tag not-a-tag: expected vX.Y.Z' ]]; then
  printf 'FAIL: invalid tag message: %s\n' "$err" >&2
  exit 1
fi

# shellcheck disable=SC1090
source "$script"

# First vX.Y.Z in a newest-first list. rc tags are skipped; a later higher tag must not win.
got=$(printf '%s\n' 'v2.0.0-rc1' 'v0.9.0' 'v1.2.3' | pick_latest_tag)
if [[ "$got" != v0.9.0 ]]; then
  printf 'FAIL: pick_latest_tag -> %s\n' "$got" >&2
  exit 1
fi
if printf '%s\n' 'v1.0.0-rc1' 'not-a-tag' | pick_latest_tag >/dev/null; then
  printf 'FAIL: pick_latest_tag matched a non-release\n' >&2
  exit 1
fi
err=$(latest_arg v1.2.3 v1.9.0 2>&1 >/dev/null)
got=$(latest_arg v1.2.3 v1.9.0 2>/dev/null)
if [[ "$got" != '--latest=false' || "$err" != 'note: v1.2.3 is not the highest vX.Y.Z tag (v1.9.0); passing --latest=false' ]]; then
  printf 'FAIL: older latest_arg: %s (%s)\n' "$got" "$err" >&2
  exit 1
fi
if [[ "$(latest_arg v1.9.0 v1.9.0 2>/dev/null)" != '--latest' ]]; then
  printf 'FAIL: highest tag should pass --latest\n' >&2
  exit 1
fi

tmp=$(mktemp -d)
note='warning: no release notes at src/content/release-notes/1.2.3.md; publishing without them'
err=$(cd "$tmp" && warn_notes 1.2.3 2>&1)
mkdir -p "$tmp/src/content/release-notes"
printf '' > "$tmp/src/content/release-notes/1.2.3.md"
empty=$(cd "$tmp" && warn_notes 1.2.3 2>&1)
printf 'shipped\n' > "$tmp/src/content/release-notes/1.2.3.md"
present=$(cd "$tmp" && warn_notes 1.2.3 2>&1)
rm -rf "$tmp"
if [[ "$err" != "$note" || "$empty" != "$note" || -n "$present" ]]; then
  printf 'FAIL: notes warnings: missing=%s empty=%s present=%s\n' "$err" "$empty" "$present" >&2
  exit 1
fi

fixture=$(
  cat <<'EOF'
### Bug Fixes
* don't drop "quotes" in the user's clipboard ([#45](https://github.com/o/r/issues/45)) ([abc1234](https://github.com/o/r/commit/abc1234))
* don't drop "quotes" in the user's clipboard ([def5678](https://github.com/o/r/commit/def5678))
* don't drop quotes in the user's clipboard ([ghi9999](https://github.com/o/r/commit/ghi9999))
* keep the user's note ([jjj0000](https://github.com/o/r/commit/jjj0000))
* keep the users note ([kkk0000](https://github.com/o/r/commit/kkk0000))
* build PR code without repository secrets or a write token ([aaa1111](https://github.com/o/r/commit/aaa1111))
* build PR code without repository secrets or a write token ([bbb2222](https://github.com/o/r/commit/bbb2222))
* build PR code without repository secrets or a write token ([#56](https://github.com/o/r/issues/56)) ([ccc3333](https://github.com/o/r/commit/ccc3333))
* do not wipe a non-text clipboard when pasting ([28c4187](u)), closes [#9](u)
* do not wipe a non-text clipboard when pasting ([#32](u)) ([28c4187](u))
* unique bullet
- dash item ([ccc](https://github.com/o/r/commit/ccc))
- dash item ([#2](https://github.com/o/r/issues/2)) ([ddd](https://github.com/o/r/commit/ddd))
not a list
not a list
EOF
)

expected=$(
  cat <<'EOF'
### Bug Fixes
* don't drop "quotes" in the user's clipboard ([#45](https://github.com/o/r/issues/45)) ([abc1234](https://github.com/o/r/commit/abc1234))
* don't drop quotes in the user's clipboard ([ghi9999](https://github.com/o/r/commit/ghi9999))
* keep the user's note ([jjj0000](https://github.com/o/r/commit/jjj0000))
* keep the users note ([kkk0000](https://github.com/o/r/commit/kkk0000))
* build PR code without repository secrets or a write token ([#56](https://github.com/o/r/issues/56)) ([ccc3333](https://github.com/o/r/commit/ccc3333))
* do not wipe a non-text clipboard when pasting ([#32](u)) ([28c4187](u))
* unique bullet
- dash item ([#2](https://github.com/o/r/issues/2)) ([ddd](https://github.com/o/r/commit/ddd))
not a list
not a list
EOF
)

got=$(printf '%s\n' "$fixture" | dedupe_changelog; printf x)
got=${got%x}
want=$(printf '%s\n' "$expected"; printf x)
want=${want%x}

if [[ "$got" != "$want" ]]; then
  printf 'FAIL: fixture dedupe mismatch\n' >&2
  diff -u <(printf '%s' "$want") <(printf '%s' "$got") >&2 || true
  exit 1
fi

if grep -nE '(^|[^[:alnum:]_])gh[[:space:]]+api([^[:alnum:]_]|$)' "$script" >/dev/null; then
  printf 'FAIL: %s calls gh api\n' "$script" >&2
  exit 1
fi

printf 'test-release-finalize: ok\n'
