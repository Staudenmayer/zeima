# Upgrading a self-hosted installation

What to do, in order, whichever way you run Zeima. Read the release notes of the version you move to
(`docs/release-*.md`) for anything specific.

## Rules that make it safe

- **The API migrates its database when it starts** (`server/drizzle`). Migrations only add; a new version
  never removes what an older app uses, so a 2.x app keeps syncing with a newer 2.y server. **Going back is a
  restore from backup**, not a downgrade: take the backup first.
- **Keycloak imports its realm only once** (on its first start). Changes to `deploy/keycloak/realm-zeima.json`
  reach a running installation only when you apply them (step 3).
- **Update the server before the apps.** An app talks to older servers as far as it can, but a new server is
  built to keep working with the apps you already have.
- **One API process.** Presence is kept in memory; do not scale the API to two copies.

## 1. Back up

```sh
# Docker Compose (service name `postgres`)
docker compose -f deploy/docker-compose.yml exec -T postgres pg_dump -U zeima -Fc zeima    > zeima-api.dump
docker compose -f deploy/docker-compose.yml exec -T postgres pg_dump -U zeima -Fc keycloak > zeima-keycloak.dump

# Kubernetes
kubectl exec -n zeima zeima-postgres-0 -- pg_dump -U zeima -Fc zeima    > zeima-api.dump
kubectl exec -n zeima zeima-postgres-0 -- pg_dump -U zeima -Fc keycloak > zeima-keycloak.dump
```

Keep `deploy/.env` (or the `zeima-secrets` Secret) too: without the secrets a restore does not start. With an
external database use its own backup tooling.

## 2. Update

**Docker Compose** (from a checkout of the new version, or the new bundle):

```sh
git pull                                           # or unpack the new bundle (docs/airgap.md)
docker compose -f deploy/docker-compose.yml --env-file deploy/.env up -d --build
```

The Keycloak image is built from `deploy/keycloak/Dockerfile` (standard image, event listener, theme and realm),
so `--build` is needed. Watch the API start: `docker compose logs -f api` shows the migrations and then
`listening`. `/readyz` answers 200 when the database and Keycloak are reachable.

**Helm:**

```sh
helm upgrade zeima deploy/helm/zeima -n zeima --reuse-values \
  --set api.image.tag=2.1.0 --set keycloak.image.tag=26.8.1
kubectl rollout status deploy/zeima-api deploy/zeima-keycloak -n zeima
```

The generated secrets are kept. Use `--reuse-values` only with a chart of the same major version; after a chart
update compare your values with the new `values.yaml`.

**Keycloak itself** (a new base version): change the version in `deploy/keycloak/Dockerfile` and
`image:` in `deploy/docker-compose.yml` (or `keycloak.image.tag`), take the Keycloak database backup, and read
Keycloak's own upgrade notes. Keycloak migrates its database on start.

## 3. Apply the realm when it changed

If the release notes say the realm changed (new clients, roles for the API's service account, event listener
settings), apply it to the running Keycloak. It changes only what the file describes and leaves users and
sessions alone:

```sh
KEYCLOAK_URL=https://id.example.com KEYCLOAK_PASSWORD=<admin password> \
ZEIMA_API_ADMIN_SECRET=<API_ADMIN_SECRET> ZEIMA_API_URL=https://zeima.example.com \
ZEIMA_WEB_URL=https://zeima.example.com ZEIMA_SWAGGER_ENABLED=false \
ZEIMA_SMTP_HOST=… ZEIMA_SMTP_PORT=… ZEIMA_SMTP_FROM=… ZEIMA_VERIFY_EMAIL=… \
  deploy/keycloak/apply.sh
```

Use the same `ZEIMA_*` values as the Keycloak container: placeholders left unset fall back to the realm file's
development defaults (for example Swagger on). On Kubernetes run it from a machine that can reach `idUrl`.

## 4. Check

- `https://<PUBLIC_URL>/readyz` is 200 and `https://<PUBLIC_URL>/.well-known/zeima-server` answers.
- Sign in with an app; open Organization → Members (this reads the Keycloak copy).
- Optional, from a checkout: `node deploy/e2e/smoke.mjs` (sign-in and sync) and `deploy/e2e/org-smoke.mjs`
  (organizations; needs a test installation, not production data).

## Rolling back

Restore both database dumps into empty databases, start the previous version, and (if you applied a realm change)
apply the previous realm file. Data written between the backup and the rollback is lost, which is why you take
the backup first.

## Changes by version

| Version | What to do |
|---|---|
| 2.0 | Apply the realm (service account roles: `manage-organizations`, `manage-identity-providers`, `manage-realm`; the event listener `zeima-webhook`). Set `KEYCLOAK_ADMIN_CLIENT_SECRET` and `KEYCLOAK_WEBHOOK_SECRET`. Migrations 0006–0015 run by themselves. New optional settings: `SSO_*`, `ADDON_REGISTRY_DIR`, `JURISDICTIONS_DIR` (the compose file mounts `deploy/addon-registry` and `deploy/jurisdictions`). |
