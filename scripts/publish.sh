#!/usr/bin/env bash
#
# Publish the current version of vtracer to every surface. Idempotent: each
# step is skipped if it is already done, so it is safe to re-run after a
# partial or interrupted release.
#
# Surfaces:
#   - git       push master, push tag  (the tag push triggers the PyPI wheels)
#   - crates.io vtracer, then vtracer-cli; and vtracer-bench (independent)
#   - npm       @visioncortex/vtracer
#   - GitHub    a release from the tag, marked latest (triggers the binaries)
#
# Prerequisites (all in your own shell — this cannot run in a sandbox):
#   cargo login   •   npm login   •   gh auth login
#   version already bumped + committed on master.
#
# Usage:  ./scripts/publish.sh          # confirm, then publish what's missing
#         ./scripts/publish.sh --dry    # checks + build/test only, no publish
#
set -euo pipefail
cd "$(dirname "$0")/.."

DRY=0
[ "${1:-}" = "--dry" ] && DRY=1

VERSION=$(grep -m1 '^version = ' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')
TAG="$VERSION"

say()  { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
skip() { printf '   \033[2m· %s\033[0m\n' "$*"; }

# --- availability probes (used to skip already-done steps) -------------------
crate_published() { # crate, version
  # crates.io rejects requests without a User-Agent (403), and `curl -f` hides
  # that as an empty body — so the UA is mandatory or this never matches.
  curl -fsS -H "User-Agent: vtracer-publish (github.com/visioncortex/vtracer)" \
    "https://crates.io/api/v1/crates/$1/$2" 2>/dev/null | grep -q "\"num\":\"$2\""
}
npm_published() {   # pkg@version
  npm view "$1" version >/dev/null 2>&1
}
gh_release_exists() { gh release view "$1" >/dev/null 2>&1; }

say "Publishing vtracer $VERSION  (dry-run: $DRY)"

# --- preconditions -----------------------------------------------------------
say "Checking preconditions"
[ "$(git rev-parse --abbrev-ref HEAD)" = master ] || { echo "!! not on master"; exit 1; }
[ -z "$(git status --porcelain --untracked-files=no)" ] || { echo "!! tracked files have uncommitted changes — commit first"; exit 1; }
for tool in cargo npm gh curl; do command -v "$tool" >/dev/null || { echo "!! missing: $tool"; exit 1; }; done
# The version we publish comes from Cargo.toml at HEAD; the tag only marks the
# release commit for CI, so it need not be at HEAD (a later commit such as this
# script is fine). Guard only against the genuinely wrong case: a tag whose own
# commit carries a different version than the one we're about to publish.
if git rev-parse "refs/tags/$TAG" >/dev/null 2>&1; then
  tag_ver=$(git show "$TAG:Cargo.toml" 2>/dev/null | grep -m1 '^version = ' | sed -E 's/.*"([^"]+)".*/\1/')
  [ "$tag_ver" = "$VERSION" ] || { echo "!! tag $TAG marks version $tag_ver, but HEAD is $VERSION"; exit 1; }
  skip "tag $TAG already exists (marks $VERSION)"
fi

# --- build + test (always, even on --dry) ------------------------------------
say "Building + testing"
cargo test --workspace
cargo build --release -p vtracer-cli
( cd nodejs && npm run build )

if [ "$DRY" = 1 ]; then say "Dry run complete — checks passed, nothing published."; exit 0; fi

# --- confirm -----------------------------------------------------------------
printf '\nPublish vtracer %s (git, crates.io, npm, GitHub)? Steps already done are skipped. [y/N] ' "$VERSION"
read -r reply
[ "$reply" = y ] || [ "$reply" = Y ] || { echo "aborted."; exit 1; }

# --- 1. git: push master, then the tag (tag push triggers the PyPI wheels) ---
say "git: push master + tag"
git push origin master
git rev-parse "refs/tags/$TAG" >/dev/null 2>&1 || git tag "$TAG"
git push origin "$TAG"

# --- 2. crates.io: core first, then the CLI that depends on it ---------------
if crate_published vtracer "$VERSION"; then
  skip "crates.io vtracer $VERSION already published"
else
  say "crates.io: publishing vtracer"
  cargo publish -p vtracer
  printf '   waiting for the index'
  until crate_published vtracer "$VERSION"; do printf '.'; sleep 10; done; echo
fi
if crate_published vtracer-cli "$VERSION"; then
  skip "crates.io vtracer-cli $VERSION already published"
else
  say "crates.io: publishing vtracer-cli"
  cargo publish -p vtracer-cli
fi
# vtracer-bench depends only on registry crates (not vtracer), so order is free.
if crate_published vtracer-bench "$VERSION"; then
  skip "crates.io vtracer-bench $VERSION already published"
else
  say "crates.io: publishing vtracer-bench"
  cargo publish -p vtracer-bench
fi

# --- 3. npm ------------------------------------------------------------------
# Published to the default `latest` tag, matching the earlier alpha releases.
# For a prerelease you may prefer:  ( cd nodejs && npm publish --tag next )
if npm_published "@visioncortex/vtracer@$VERSION"; then
  skip "npm @visioncortex/vtracer@$VERSION already published"
else
  say "npm: publishing @visioncortex/vtracer"
  ( cd nodejs && npm publish )
fi

# --- 4. GitHub release (its creation triggers the binary build workflow) -----
if gh_release_exists "$TAG"; then
  skip "GitHub release $TAG already exists"
else
  say "GitHub: creating release $TAG"
  NOTES=$(awk -v h="## $VERSION" 'index($0,h)==1{f=1;next} /^## /&&f{exit} f' CHANGELOG.md)
  # --latest: mark as the latest release (gh would otherwise treat an -alpha
  # tag as a prerelease and not promote it).
  gh release create "$TAG" --latest --title "$TAG" --notes "${NOTES:-Release $VERSION}"
fi

say "Done. crates.io + npm up to date; PyPI wheels build from the tag; binaries build from the release."
