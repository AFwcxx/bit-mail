#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

fail() {
    echo "release: $*" >&2
    exit 1
}

count_fixed() {
    awk -v text="$2" '
        { line = $0; while ((at = index(line, text))) { count++; line = substr(line, at + length(text)) } }
        END { print count + 0 }
    ' "$1"
}

require_count() {
    local count
    count=$(count_fixed "$1" "$2")
    [[ "$count" == "$3" ]] || fail "$1 changed: expected $3 occurrence(s) of: $2"
}

replace_exact() {
    local file=$1 old=$2 new=$3
    awk -v old="$old" -v new="$new" '
        {
            while ((at = index($0, old))) {
                $0 = substr($0, 1, at - 1) new substr($0, at + length(old))
            }
            print
        }
    ' "$file" > "$temporary_file"
    cp "$temporary_file" "$file"
}

[[ "$(git branch --show-current)" == main ]] || fail "run from the main branch"
[[ -z "$(git status --porcelain)" ]] || fail "the worktree must be completely clean"
[[ -n "$(git config --get user.signingkey || true)" ]] || fail "configure Git user.signingkey first"
cargo +1.88.0 --version >/dev/null 2>&1 || fail "install the Rust 1.88.0 toolchain first"

current=$(sed -n 's/^version = "\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)"$/\1/p' Cargo.toml)
[[ "$current" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || fail "Cargo.toml must contain one stable SemVer package version"
next="${BASH_REMATCH[1]}.${BASH_REMATCH[2]}.$((10#${BASH_REMATCH[3]} + 1))"
current_tag="v$current"
next_tag="v$next"
base_head=$(git rev-parse HEAD)

git rev-parse --verify --quiet "refs/tags/$current_tag^{commit}" >/dev/null || fail "$current_tag does not exist"
git merge-base --is-ancestor "$current_tag" HEAD || fail "$current_tag is not an ancestor of HEAD"
[[ "$base_head" != "$(git rev-parse "$current_tag^{commit}")" ]] || fail "there are no commits since $current_tag"
! git show-ref --verify --quiet "refs/tags/$next_tag" || fail "$next_tag already exists"

old_status="$current is released for Linux and macOS. Release verification is tracked under"
new_status="$next is release-ready; publication and hosted verification are tracked under"
require_count Cargo.toml "version = \"$current\"" 1
require_count README.md "$old_status" 1
require_count README.md "--tag v$current" 1
require_count docs/release.md "$current" 4
require_count docs/user-manual.md "$current" 1

echo "Commits since $current_tag:"
git log --oneline "$current_tag"..HEAD
printf '\nPatch release: %s -> %s\nFiles: Cargo.toml Cargo.lock README.md docs/release.md docs/user-manual.md\n' "$current" "$next"
read -r -p "Confirm patch semantics and documentation readiness [y/N] " answer
[[ "$answer" == y || "$answer" == Y ]] || fail "cancelled without changes"

temporary_file=$(mktemp)
trap 'rm -f "$temporary_file"' EXIT
replace_exact Cargo.toml "version = \"$current\"" "version = \"$next\""
replace_exact README.md "$old_status" "$new_status"
replace_exact README.md "--tag v$current" "--tag v$next"
replace_exact docs/release.md "$current" "$next"
replace_exact docs/user-manual.md "$current" "$next"

cargo +1.88.0 check --quiet
[[ "$(git diff --numstat -- Cargo.lock)" == $'1\t1\tCargo.lock' ]] || fail "Cargo changed more than the root package version in Cargo.lock"
awk -v version="$next" '
    $0 == "name = \"bit-mail\"" { package = 1; next }
    package && $0 == "version = \"" version "\"" { found = 1; exit }
    package && $0 == "[[package]]" { exit }
    END { exit !found }
' Cargo.lock || fail "Cargo.lock does not contain bit-mail $next"

expected_files=$'Cargo.lock\nCargo.toml\nREADME.md\ndocs/release.md\ndocs/user-manual.md'
[[ "$(git diff --name-only | LC_ALL=C sort)" == "$expected_files" ]] || fail "unexpected files changed during release preparation"
git diff --check
cargo +1.88.0 test --locked --all-features

git add Cargo.toml Cargo.lock README.md docs/release.md docs/user-manual.md
git commit -S -m "prepare $next_tag release"
if ! git tag -s -m "bit-mail $next_tag" "$next_tag"; then
    git reset --mixed "$base_head"
    fail "tag signing failed; the release commit was undone and its edits were kept"
fi

git push --atomic origin main "$next_tag"
echo "Published signed commit and tag $next_tag; verify the hosted release, then mark README as released."
