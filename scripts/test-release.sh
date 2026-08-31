#!/usr/bin/env bash
set -euo pipefail

source_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
test_root=$(mktemp -d)
trap 'rm -rf -- "$test_root"' EXIT

git clone --bare --quiet "$source_root" "$test_root/remote.git"
git clone --quiet "$test_root/remote.git" "$test_root/repo"
mkdir -p "$test_root/repo/scripts"
cp "$source_root/scripts/release.sh" "$test_root/repo/scripts/release.sh"
cp "$source_root/README.md" "$test_root/repo/README.md"
cp "$source_root/docs/release.md" "$test_root/repo/docs/release.md"
ssh-keygen -q -t ed25519 -N "" -f "$test_root/signing-key"

cd "$test_root/repo"
git config user.name "Release smoke test"
git config user.email "release-test@example.invalid"
git config gpg.format ssh
git config user.signingkey "$test_root/signing-key"
before=$(sed -n 's/^version = "\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)"$/\1/p' Cargo.toml)
sed -i "s/$before is release-ready; publication and hosted verification are tracked under/$before is released for Linux and macOS. Release verification is tracked under/" README.md
git add scripts/release.sh README.md docs/release.md
git commit --quiet --allow-empty -m "release smoke-test change"

remote_before=$(git --git-dir="$test_root/remote.git" rev-parse refs/heads/main)
if printf 'n\n' | ./scripts/release.sh; then
    echo "release cancellation unexpectedly succeeded" >&2
    exit 1
fi
[[ -z "$(git status --porcelain)" ]]
[[ "$(git --git-dir="$test_root/remote.git" rev-parse refs/heads/main)" == "$remote_before" ]]
printf 'y\n' | ./scripts/release.sh
after=$(sed -n 's/^version = "\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)"$/\1/p' Cargo.toml)

[[ "$before" != "$after" ]]
[[ "$(git log -1 --format=%s)" == "prepare v$after release" ]]
git rev-parse --verify "refs/tags/v$after^{tag}" >/dev/null
git cat-file commit HEAD | grep -q "BEGIN SSH SIGNATURE"
git cat-file tag "v$after" | grep -q "BEGIN SSH SIGNATURE"
grep -Fq "$after is release-ready" README.md
grep -Fq -- "cargo install --locked --force --git https://github.com/AFwcxx/bit-mail --tag v$after bit-mail" README.md
! grep -Fq "$before" docs/release.md docs/user-manual.md
[[ -z "$(git status --porcelain)" ]]
[[ "$(git --git-dir="$test_root/remote.git" rev-parse refs/heads/main)" == "$(git rev-parse HEAD)" ]]
[[ "$(git --git-dir="$test_root/remote.git" rev-parse "refs/tags/v$after")" == "$(git rev-parse "refs/tags/v$after")" ]]

echo "release smoke test passed"
