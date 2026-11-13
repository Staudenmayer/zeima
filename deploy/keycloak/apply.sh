#!/bin/sh
# Applies deploy/keycloak/realm-zeima.json to a RUNNING Keycloak with
# keycloak-config-cli, so realm changes reach installations whose realm was
# imported long ago (Keycloak imports only on the first start). It changes what
# the file describes and leaves other settings, users and sessions alone.
#
#   KEYCLOAK_URL=https://id.example.com KEYCLOAK_PASSWORD=... \
#   ZEIMA_API_ADMIN_SECRET=... ZEIMA_API_URL=https://zeima.example.com \
#     deploy/keycloak/apply.sh
#
# Use the same ZEIMA_* values as the Keycloak container (see
# deploy/docker-compose.yml): placeholders left unset fall back to the realm
# file's development defaults. Every ZEIMA_* variable in the environment is
# passed through. Other variables:
#   KEYCLOAK_USER        admin user (default: admin)
#   CONTAINER_NETWORK    docker network to join (default: host)
#   CONTAINER            docker or podman (default: docker)
set -eu

here=$(cd "$(dirname "$0")" && pwd)
: "${KEYCLOAK_URL:?set KEYCLOAK_URL}"
: "${KEYCLOAK_PASSWORD:?set KEYCLOAK_PASSWORD}"
tool=${CONTAINER:-docker}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# Keycloak writes defaults as ${NAME:default}; keycloak-config-cli expects ${NAME:-default}.
sed -E 's/\$\{([A-Za-z_]+):([^}]*)\}/${\1:-\2}/g' "$here/realm-zeima.json" > "$work/realm-zeima.json"
# The tool runs as another user inside the container.
chmod 755 "$work"
chmod 644 "$work/realm-zeima.json"

env_args=""
for name in $(env | sed -n 's/^\(ZEIMA_[A-Za-z0-9_]*\)=.*/\1/p'); do
  env_args="$env_args -e $name"
done

# shellcheck disable=SC2086
"$tool" run --rm --network "${CONTAINER_NETWORK:-host}" -v "$work:/config:ro,z" \
  -e KEYCLOAK_URL -e "KEYCLOAK_USER=${KEYCLOAK_USER:-admin}" -e KEYCLOAK_PASSWORD \
  -e IMPORT_FILES_LOCATIONS=/config/realm-zeima.json \
  -e IMPORT_VARSUBSTITUTION_ENABLED=true \
  -e 'IMPORT_VARSUBSTITUTION_PREFIX=${' -e 'IMPORT_VARSUBSTITUTION_SUFFIX=}' \
  $env_args \
  docker.io/adorsys/keycloak-config-cli:latest-26
