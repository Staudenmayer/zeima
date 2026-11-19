# Zeima on Kubernetes (Helm)

The chart `deploy/helm/zeima` runs the same stack as `deploy/docker-compose.yml`: the API, Keycloak (with the
Zeima event listener, theme and realm) and PostgreSQL, behind an ingress. It is for self-hosted
installations; the cloud edition is described in [deployment-cloud.md](deployment-cloud.md).

## Images

There is no public registry yet: build the two images and push them to yours.

```sh
docker build -f server/Dockerfile -t registry.example.com/zeima/api:2.0.0 .
docker build -t registry.example.com/zeima/keycloak:26.8.1 deploy/keycloak   # includes theme and realm
docker push registry.example.com/zeima/api:2.0.0
docker push registry.example.com/zeima/keycloak:26.8.1
```

For a cluster without internet access see [airgap.md](airgap.md).

## Install

You need two host names that point at the ingress: one for the API (`publicUrl`), one for sign-in
(`idUrl`). Tokens carry `idUrl` as their issuer, so it must be the address people really use.

```sh
helm install zeima deploy/helm/zeima -n zeima --create-namespace \
  --set publicUrl=https://zeima.example.com \
  --set idUrl=https://id.zeima.example.com \
  --set api.image.repository=registry.example.com/zeima/api --set api.image.tag=2.0.0 \
  --set keycloak.image.repository=registry.example.com/zeima/keycloak \
  --set ingress.className=nginx --set ingress.tlsSecretName=zeima-tls \
  --set smtp.host=smtp.example.com --set smtp.port=587 --set smtp.starttls=true \
  --set keycloak.verifyEmail=true
```

or put the values in a file (`values.yaml` documents all of them) and use `-f`. The notes printed at the end tell
you how to read Keycloak's admin password. Then enter `https://zeima.example.com` in the app (Settings → Account →
Server).

## What the chart makes

| Object | Notes |
|---|---|
| `<release>-api` | Deployment (one replica: live connections are in memory), Service, probes on `/healthz` and `/readyz`; runs as non-root with a read-only file system. Migrations run when it starts. |
| `<release>-keycloak` | Deployment (one replica, `start --import-realm`), Service, probes on port 9000. |
| `<release>-postgres` | StatefulSet with a volume, and a second database for Keycloak. Switch off with `postgres.enabled=false` and fill `externalDatabase`. |
| `<release>` | Ingress for both host names. |
| `<release>-secrets` | The six secrets, generated on the first install and **kept when you upgrade**. Use `secrets.existingSecret` for your own (below). |
| NetworkPolicies | Default deny, then: the ingress controller reaches the API (3000) and Keycloak (8080), only Keycloak reaches the API's internal port (3001), only the API and Keycloak reach PostgreSQL. Needs a network plugin that enforces them. `networkPolicy.enabled=false` switches them off; `networkPolicy.ingressFrom` narrows where the ingress controller runs. |

### Secrets

Leave `secrets.*` empty and the chart generates them. To manage them yourself, make a Secret with the keys
`postgres-password`, `app-db-password`, `keycloak-db-password`, `keycloak-admin-password`, `api-admin-secret`, `webhook-secret` (and
`smtp-password` if you use authenticated mail) and set `secrets.existingSecret`. The realm is read only on
Keycloak's first start: choose `api-admin-secret` before the first install, or apply the realm again with
`deploy/keycloak/apply.sh` (see [upgrading.md](upgrading.md)).

### Decide: registration

`keycloak.registrationAllowed` has no default; the chart does not install until you set it to `true` or `false` (may people create their own accounts?).
See docs/admin-guide.md.

### Outgoing traffic

Besides default-deny for incoming traffic, the API and Keycloak may only send to DNS, each other, the database and **public** addresses (https;
and the mail port for Keycloak). Private ranges, link-local and the cloud metadata address are blocked, so a provider address cannot be used to
reach your internal services. List anything inside your network they must reach (a mail relay, a directory, an external database, an identity
provider on a private network) in `networkPolicy.egressAllowCidrs`; `networkPolicy.restrictEgress=false` turns it off.

### The sign-in host

The ingress publishes only `/realms/zeima` and `/resources` on the sign-in host (`idUrl`): not Keycloak's administration console or the master
realm. Reach the console with `kubectl port-forward -n <ns> svc/<release>-keycloak 8080:8080`, or set `exposeKeycloakAdmin=true` (behind a VPN or an
IP allowlist). `keycloak.verifyEmail` is on by default, so the install needs `smtp.host`; the API's `trustProxy` is `1`, which is right for
one ingress controller.

### Your own database

Create two databases with an owner each (default names `zeima` and `keycloak`, set in `externalDatabase`), then:

```sh
--set postgres.enabled=false --set externalDatabase.host=db.example.com
```

and give the passwords in the Secret. The API's user must own its database: it creates tables and policies.

### Addons and country profiles

```sh
# the addon registry (docs/addons.md): a volume the API reads
helm upgrade zeima deploy/helm/zeima -n zeima --reuse-values --set addonRegistry.enabled=true
node scripts/addons.mjs publish ./registry --publisher acme=Acme="<public key>" hours-1.0.0.zip
kubectl cp ./registry/. zeima/$(kubectl get pod -n zeima -l app.kubernetes.io/name=zeima-api -o name | cut -d/ -f2):/data/addon-registry
```

The volume is mounted read-only in the API pod, so copy into it from a pod that mounts it for writing (a one-off
`kubectl run` with the claim `<release>-addon-registry`), or set `addonRegistry.existingClaim` to a volume you fill another
way. Country profiles are small: put them in the values (`jurisdictions.files."LU.toml"`, with `--set-file`) or in a
ConfigMap you manage (`jurisdictions.existingConfigMap`); the API restarts when they change.

## Operations

- **Logs/metrics**: the API logs JSON to stdout. Prometheus metrics (`/metrics`) are off by default because the ingress would publish them; set `api.metricsEnabled=true` when your ingress blocks that path or only the cluster scrapes it.
- **Backups**: back up PostgreSQL (both databases) and keep `<release>-secrets`. Zeima has no other state except the
  addon registry volume.
- **Upgrades**: see [upgrading.md](upgrading.md).
- **Licenses**: `ENTITLEMENT_PUBLIC_KEYS` via `api.entitlementPublicKeys`; licenses are uploaded to the API as described
  in `server/README.md`.
