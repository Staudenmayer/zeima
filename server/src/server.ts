// Entry point: `node src/server.ts` (Node 24 runs the TypeScript directly).

import { buildApp } from './app.ts'
import { ConfigError, loadConfig } from './config.ts'

let config
try {
  config = loadConfig()
} catch (err) {
  // Fail fast with every problem listed (SPEC §7.1).
  console.error(err instanceof ConfigError ? err.message : err)
  process.exit(1)
}

const app = await buildApp(config)

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
