#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

fail() {
    echo "mark-release: $*" >&2
    exit 1
}

count_fixed() {
    awk -v text="$2" '
        { line = $0; while ((at = index(line, text))) { count++; line = substr(line, at + length(text)) } }
        END { print count + 0 }
    ' "$1"
}

[[ "$(git branch --show-current)" == main ]] || fail "run from the main branch"
[[ -z "$(git status --porcelain)" ]] || fail "the worktree must be completely clean"
[[ -n "$(git config --get user.signingkey || true)" ]] || fail "configure Git user.signingkey first"

current=$(sed -n 's/^version = "\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)"$/\1/p' Cargo.toml)
[[ "$current" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || fail "Cargo.toml must contain one stable SemVer package version"
current_tag="v$current"

git rev-parse --verify --quiet "refs/tags/$current_tag^{commit}" >/dev/null || fail "$current_tag does not exist"
git merge-base --is-ancestor "$current_tag" HEAD || fail "$current_tag is not an ancestor of HEAD"

old_status="$current is release-ready; publication and hosted verification are tracked under"
new_status="$current is released for Linux and macOS. Release verification is tracked under"
[[ "$(count_fixed README.md "$old_status")" == 1 ]] || fail "README.md does not contain exactly one release-ready status for $current"

read -r -p "Confirm $current_tag was published and hosted verification completed [y/N] " answer
[[ "$answer" == y || "$answer" == Y ]] || fail "cancelled without changes"

sed -i "s/$old_status/$new_status/" README.md
git diff --check
git add README.md
git commit -S -m "mark $current_tag released"
echo "Marked $current_tag released. Push main, then run ./scripts/release.sh."
