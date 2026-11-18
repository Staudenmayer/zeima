#!/usr/bin/env bash
# Fills the development stack with demo data (docs/seed-demo-data.md), without
# looking up ids by hand: the organization and its owner are read from Keycloak.
#
#   deploy/dev/seed.sh                      the first organization, its owner
#   deploy/dev/seed.sh --org <org id>       a given organization
#   deploy/dev/seed.sh --local              also fill the app's local database (close the app first)
#   deploy/dev/seed.sh --local-only
#
# Needs: the dev stack (deploy/compose.dev.yml) and the API running (bun run --cwd server dev),
# server/.env, an organization you created in the app, curl and python3.
# Development only. Never run it against a real installation.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
ID_URL="${ID_URL:-http://localhost:8080}"
REALM="${REALM:-zeima}"
KC_ADMIN_PASSWORD="${KEYCLOAK_ADMIN_PASSWORD:-admin}"
DB="${ZEIMA_DB:-$HOME/.local/share/com.zeima.zeima/zeima.db}"

org=""; local_too=0; local_only=0
while [ $# -gt 0 ]; do
  case "$1" in
    --org) org="$2"; shift 2 ;;
    --local) local_too=1; shift ;;
    --local-only) local_only=1; shift ;;
    -h|--help) sed -n 2,12p "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

if [ "$local_only" = 0 ]; then
  token=$(curl -sf -d client_id=admin-cli -d grant_type=password -d username=admin -d "password=$KC_ADMIN_PASSWORD" \
    "$ID_URL/realms/master/protocol/openid-connect/token" | python3 -c 'import sys,json;print(json.load(sys.stdin)["access_token"])') \
    || { echo "cannot sign in to Keycloak at $ID_URL (is the dev stack up?)" >&2; exit 1; }
  kc() { curl -sf -H "Authorization: Bearer $token" "$ID_URL/admin/realms/$REALM$1"; }

  if [ -z "$org" ]; then
    org=$(kc /organizations | python3 -c 'import sys,json;o=json.load(sys.stdin);print(o[0]["id"] if o else "")')
    [ -n "$org" ] || { echo "Keycloak has no organization yet: create one in the app first." >&2; exit 1; }
  fi
  # The owner: the member of the organization's owner role group.
  # /orgs/<org>/roles/owner, found one level at a time (a search does not expand sub groups).
  child() { kc "$1" | python3 -c 'import sys,json;n=sys.argv[1];print(next((g["id"] for g in json.load(sys.stdin) if g["name"]==n),""))' "$2"; }
  group=$(kc "/groups?search=orgs&exact=true" | python3 -c 'import sys,json;print(next((g["id"] for g in json.load(sys.stdin) if g["path"]=="/orgs"),""))')
  for name in "$org" roles owner; do
    [ -n "$group" ] || break
    group=$(child "/groups/$group/children?max=1000" "$name")
  done
  owner=""
  [ -n "$group" ] && owner=$(kc "/groups/$group/members" | python3 -c 'import sys,json;m=json.load(sys.stdin);print(m[0]["id"] if m else "")')
  [ -n "$owner" ] || { echo "organization $org has no owner in Keycloak (wrong id, or the stack was recreated?)" >&2; exit 1; }
  echo "organization $org, owner $owner"
  (cd "$ROOT/server" && ORG_ID="$org" OWNER_ID="$owner" node --env-file=.env scripts/seed-org.ts)
fi

if [ "$local_too" = 1 ] || [ "$local_only" = 1 ]; then
  [ -f "$DB" ] || { echo "no local database at $DB (set ZEIMA_DB)" >&2; exit 1; }
  cp "$DB" "$DB.backup"
  echo "local database: backup at $DB.backup"
  (cd "$ROOT" && cargo run -q -p zeima-core --features db --example seed -- "$DB")
fi
