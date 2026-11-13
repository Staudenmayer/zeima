# Setting up keys, secrets and a server

What is already set up, what to do before shipping, and what can wait. Related
guides: [releasing.md](./releasing.md), [keycloak.md](./keycloak.md),
[deployment-cloud.md](./deployment-cloud.md) and `server/README.md`.

## Where things stand

| Item | State |
|---|---|
| Updater key (signs app updates) | Done: key pair `~/.tauri/zeima-updater-v2.key(.pub)` with a password; public key in `src-tauri/tauri.conf.json`; GitHub secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` set |
| Entitlement public key (app builds trust it) | Done: GitHub variable `ZEIMA_ENTITLEMENT_KEYS` set |
| Entitlement private key on a server | Needed only for a cloud server or for issuing licenses (below) |
| macOS / Windows code signing | Later |
| Google / Microsoft sign-in | Later |
| Billing (Lemon Squeezy) | Later |
| Cloud hosting | Later; the single-machine stack is `deploy/docker-compose.yml` |

Syncing works without any of the "later" items: start the API with
`DEFAULT_PLAN=pro` (development) or install a license (self-hosted).

## Run the app against a local server

1. Dev stack (PostgreSQL, Keycloak with the `zeima` realm, Mailpit):

   ```sh
   docker compose -f deploy/compose.dev.yml up -d
   ```

   After a change to `deploy/keycloak/realm-zeima.json`, either recreate it
   with `docker compose -f deploy/compose.dev.yml down -v` and `up -d`, or apply
   the change to the running Keycloak:

   ```sh
   KEYCLOAK_URL=http://localhost:8080 KEYCLOAK_PASSWORD=admin \
   ZEIMA_API_ADMIN_SECRET=dev-admin-secret ZEIMA_SMTP_HOST=mailpit ZEIMA_SMTP_PORT=1025 \
   deploy/keycloak/apply.sh
   ```

2. API, with the Pro plan for everyone (development only):

   ```sh
   DEFAULT_PLAN=pro bun run --cwd server dev
   ```

3. The app: `bun run tauri dev`.
4. Settings → Account: enter `localhost:3000` (the app uses `http://` for
   addresses on this machine), save, then "Sign in". In the browser use
   "Register" to create a user. The verification mail arrives in Mailpit at
   http://localhost:8025.

### Two instances on one machine (testing sync)

Set `ZEIMA_DATA_DIR` to a folder and the app runs as a separate device with its
own database, backups, addons, device id and keychain entries; it neither stops
nor is stopped by another instance. Debug builds load the dev server, so start
the normal instance with `bun run tauri dev` first, then in another terminal:

```sh
ZEIMA_DATA_DIR=/tmp/zeima-b ./target/debug/zeima
```

(Build it once with `cargo build -p zeima`; after code changes rebuild.) The
window title shows the folder name (`Zeima (zeima-b)`). Sign in to the same
account in both and use Sync now. Things to know:

- A test instance never changes the system's autostart setting.
- Global shortcuts are claimed by whichever instance starts first; the second
  logs "shortcuts not registered". Choose other shortcuts in its Settings.
- Delete the folder to start over with a fresh device.

An automated check of the same flow, without the app, is
`deploy/e2e/smoke.mjs` (see the comment at the top of the file).

## Updater key

The app installs an update only if it is signed with the key whose public half
is in `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`). The release
workflow signs with the private key from the GitHub secrets.

- **Keep the private key file and its password backed up**, separately (a
  password manager or an encrypted drive). If either is lost, installed apps
  can no longer be updated automatically.
- **Check that key, password and the public key in the config match** (run from
  the repository root):

  ```sh
  echo hello > /tmp/keytest.txt
  # The password comes from the prompt, or from TAURI_SIGNING_PRIVATE_KEY_PASSWORD
  # (put a space before the command so it stays out of your shell history).
  bunx tauri signer sign -f ~/.tauri/zeima-updater-v2.key /tmp/keytest.txt
  node scripts/verify-updater-key.mjs /tmp/keytest.txt.sig \
    "$(python3 -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['plugins']['updater']['pubkey'])")"
  ```

  A wrong password makes the first command fail; a different key makes the
  second print `FAIL: the signature was made by a different key`.
- **Generate a new key** (do this only before the first release that contains
  the updater; installed apps trust the key they shipped with):

  ```sh
  bunx tauri signer generate -w ~/.tauri/zeima-updater-v3.key     # prompts for a password
  ```

  Put the `.pub` file's contents into `tauri.conf.json`, then update the secrets:

  ```sh
  gh secret set TAURI_SIGNING_PRIVATE_KEY --repo Staudenmayer/zeima < ~/.tauri/zeima-updater-v3.key
  gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo Staudenmayer/zeima   # type the password at the prompt
  ```

  To rotate after users have a version with the updater, release one version
  signed with the old key that contains the new public key, then switch.

## Entitlement keys

Plans (Pro, Team) and license files are Ed25519-signed tokens (SPEC §2). The
app checks them offline against a public key built in.

- **Public key** → app builds, as the repository variable `ZEIMA_ENTITLEMENT_KEYS`
  (done). Builds made before it was set treat everyone as Free.
- **Private key** → whoever issues plans: the cloud server
  (`ENTITLEMENT_SIGNING_KEY`, from a secret store) or you, signing license
  files for self-hosted customers. Never put it in the repository or in an image.
- **Generate a pair** (run from anywhere; `--private-file` keeps the private key
  out of your terminal scrollback and refuses to overwrite a file):

  ```sh
  node scripts/generate-entitlement-key.mjs --private-file ~/.zeima-entitlement.key
  ```

  Then set the printed public key:

  ```sh
  gh variable set ZEIMA_ENTITLEMENT_KEYS --repo Staudenmayer/zeima --body "<public key>"
  ```

  If you generate a **new** pair, existing apps keep trusting the old public
  key until they are updated, so plans signed with the new private key reach only
  newer builds. Back up the private key.
- **Self-hosted servers** accept licenses you signed: give the server your public
  key as `ENTITLEMENT_PUBLIC_KEYS` (see `deploy/.env.example`) and install the
  license with `PUT /v1/license` (role `platform-admin`; see `server/README.md`).
- The development key in `packages/shared-fixtures/entitlement/dev-key.json` is
  public on purpose. Debug builds and non-production servers trust it; release
  builds do not.

## Later

- **Billing** (cloud only): create a Lemon Squeezy store, add the webhook
  `POST /v1/billing/webhook` and set `BILLING_STORE_URL`, `BILLING_WEBHOOK_SECRET`
  and `BILLING_PLANS` on the cloud server. Details in `server/README.md`.
- **Google / Microsoft sign-in**: register OAuth clients with the providers and add
  them in Keycloak. Steps in [keycloak.md](./keycloak.md).
- **Code signing**: macOS needs an Apple Developer ID certificate (secrets are
  already wired in the release workflow); Windows needs a certificate or Azure
  Trusted Signing. See [releasing.md](./releasing.md). Without signing the
  installers work but show security warnings.
- **Cloud hosting**: see [deployment-cloud.md](./deployment-cloud.md). For one
  machine, `deploy/docker-compose.yml` with `deploy/.env.example` is enough.
