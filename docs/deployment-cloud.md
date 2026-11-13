# Cloud deployment (Zeima cloud edition)

This is the checklist for running the hosted service. It needs accounts and
infrastructure that are not part of the repository; the repository supplies the
images, the realm, the migrations and the checks. The single-machine version
is `deploy/docker-compose.yml`.

## Shape

```
Internet ─ CDN/WAF ─ load balancer (TLS) ─┬─ API ×2+      (server/Dockerfile, stateless)
                                          └─ Keycloak ×2+ (HA, same realm file)
                       managed PostgreSQL ── databases `zeima` and `keycloak` (separate users)
```

- **Region**: EU (customers are EU-based; the data processing agreement and
  VAT handling assume it). Keep API, Keycloak and both databases in one region.
- **API**: run the image from `server/Dockerfile` with `DEPLOYMENT_MODE=cloud`,
  `TRUST_PROXY=true`, `NODE_ENV=production`. It applies its migrations on
  start (`DATABASE_URL` needs a role that may create the `zeima_app` role and
  tables). Give it `ENTITLEMENT_SIGNING_KEY` (Ed25519 PKCS#8, base64url) from a
  secret store, never from the image. The matching public key goes into the
  apps (`ZEIMA_ENTITLEMENT_KEYS`, see `docs/releasing.md`).
- **Keycloak**: two or more nodes with the `jdbc-ping` cache stack
  (`KC_CACHE=ispn`, `KC_CACHE_STACK=jdbc-ping`) against the same database, behind
  the load balancer with `KC_PROXY_HEADERS=xforwarded` and
  `KC_HOSTNAME=<ID_URL>`. Start with `--optimized` from an image built with
  `kc.sh build`. Apply realm changes with `deploy/keycloak/apply.sh` during the
  rollout, not by re-importing.
- **PostgreSQL**: managed service, private network only, TLS on, automated
  backups with point-in-time recovery (test a restore), a read replica is not
  needed at first. The API connects as the owner role and switches to
  `zeima_app` per request (row-level security, `server/drizzle/0001_rls.sql`).
- **Billing**: Lemon Squeezy webhook → `POST /v1/billing/webhook`
  (`BILLING_WEBHOOK_SECRET`, `BILLING_STORE_URL`, `BILLING_PLANS`; see
  `server/README.md`). Only the cloud registers these routes.
- **Social login**: `docs/keycloak.md`.
- **Mail**: an SMTP relay for Keycloak (verification, password reset) with SPF,
  DKIM and DMARC set up for the sender domain.

## Operations

- **Health and metrics**: `/healthz` (liveness), `/readyz` (Keycloak and
  database), `/metrics` (Prometheus). Alert on readiness failing for more than
  two minutes, 5xx rate, p95 latency and database connections.
- **Traces and errors** (optional): add OpenTelemetry (`@opentelemetry/sdk-node`)
  or Sentry to `server/src/server.ts` once there is a collector to send to.
- **Logs**: JSON to stdout (Pino); the authorization header is redacted.
- **Rollouts**: the API is backwards compatible within `/v1`; the CI job
  "No breaking API changes" enforces it. Raise `MIN_CLIENT_VERSION` only to cut
  off app versions that cannot work with the server.
- **Secrets to create**: `POSTGRES` and `KEYCLOAK` database passwords,
  Keycloak admin password, `API_ADMIN_SECRET` (client secret of
  `zeima-api-admin`), `ENTITLEMENT_SIGNING_KEY`, `BILLING_WEBHOOK_SECRET`.
- **Backups**: database backups plus the realm file in git are enough to rebuild;
  also export Keycloak users before major Keycloak upgrades.

## Before the first customer

1. A staging environment that is a copy of this one (run `deploy/e2e/smoke.mjs`
   against it).
2. Restore drill for both databases.
3. Load test: sign in, push 500 changes, pull from a second device
   (`deploy/e2e/smoke.mjs` is the template).
4. Terms, privacy policy and the data processing agreement (SPEC §12).
