// Entry point: `node src/server.ts` (Node 24 runs the TypeScript directly).

import { buildApp } from './app.ts'
import { ConfigError, loadConfig } from './config.ts'
import { connect, migrateDb } from './db/index.ts'

let config: ReturnType<typeof loadConfig>
try {
  config = loadConfig()
} catch (err) {
  // Fail fast with every problem listed (SPEC §7.1).
  console.error(err instanceof ConfigError ? err.message : err)
  process.exit(1)
}

const { client, db } = connect(config.databaseUrl)
// Self-hosted installs have no separate migration step: the API brings its
// schema up to date before it listens.
await migrateDb(db)

const app = await buildApp(config, { db })
app.addHook('onClose', async () => {
  await client.end()
})

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

try {
  await app.listen({ host: config.host, port: config.port })
} catch (err) {
  app.log.fatal({ err }, 'could not start')
  process.exit(1)
}
