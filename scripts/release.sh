#!/usr/bin/env bash
# Cuts a release: bumps the version everywhere, commits, tags and pushes.
# The tag triggers .github/workflows/release.yml, which builds the installers.
#
# Usage: scripts/release.sh 1.0.0        (or 1.0.0-rc.1 for a pre-release)
set -euo pipefail
cd "$(dirname "$0")/.."

version="${1:-}"
version="${version#v}"
if [[ -z "$version" ]]; then
  echo "Usage: $0 <version>   e.g. $0 1.0.0" >&2
  exit 1
fi
tag="v$version"

branch=$(git rev-parse --abbrev-ref HEAD)
if [[ "$branch" != "main" ]]; then
  echo "Releases are cut from main, you are on '$branch'." >&2
  exit 1
fi
if [[ -n "$(git status --porcelain)" ]]; then
  echo "Working tree is not clean; commit or stash your changes first." >&2
  exit 1
fi
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  echo "Tag $tag already exists." >&2
  exit 1
fi

git pull --ff-only

node scripts/version.mjs set "$version"
node scripts/version.mjs check "$tag"

git add Cargo.toml Cargo.lock package.json server/package.json packages/api-contract/package.json
if git diff --cached --quiet; then
  echo "Version is already $version; tagging the current commit."
else
  git commit -m "chore: release $version"
fi

git tag -a "$tag" -m "Zeima $tag"
git push origin main "$tag"

echo "Pushed $tag. Follow the build in the repository's Actions tab."
