# Running a Zeima server

For the people who install and look after a self-hosted installation. Setting up is covered by the
guides it points to; this page is the map and the day-to-day.

## Choose how to run it

| | Guide | Good for |
|---|---|---|
| Docker Compose (API, Keycloak, PostgreSQL, Caddy with automatic certificates) | [`deploy/docker-compose.yml`](../deploy/docker-compose.yml), [`deploy/.env.example`](../deploy/.env.example) | One machine, up to a few hundred people |
| Kubernetes | [helm.md](helm.md) | Where a cluster already is |
| No internet | [airgap.md](airgap.md) | Closed networks |

All three run the same pieces: **Caddy/ingress** (HTTPS) → **API** (`server/`, Node) and **Keycloak** (sign-in,
organizations, roles) → **PostgreSQL**. The API is at `PUBLIC_URL`, sign-in at `ID_URL`; both must be addresses the
people's machines can reach, and `ID_URL` must not change (tokens carry it).

## First day

1. Install, then open `PUBLIC_URL/.well-known/zeima-server`: it should answer with JSON.
2. In the app: Settings → Account → Server → your `PUBLIC_URL`, then Sign in and register the first person.
3. Make that person the platform administrator in Keycloak (`ID_URL/admin`, user `admin`, realm `zeima` → Users →
   Role mapping → realm role `platform-admin`). Platform administrators upload licenses and set up a company directory.
4. Upload your license (`PUT /v1/license`, `server/README.md`): until then the server offers the Free plan.
5. Mail: invitations and password resets are sent by Keycloak (`SMTP_*` in `.env`). Test one invitation.

## Settings

Everything is an environment variable, checked at start-up; the API refuses to start on a wrong value and says which.
[`server/.env.example`](../server/.env.example) lists them all with their meaning. The ones an administrator usually
touches:

| Variable | Meaning |
|---|---|
| `PUBLIC_URL`, `KEYCLOAK_URL`, `KEYCLOAK_INTERNAL_URL` | The addresses (the internal one is how the API reaches Keycloak inside the network). |
| `KEYCLOAK_ADMIN_CLIENT_SECRET` | Lets the API manage organizations. Without it organization routes answer 503. |
| `KEYCLOAK_WEBHOOK_SECRET` | Lets Keycloak tell the API about changes at once; otherwise the nightly check does it. |
| `RECONCILE_INTERVAL_HOURS` | How often the API checks its copy of members against Keycloak (24). |
| `ENTITLEMENT_PUBLIC_KEYS` | Vendor keys whose licenses are accepted. |
| `SSO_ALLOW_PRIVATE_URLS` | Allow single sign-on providers on a private network (set it on an internal installation). |
| `ADDON_REGISTRY_DIR`, `JURISDICTIONS_DIR` | Addons and country profiles you offer ([addons.md](addons.md), [country-profiles.md](country-profiles.md)). |
| `DOCS_ENABLED` | The API reference at `/docs`; off in production. |

## People and sign-in

- **Organizations, roles, departments** are managed by the organization's owners and admins in the app (Organization).
- **Single sign-on** for an organization (Entra ID, Google Workspace, OIDC, SAML), **SCIM** provisioning and a **company
  directory (LDAP / Active Directory)** are described in [keycloak.md](keycloak.md).
- **Seats**: an organization's license has a number of seats; people who join by SSO or SCIM beyond it are turned away
  (the activity log says `member.rejected`).
- **A disabled account** is signed out within seconds.

## Addons and rules

Offer addons from your own signed registry and, per organization, limit which ones may run
([addons.md](addons.md), "Signing and the registry").

## Looking after it

| | |
|---|---|
| **Health** | `GET /healthz` (alive), `GET /readyz` (database and Keycloak reachable). Keycloak: port 9000 `/health/ready`. |
| **Metrics** | Prometheus at `/metrics` of the API (`api:3000` inside the network). The Caddy files answer 404 for it on the public address; with Helm it is off unless `api.metricsEnabled=true`, and then your ingress must block it. |
| **Logs** | The API logs JSON to stdout (`LOG_LEVEL`). `docker compose logs api keycloak`. |
| **Backups** | `pg_dump` of the `zeima` and `keycloak` databases, and your secrets ([upgrading.md](upgrading.md#1-back-up)). Test a restore once. Desktop apps keep their own daily backups. |
| **Upgrades** | [upgrading.md](upgrading.md). Update the server before the apps. |
| **One API process** | Do not run two copies: presence and live notifications are in memory. |
| **Certificates** | Caddy renews its own. With your own certificates ([airgap.md](airgap.md)) renew the files and `docker compose restart caddy`. |

## When something is wrong

| Symptom | Look at |
|---|---|
| The app says it cannot find the server | `/.well-known/zeima-server`; `PUBLIC_URL` must be what people type; certificate trusted by their machines. |
| Sign-in loops or tokens are refused | `ID_URL` differs from what Keycloak calls itself (`KC_HOSTNAME`); clock drift between machines. |
| Organization pages answer 503 | `KEYCLOAK_ADMIN_CLIENT_SECRET` is not set, or does not match the realm's `zeima-api-admin` secret (`apply.sh` fixes the realm side). |
| Invitations do not arrive | Keycloak's SMTP settings; the Keycloak log shows the send error. |
| Someone who joined by SSO is missing | The seat limit: `member.rejected` in the organization's activity. |
| A new member appears late | The webhook secret is not set or does not match, so only the nightly check catches it. |
