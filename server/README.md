# Zeima API

Sync, organizations and licensing for the desktop app (SPEC §7, PLAN Phase 5). Fastify 5 + TypeBox, Keycloak for sign-in. Node 24 runs the TypeScript sources directly; there is no build step.

```sh
docker compose -f deploy/compose.dev.yml up -d   # Postgres, Keycloak (realm zeima), Mailpit
cp server/.env.example server/.env
bun run --cwd server dev                          # http://localhost:3000/docs
```

| Command | |
|---|---|
| `bun run --cwd server test` | Vitest, incl. the contract check |
| `bun run --cwd server typecheck` | `tsc --noEmit` |
| `bun run --cwd server openapi:export` | Rewrites `packages/api-contract/openapi.json` after route changes |
| `docker build -f server/Dockerfile .` | Production image (from the repository root) |

Configuration is environment variables only; `src/config.ts` lists them with defaults and fails at start-up with every problem listed.

**Conventions**
- Routes live in `src/modules/<feature>/routes.ts` and declare `tags`, `summary` and response schemas for every status. A test enforces it.
- A route is protected by `security: SECURITY` in its schema; the auth plugin enforces exactly what the docs show. `request.user` holds the user and their org memberships; `X-Org-Id` picks the active org (`request.orgId`).
- Errors are thrown as `HttpProblem(status, code, detail)` and sent as `application/problem+json`. Use `problems(401, 403, …)` for their response schemas.
