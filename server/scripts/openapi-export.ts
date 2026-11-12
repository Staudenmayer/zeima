// Writes the API contract to packages/api-contract/openapi.json. It is
// committed; a test fails when it differs from the routes.
//   bun run --cwd server openapi:export

import { writeFileSync } from 'node:fs'
import { buildApp } from '../src/app.ts'
import { loadConfig } from '../src/config.ts'

export const CONTRACT_PATH = new URL('../../packages/api-contract/openapi.json', import.meta.url)

/** The OpenAPI document with the default configuration, as committed. */
export async function exportOpenApi(): Promise<string> {
  const app = await buildApp(loadConfig({ NODE_ENV: 'production', METRICS_ENABLED: 'false' }), {
    logger: false,
    readiness: {},
  })
  await app.ready()
  const doc = JSON.stringify(app.swagger(), null, 2) + '\n'
  await app.close()
  return doc
}

if (import.meta.main) {
  writeFileSync(CONTRACT_PATH, await exportOpenApi())
  console.log(`wrote ${CONTRACT_PATH.pathname}`)
}
