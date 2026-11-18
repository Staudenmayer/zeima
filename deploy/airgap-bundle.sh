#!/usr/bin/env bash
# Makes the bundle for an installation without internet access (docs/airgap.md). Run it on a machine
# that has internet access, Docker or Podman, and this repository.
#
#   deploy/airgap-bundle.sh [--version 2.0.0] [--out dist] [--no-images]
#
# Result: <out>/zeima-airgap-<version>.tar.gz with the images (postgres, caddy, the API, Keycloak with
# the Zeima listener and theme), the compose files, the Helm chart and an install script, plus a
# SHA256SUMS file next to it. `--no-images` leaves the images out (to look at the layout).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
version="$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version *= *"\(.*\)"/\1/p}' "$ROOT/Cargo.toml" | head -1)"
version="${version:-latest}"
out="$ROOT/dist"
images=1
while [ $# -gt 0 ]; do
  case "$1" in
    --version) version="$2"; shift 2 ;;
    --out) out="$(cd "$2" 2>/dev/null && pwd || echo "$2")"; shift 2 ;;
    --no-images) images=0; shift ;;
    -h|--help) sed -n 2,11p "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

engine=""
if [ "$images" = 1 ]; then
  for c in docker podman; do command -v "$c" >/dev/null 2>&1 && engine="$c" && break; done
  [ -n "$engine" ] || { echo "needs docker or podman (or --no-images)" >&2; exit 1; }
fi

name="zeima-airgap-$version"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
dir="$stage/$name"
mkdir -p "$dir/deploy/keycloak" "$dir/images" "$out"

# Compose files and what they mount.
cp "$ROOT/deploy/docker-compose.yml" "$ROOT/deploy/docker-compose.airgap.yml" "$ROOT/deploy/Caddyfile" "$ROOT/deploy/Caddyfile.airgap" \
   "$ROOT/deploy/postgres-init.sh" "$ROOT/deploy/.env.example" "$dir/deploy/"
cp -r "$ROOT/deploy/keycloak/themes" "$dir/deploy/keycloak/themes"
cp "$ROOT/deploy/keycloak/realm-zeima.json" "$ROOT/deploy/keycloak/apply.sh" "$dir/deploy/keycloak/"
mkdir -p "$dir/deploy/addon-registry" "$dir/deploy/jurisdictions" "$dir/deploy/certs"

# The tools for making an addon registry (no internet needed).
mkdir -p "$dir/tools"
cp "$ROOT/scripts/addons.mjs" "$dir/tools/"

# The Helm chart, packaged.
if command -v helm >/dev/null 2>&1; then
  helm package "$ROOT/deploy/helm/zeima" --destination "$dir" >/dev/null
else
  echo "helm not found: the chart is not in the bundle" >&2
fi

cp "$ROOT/docs/airgap.md" "$ROOT/docs/upgrading.md" "$ROOT/docs/admin-guide.md" "$dir/" 2>/dev/null || true

cat > "$dir/install.sh" <<INSTALL
#!/usr/bin/env bash
# Loads the images and starts Zeima from this folder (docs/airgap.md).
set -euo pipefail
cd "\$(dirname "\$0")"
engine=docker; command -v docker >/dev/null 2>&1 || engine=podman
for f in images/*.tar; do [ -e "\$f" ] && "\$engine" load -i "\$f"; done
cd deploy
[ -f .env ] || { cp .env.example .env; echo "Fill in deploy/.env (see docs/airgap.md), then run this script again."; exit 0; }
[ -f certs/zeima.crt ] && [ -f certs/zeima.key ] || { echo "Put certs/zeima.crt and certs/zeima.key in deploy/ first (docs/airgap.md)."; exit 1; }
export ZEIMA_VERSION=$version
"\$engine" compose -f docker-compose.yml -f docker-compose.airgap.yml --env-file .env up -d --no-build
INSTALL
chmod +x "$dir/install.sh"

if [ "$images" = 1 ]; then
  echo "building the images…"
  "$engine" build -f "$ROOT/server/Dockerfile" -t "zeima-api:$version" "$ROOT"
  "$engine" build -t "zeima-keycloak:26.8.1" "$ROOT/deploy/keycloak"
  for img in docker.io/library/postgres:17 docker.io/library/caddy:2; do "$engine" pull "$img"; done
  "$engine" save -o "$dir/images/zeima.tar" "zeima-api:$version" "zeima-keycloak:26.8.1" docker.io/library/postgres:17 docker.io/library/caddy:2
fi

tar -C "$stage" -czf "$out/$name.tar.gz" "$name"
(cd "$out" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
echo "$out/$name.tar.gz"
