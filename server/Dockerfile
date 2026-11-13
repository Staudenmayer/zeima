# Zeima API image (SPEC §7.0): Node 24, non-root, runs the TypeScript
# sources directly (type stripping). Build from the repository root:
#   docker build -f server/Dockerfile -t zeima-api .

FROM docker.io/oven/bun:1.4 AS deps
WORKDIR /app
COPY package.json bun.lock ./
COPY server/package.json server/
COPY packages/api-contract/package.json packages/api-contract/
COPY packages/shared-fixtures/package.json packages/shared-fixtures/
# Only the server's production dependencies, not the desktop frontend's.
RUN bun install --frozen-lockfile --production --filter '@zeima/server'

FROM docker.io/library/node:24-slim
ENV NODE_ENV=production \
    PORT=3000
WORKDIR /app
COPY --from=deps /app/node_modules ./node_modules
COPY --from=deps /app/server/node_modules ./server/node_modules
COPY server/package.json ./server/
COPY server/src ./server/src
COPY server/drizzle ./server/drizzle
COPY packages/shared-fixtures/permissions.yaml ./packages/shared-fixtures/permissions.yaml
USER node
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s \
  CMD ["node", "-e", "fetch('http://127.0.0.1:'+process.env.PORT+'/healthz').then(r=>process.exit(r.ok?0:1),()=>process.exit(1))"]
CMD ["node", "server/src/server.ts"]
