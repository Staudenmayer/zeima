# Development image: Rust + Node 24 + bun, plus the system libraries Tauri
# needs on Linux (same list as the CI workflow). Build tools only; the
# source is mounted by the dev container.
FROM mcr.microsoft.com/devcontainers/base:ubuntu-24.04

RUN apt-get update && export DEBIAN_FRONTEND=noninteractive \
    && apt-get install -y --no-install-recommends \
       build-essential pkg-config libssl-dev \
       libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev \
       libdbus-1-dev postgresql-client unzip \
    && rm -rf /var/lib/apt/lists/*

# bun for the workspace (root app + server + packages)
ENV BUN_INSTALL=/usr/local/bun
RUN curl -fsSL https://bun.sh/install | bash \
    && ln -s /usr/local/bun/bin/bun /usr/local/bin/bun \
    && chmod -R a+rX /usr/local/bun
