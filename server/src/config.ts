// Server configuration from environment variables (SPEC §7.1: everything is
// configured through env vars and checked at start-up).

import { type Static, Type } from 'typebox'
import { Value } from 'typebox/value'

const LogLevel = Type.Union(['fatal', 'error', 'warn', 'info', 'debug', 'trace', 'silent'].map((l) => Type.Literal(l)))

export const EnvSchema = Type.Object({
  NODE_ENV: Type.Union([Type.Literal('development'), Type.Literal('production'), Type.Literal('test')], {
    default: 'development',
  }),
  HOST: Type.String({ default: '0.0.0.0' }),
  PORT: Type.Integer({ minimum: 1, maximum: 65535, default: 3000 }),
  /** Address clients use, for the API docs and links. */
  PUBLIC_URL: Type.String({ format: 'uri', default: 'http://localhost:3000' }),
  LOG_LEVEL: { ...LogLevel, default: 'info' },
  /** Behind a reverse proxy (Caddy, ingress): trust its X-Forwarded-* headers. */
  TRUST_PROXY: Type.Boolean({ default: false }),
  /** `cloud` enables billing and sign-up modules; self-hosted installs keep the default. */
  DEPLOYMENT_MODE: Type.Union([Type.Literal('cloud'), Type.Literal('self_hosted')], { default: 'self_hosted' }),

  /** Keycloak base URL, without `/realms/…`. */
  KEYCLOAK_URL: Type.String({ format: 'uri', default: 'http://localhost:8080' }),
  /** Where the API itself reaches Keycloak, if not at KEYCLOAK_URL (e.g.
   * `http://keycloak:8080` inside Docker). Tokens are still checked against
   * the public KEYCLOAK_URL as issuer. */
  KEYCLOAK_INTERNAL_URL: Type.Optional(Type.String({ format: 'uri' })),
  KEYCLOAK_REALM: Type.String({ minLength: 1, default: 'zeima' }),
  /** `aud` access tokens must carry. */
  API_AUDIENCE: Type.String({ minLength: 1, default: 'zeima-api' }),
  /** Clients (`azp`) whose tokens are accepted, comma-separated. */
  ALLOWED_CLIENTS: Type.String({ default: 'zeima-desktop,zeima-swagger,zeima-web' }),
  /** Public client of the Swagger UI "Authorize" button. */
  DOCS_CLIENT_ID: Type.String({ default: 'zeima-swagger' }),

  DOCS_ENABLED: Type.Boolean({ default: true }),
  METRICS_ENABLED: Type.Boolean({ default: true }),
  /** Browser origins allowed by CORS, comma-separated (the desktop app needs none). */
  CORS_ORIGINS: Type.String({ default: '' }),
  RATE_LIMIT_PER_MINUTE: Type.Integer({ minimum: 1, default: 300 }),
})

export type Env = Static<typeof EnvSchema>

export interface Config {
  env: Env['NODE_ENV']
  host: string
  port: number
  publicUrl: string
  logLevel: string
  trustProxy: boolean
  deploymentMode: Env['DEPLOYMENT_MODE']
  keycloak: {
    url: string
    realm: string
    /** `<url>/realms/<realm>`: the token `iss`. */
    issuer: string
    jwksUrl: string
    authorizationUrl: string
    tokenUrl: string
  }
  audience: string
  allowedClients: string[]
  docsClientId: string
  docsEnabled: boolean
  metricsEnabled: boolean
  corsOrigins: string[]
  rateLimitPerMinute: number
}

export class ConfigError extends Error {
  readonly problems: string[]

  constructor(problems: string[]) {
    super(`invalid configuration:\n  ${problems.join('\n  ')}`)
    this.name = 'ConfigError'
    this.problems = problems
  }
}

const list = (value: string) =>
  value
    .split(',')
    .map((v) => v.trim())
    .filter(Boolean)

/** Reads and checks the configuration; throws `ConfigError` listing every problem. */
export function loadConfig(source: Record<string, string | undefined> = process.env): Config {
  // Only our keys; empty values count as unset.
  const raw: Record<string, unknown> = {}
  for (const key of Object.keys(EnvSchema.properties)) {
    const value = source[key]
    if (value !== undefined && value.trim() !== '') raw[key] = value.trim()
  }
  const value = Value.Convert(EnvSchema, Value.Default(EnvSchema, raw))
  if (!Value.Check(EnvSchema, value)) {
    const problems = [...Value.Errors(EnvSchema, value)].map((e) => {
      const key = e.instancePath.replace(/^\//, '') || String(e.params && 'requiredProperties' in e.params ? e.params.requiredProperties : '')
      return `${key}: ${e.message}`
    })
    throw new ConfigError(problems)
  }
  const env = value as Env
  const keycloakUrl = env.KEYCLOAK_URL.replace(/\/+$/, '')
  const realmPath = `/realms/${encodeURIComponent(env.KEYCLOAK_REALM)}`
  const issuer = `${keycloakUrl}${realmPath}`
  const internal = `${(env.KEYCLOAK_INTERNAL_URL ?? keycloakUrl).replace(/\/+$/, '')}${realmPath}`
  return {
    env: env.NODE_ENV,
    host: env.HOST,
    port: env.PORT,
    publicUrl: env.PUBLIC_URL.replace(/\/+$/, ''),
    logLevel: env.LOG_LEVEL,
    trustProxy: env.TRUST_PROXY,
    deploymentMode: env.DEPLOYMENT_MODE,
    keycloak: {
      url: keycloakUrl,
      realm: env.KEYCLOAK_REALM,
      issuer,
      jwksUrl: `${internal}/protocol/openid-connect/certs`,
      authorizationUrl: `${issuer}/protocol/openid-connect/auth`,
      tokenUrl: `${issuer}/protocol/openid-connect/token`,
    },
    audience: env.API_AUDIENCE,
    allowedClients: list(env.ALLOWED_CLIENTS),
    docsClientId: env.DOCS_CLIENT_ID,
    docsEnabled: env.DOCS_ENABLED,
    metricsEnabled: env.METRICS_ENABLED,
    corsOrigins: list(env.CORS_ORIGINS),
    rateLimitPerMinute: env.RATE_LIMIT_PER_MINUTE,
  }
}
