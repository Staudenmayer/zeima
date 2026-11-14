# Releasing Zeima

## Cutting a release

```sh
scripts/release.sh 1.2.3            # or 1.2.3-rc.1 for a pre-release
scripts/release.sh 1.2.3 --dry-run  # run every check, change nothing
```

The script checks first and stops on any problem: you are on `main`, the tree
is clean and pushed, the tag is free, CI passed for this commit, the updater
signing key secret (`TAURI_SIGNING_PRIVATE_KEY`) is set, and the local test
suites pass (Rust fmt, clippy and tests; app and server type check, lint and
tests; no generated file changed). Missing licence keys
(`ZEIMA_ENTITLEMENT_KEYS`) and missing macOS signing secrets are warnings. It
then asks, sets the version in every file (`node scripts/version.mjs set`),
commits, tags `v1.2.3` and pushes. Options: `--skip-tests`, `--skip-ci`, `--yes`
(no question) and `--watch` (follow the release build and list the attached
files when it ends).

The tag starts `.github/workflows/release.yml`: it checks that the tag matches
the app version, builds the installers for Linux, Windows and macOS (Apple
silicon), attaches them and the example addons to a GitHub release, and
publishes `latest.json` for the updater. Tags with a suffix (`v1.2.3-rc.1`)
are marked as pre-releases.

By hand (what the script does): `node scripts/version.mjs set 1.2.3`,
`git commit -am "chore: release 1.2.3"`, `git tag v1.2.3 && git push --follow-tags`.

## The auto-updater

The app checks `latest.json` of the newest release (endpoint in
`src-tauri/tauri.conf.json`) shortly after it starts and from Settings →
About. An update is installed only if its installer is signed with the
**updater key** whose public half is in `tauri.conf.json` (`plugins.updater.pubkey`).

- The private key is **not in the repository**. The key in use was generated
  with `bunx tauri signer generate -w ~/.tauri/zeima-updater.key` (no
  password). Put its contents into the GitHub secret `TAURI_SIGNING_PRIVATE_KEY`
  (and the password, if you set one, into `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`).
- **If this key is lost, installed apps can no longer be updated** (they only
  trust the public key they contain); users would have to install a new
  version by hand. Keep a backup. To rotate: generate a new pair, release one
  version signed with the old key that contains the new public key, then
  switch.
- `src-tauri/tauri.release.conf.json` turns on the updater artifacts; it is
  passed only by the release workflow, so local builds do not need the key.

## Code signing (installers)

Without signing, macOS shows "unidentified developer" and Windows
SmartScreen warns. Signing needs accounts and certificates that are not part of
this repository:

| Platform | What you need | Secrets (release workflow reads them) |
|---|---|---|
| macOS | Apple Developer Program, a "Developer ID Application" certificate exported as `.p12` (base64), an app-specific password | `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` |
| Windows | An OV/EV code-signing certificate or Azure Trusted Signing; set `bundle.windows.signCommand` in `tauri.release.conf.json` | depends on the provider |
| Linux | nothing (the updater signature is enough) | |

The macOS secrets are already wired in the workflow; unset secrets leave the
build unsigned. See Tauri's guides "Code signing macOS / Windows".

## Entitlement keys

Builds trust the licence issuer keys given in `ZEIMA_ENTITLEMENT_KEYS` at build
time (raw Ed25519 public keys, base64url, comma-separated); debug builds also
trust the public development key. A release build with no key cannot verify
plans, so it behaves as Free. Set it as a repository variable and pass it to
the build step (see `server/README.md`, "Plans and sync").
