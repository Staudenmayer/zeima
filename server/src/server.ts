// Entry point: `node src/server.ts` (Node 24 runs the TypeScript directly).

import { buildApp } from './app.ts'
import { listenInternal } from './internal-listener.ts'
import { ConfigError, loadConfig } from './config.ts'
import { connect, enableTenantLogin, migrateDb, tenantUrlFor } from './db/index.ts'
import { reconcileAll } from './modules/orgs/reconcile.ts'

let config: ReturnType<typeof loadConfig>
try {
  config = loadConfig()
} catch (err) {
  // Fail fast with every problem listed (SPEC §7.1).
  console.error(err instanceof ConfigError ? err.message : err)
  process.exit(1)
}

const { client, db, tenantClient } = connect(
  config.databaseUrl,
  {},
  config.appDatabasePassword ? tenantUrlFor(config.databaseUrl, config.appDatabasePassword) : undefined,
)
// Self-hosted installs have no separate migration step: the API brings its
// schema up to date before it listens.
await migrateDb(db)
if (config.appDatabasePassword) await enableTenantLogin(db, config.appDatabasePassword)

const app = await buildApp(config, { db })
if (!config.appDatabasePassword && config.env === 'production') {
  // Not an error (a custom setup may do without), but the weaker arrangement: requests step down from the owner's session.
  app.log.warn('APP_DATABASE_PASSWORD is not set: requests run through the owner database login with SET ROLE, which a bug in a query could undo. Set it so they use the restricted login (docs/admin-guide.md, "The database logins").')
}
app.addHook('onClose', async () => {
  await tenantClient?.end()
  await client.end()
})

// The member and department mirror is checked against Keycloak now and then,
// in case an event was missed.
if (config.reconcileIntervalHours > 0 && app.keycloakAdmin) {
  const kc = app.keycloakAdmin
  const run = () => reconcileAll(db, kc, (msg, err) => (err ? app.log.warn({ err }, msg) : app.log.info(msg)), {
      seatLimit: async (orgId) => (await app.licensing.ofOrg(db, orgId))?.seats ?? null,
    })
  const first = setTimeout(run, 60_000)
  const every = setInterval(run, config.reconcileIntervalHours * 3_600_000)
  app.addHook('onClose', async () => {
    clearTimeout(first)
    clearInterval(every)
  })
}

for (const signal of ['SIGINT', 'SIGTERM'] as const) {
  process.once(signal, () => {
    app.log.info({ signal }, 'shutting down')
    app.close().then(
      () => process.exit(0),
      (err: unknown) => {
        app.log.error({ err }, 'shutdown failed')
        process.exit(1)
      },
    )
  })
}

let internalServer: Awaited<ReturnType<typeof listenInternal>> | null = null
app.addHook('onClose', async () => {
  await new Promise<void>((resolve) => (internalServer ? internalServer.close(() => resolve()) : resolve()))
})

// The development signing key is public: a server that trusts it must not be one that serves other people.
if (config.env !== 'production') {
  if (config.deploymentMode === 'cloud') {
    app.log.fatal('NODE_ENV is not production on a cloud deployment: the development license key would be trusted. Refusing to start.')
    process.exit(1)
  }
  app.log.warn('NODE_ENV is not production: the public development license key is trusted. Never expose this server to other people.')
}

try {
  await app.listen({ host: config.host, port: config.port })
  if (config.internalPort !== null) internalServer = await listenInternal(app, config.internalPort, config.host)
} catch (err) {
  app.log.fatal({ err }, 'could not start')
  process.exit(1)
}
