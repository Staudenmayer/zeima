# ADR 0002: Fastify + OpenAPI server with Keycloak

- Status: accepted (implementation in Phase 5)
- Date: 2026-10-04

## Context

Paid tiers need a sync and organization server that can run in our cloud and
self-hosted at customers (SPEC §7). It needs user management, SSO and
documented APIs.

## Decision

- Server: Fastify 5 (TypeScript, Node 24 LTS) with TypeBox route schemas.
  `@fastify/swagger` generates OpenAPI 3.1, served with Swagger UI at `/docs`.
  The generated `openapi.json` is the contract; clients are generated from it.
- Identity: Keycloak (one realm, Keycloak Organizations as tenants). Keycloak
  owns authentication, org membership, roles and groups. Record-level
  authorization stays in the API (`can()`), not in Keycloak Authorization
  Services.
- Domain math is shared by compiling `zeima-core` to WASM.
- Cloud and self-hosted run the same images; only Postgres and Keycloak are
  required.

## Consequences

- Two languages on the server path (Rust domain via WASM, TS API); sync merge
  and permission rules are covered by shared JSON test vectors.
- Keycloak adds an operational component but replaces building login, MFA,
  SSO and LDAP ourselves.
