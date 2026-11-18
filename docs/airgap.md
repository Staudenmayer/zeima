# Installing without internet access

For networks that cannot reach the internet. You make a **bundle** on a machine that can, carry it over, and install
from it. Nothing in Zeima needs the internet at run time: sign-in is Keycloak on your machine, licenses are files
checked offline, addons come from your own registry, and the apps update from a file you choose (or not at all).

## 1. Make the bundle (a machine with internet)

You need this repository, Docker or Podman, and (for the Helm chart) `helm`.

```sh
deploy/airgap-bundle.sh --version 2.0.0       # dist/zeima-airgap-2.0.0.tar.gz and .sha256
```

It builds the API and Keycloak images (Keycloak with the Zeima listener, theme and realm), pulls PostgreSQL and
Caddy, saves them in one `images/zeima.tar`, and packs them with the compose files, an air-gapped Caddy
configuration, the packaged Helm chart, `tools/addons.mjs` and these docs. Check the file after you carry it over:

```sh
sha256sum -c zeima-airgap-2.0.0.tar.gz.sha256
```

## 2. Install with Docker Compose

```sh
tar xzf zeima-airgap-2.0.0.tar.gz && cd zeima-airgap-2.0.0
./install.sh            # loads the images; the first run only makes deploy/.env and stops
$EDITOR deploy/.env     # DOMAIN, PUBLIC_URL, ID_URL, the four passwords (docs: deploy/.env.example)
```

Caddy cannot get a certificate from Let's Encrypt, so give it yours: one certificate for `DOMAIN` and `id.DOMAIN` (or a
wildcard), with the chain, as `deploy/certs/zeima.crt` and `deploy/certs/zeima.key`. Use your company CA, and make sure
the people's machines trust it (the app uses the system's trust store). Then:

```sh
./install.sh
```

which runs `docker compose -f docker-compose.yml -f docker-compose.airgap.yml --env-file .env up -d --no-build`.

Mail: invitations and password resets need an SMTP server inside your network (`SMTP_HOST` in `.env`). Without one,
set `VERIFY_EMAIL=false` and create people's first accounts in Keycloak's console.

## 3. Install with Helm

Load the images into your registry, then use the packaged chart:

```sh
docker load -i images/zeima.tar
docker tag zeima-api:2.0.0 registry.corp/zeima/api:2.0.0 && docker push registry.corp/zeima/api:2.0.0
docker tag zeima-keycloak:26.8.1 registry.corp/zeima/keycloak:26.8.1 && docker push registry.corp/zeima/keycloak:26.8.1
docker tag docker.io/library/postgres:17 registry.corp/postgres:17 && docker push registry.corp/postgres:17
helm install zeima zeima-0.1.0.tgz -n zeima --create-namespace -f my-values.yaml   # see docs/helm.md
```

Set `postgres.image.repository` to your copy of PostgreSQL.

## 4. What else you provide from inside

| What | How |
|---|---|
| The desktop app | The installer files from the release, on your software distribution. The app checks for updates at its release page, which it cannot reach: roll out new versions yourself. |
| Licenses | A license file from the vendor, uploaded to your server (`PUT /v1/license`); verified offline. |
| Addons | Your own registry (docs/addons.md): sign with your key, `publish`, put the folder in `deploy/addon-registry`. `tools/addons.mjs` in the bundle does it without the repository. |
| Country profiles | `deploy/jurisdictions/<ID>.toml` (docs/country-profiles.md). |
| Single sign-on / SCIM | Your identity provider must be reachable from the Keycloak container. The API refuses non-public provider addresses unless `SSO_ALLOW_PRIVATE_URLS=true` (set it, since your provider is on a private network). |

## Updating

Make a new bundle, carry it over, and follow [upgrading.md](upgrading.md): `./install.sh` loads the new images and
restarts the stack.
