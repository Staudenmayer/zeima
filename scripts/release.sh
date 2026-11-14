#!/usr/bin/env bash
# Cuts a release: checks everything a release needs, bumps the version
# everywhere, commits, tags and pushes. The tag triggers
# .github/workflows/release.yml, which builds the installers and publishes
# the release and the updater's latest.json (see docs/releasing.md).
#
# Usage: scripts/release.sh <version> [options]
#   scripts/release.sh 1.2.3            a release
#   scripts/release.sh 1.2.3-rc.1       a pre-release
#
# Options:
#   --dry-run      run every check, change nothing (no commit, tag or push)
#   --skip-tests   do not run the local test suites (CI results are still checked)
#   --skip-ci      do not require a green CI run for this commit
#   --yes          do not ask before tagging and pushing
#   --watch        after pushing, follow the release build until it ends
set -euo pipefail
cd "$(dirname "$0")/.."

# ------------------------------------------------------------------ arguments
version=""
dry_run=0 skip_tests=0 skip_ci=0 assume_yes=0 watch=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=1 ;;
    --skip-tests) skip_tests=1 ;;
    --skip-ci) skip_ci=1 ;;
    --yes | -y) assume_yes=1 ;;
    --watch) watch=1 ;;
    -h | --help) sed -n '2,19p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "Unknown option: $arg (see --help)" >&2; exit 1 ;;
    *) version="${arg#v}" ;;
  esac
done
if [[ -z "$version" ]]; then
  echo "Usage: $0 <version> [--dry-run] [--skip-tests] [--skip-ci] [--yes] [--watch]   e.g. $0 1.2.3" >&2
  exit 1
fi
tag="v$version"

# ------------------------------------------------------------------- helpers
problems=0
warnings=0
step() { printf '\n==> %s\n' "$*"; }
ok() { printf '  ok    %s\n' "$*"; }
warn() { printf '  WARN  %s\n' "$*"; warnings=$((warnings + 1)); }
bad() { printf '  FAIL  %s\n' "$*" >&2; problems=$((problems + 1)); }
die() { printf '\n%s\n' "$*" >&2; exit 1; }

# ------------------------------------------------------------ 1. the checkout
step "The checkout"
if [[ ! "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$ ]]; then
  die "'$version' is not a version like 1.2.3 or 1.2.3-rc.1."
fi
branch=$(git rev-parse --abbrev-ref HEAD)
if [[ "$branch" == "main" ]]; then ok "on main"; else bad "releases are cut from main, you are on '$branch'"; fi
if [[ -z "$(git status --porcelain)" ]]; then ok "working tree is clean"; else bad "working tree is not clean; commit or stash your changes first"; fi
git fetch --quiet --tags origin || warn "could not reach origin (offline?)"
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then bad "tag $tag already exists"; else ok "tag $tag is free"; fi
if git rev-parse -q --verify "origin/main" >/dev/null; then
  behind=$(git rev-list --count HEAD..origin/main)
  ahead=$(git rev-list --count origin/main..HEAD)
  if ((behind > 0)); then bad "main is $behind commit(s) behind origin/main; run: git pull --ff-only"; fi
  if ((ahead > 0)); then bad "main has $ahead commit(s) that are not pushed yet; push them and let CI run first"; fi
  if ((behind == 0 && ahead == 0)); then ok "main is the same as origin/main"; fi
fi
current=$(node -e "console.log(JSON.parse(require('fs').readFileSync('package.json','utf8')).version)")
ok "current version $current → $version"
if [[ "$current" == "$version" ]]; then warn "the version is already $version; the current commit will be tagged"; fi

# ----------------------------------------------------- 2. GitHub: CI, secrets
step "GitHub (CI result, secrets)"
if command -v gh >/dev/null && gh auth status >/dev/null 2>&1; then
  sha=$(git rev-parse HEAD)
  if ((skip_ci)); then
    warn "CI result not checked (--skip-ci)"
  else
    run=$(gh run list --workflow ci.yml --commit "$sha" --limit 1 --json status,conclusion,url --jq '.[0] | "\(.status) \(.conclusion) \(.url)"' 2>/dev/null || true)
    case "$run" in
      "completed success "*) ok "CI passed for this commit" ;;
      "") bad "no CI run found for this commit ($sha); push it and wait for CI (or use --skip-ci)" ;;
      "completed "*) bad "CI did not pass for this commit: ${run##* }" ;;
      *) bad "CI is still running for this commit: ${run##* }" ;;
    esac
  fi
  secrets=$(gh secret list --json name --jq '.[].name' 2>/dev/null || true)
  if grep -qx TAURI_SIGNING_PRIVATE_KEY <<<"$secrets"; then
    ok "updater signing key secret is set (TAURI_SIGNING_PRIVATE_KEY)"
  else
    bad "the secret TAURI_SIGNING_PRIVATE_KEY is not set; without it installed apps cannot update (docs/releasing.md)"
  fi
  if ! grep -qx TAURI_SIGNING_PRIVATE_KEY_PASSWORD <<<"$secrets"; then
    warn "TAURI_SIGNING_PRIVATE_KEY_PASSWORD is not set (fine if the key has no password)"
  fi
  vars=$(gh variable list --json name --jq '.[].name' 2>/dev/null || true)
  if grep -qx ZEIMA_ENTITLEMENT_KEYS <<<"$vars"; then
    ok "licence issuer keys variable is set (ZEIMA_ENTITLEMENT_KEYS)"
  else
    warn "the variable ZEIMA_ENTITLEMENT_KEYS is not set: this build cannot verify paid plans and behaves as Free"
  fi
  for s in APPLE_CERTIFICATE APPLE_ID APPLE_TEAM_ID; do
    grep -qx "$s" <<<"$secrets" || { warn "macOS signing secrets are missing: the macOS installer will be unsigned"; break; }
  done
else
  warn "gh (GitHub CLI) is missing or not logged in: CI result and secrets could not be checked"
fi

# ------------------------------------------------------------- 3. local tests
if ((skip_tests)); then
  step "Local tests"
  warn "skipped (--skip-tests)"
else
  step "Local tests (as in CI)"
  run_check() {
    local name="$1"
    shift
    if "$@" >/tmp/zeima-release-check.log 2>&1; then ok "$name"; else bad "$name (output: tail /tmp/zeima-release-check.log)"; tail -n 15 /tmp/zeima-release-check.log >&2; fi
  }
  run_check "versions agree" node scripts/version.mjs check
  run_check "rust: cargo fmt" cargo fmt --all --check
  run_check "rust: clippy" cargo clippy --workspace --all-targets --all-features -- -D warnings
  run_check "rust: tests" cargo test --workspace --all-features
  run_check "app: type check" bun run typecheck
  run_check "app: tests" bun run test
  run_check "server: lint" bun run --cwd server lint
  run_check "server: type check" bun run --cwd server typecheck
  run_check "server: tests" bun run --cwd server test
  if [[ -n "$(git status --porcelain)" ]]; then
    bad "the checks changed files (generated bindings or contract out of date): $(git status --porcelain | head -5 | tr '\n' ' ')"
  fi
fi

# ----------------------------------------------------------------- the verdict
step "Result"
if ((problems > 0)); then
  die "$problems problem(s) stop the release; fix them and run this again ($warnings warning(s))."
fi
ok "everything needed is in place ($warnings warning(s))"
if ((dry_run)); then
  echo
  echo "Dry run: nothing was changed. Without --dry-run this would set version $version, commit, tag $tag and push."
  exit 0
fi
if ((!assume_yes)); then
  echo
  read -r -p "Set version $version, commit, tag $tag and push to origin? [y/N] " answer
  [[ "$answer" == [yY]* ]] || die "Stopped; nothing was changed."
fi

# --------------------------------------------------------- 4. version, tag, push
step "Version and tag"
node scripts/version.mjs set "$version"
node scripts/version.mjs check "$tag"
git add -A # the tree was clean: this is only the version files and the lockfiles
if git diff --cached --quiet; then
  echo "  Version is already $version; tagging the current commit."
else
  git commit -q -m "chore: release $version"
  ok "committed the version change"
fi
git tag -a "$tag" -m "Zeima $tag"
git push origin main "$tag"
ok "pushed main and $tag"

repo_url=$(gh repo view --json url --jq .url 2>/dev/null || true)
echo
echo "Pushed $tag. The build starts now${repo_url:+: $repo_url/actions}"
echo "When it is done the release is at ${repo_url:+$repo_url/releases/tag/$tag}"

# ------------------------------------------------------------ 5. follow the build
if ((watch)); then
  step "Release build"
  command -v gh >/dev/null || die "--watch needs the GitHub CLI (gh)."
  sleep 8 # the run takes a moment to appear
  id=$(gh run list --workflow release.yml --branch "$tag" --limit 1 --json databaseId --jq '.[0].databaseId' 2>/dev/null || true)
  if [[ -z "$id" ]]; then
    warn "could not find the release run yet; look in the Actions tab"
  elif gh run watch "$id" --exit-status; then
    ok "release build finished"
    gh release view "$tag" --json assets --jq '.assets[].name' 2>/dev/null | sed 's/^/        /' || true
  else
    die "The release build failed: ${repo_url:+$repo_url/actions/runs/$id}"
  fi
fi
