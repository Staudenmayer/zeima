// The example files must list every setting, so people can start from them.
// They fail when a setting is added without being described.

import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { EnvSchema, loadConfig } from '../src/config.ts'

const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8')

/** `NAME=value` lines, commented out or not. */
const names = (text: string) => new Set([...text.matchAll(/^#?([A-Z][A-Z0-9_]+)=/gm)].map((m) => m[1]!))

describe('server/.env.example', () => {
  const text = read('../.env.example')

  it('describes every setting of the API, and nothing the API does not read', () => {
    const listed = names(text)
    listed.delete('TEST_DATABASE_URL') // read by the tests, not by the API
    expect([...listed].sort()).toEqual(Object.keys(EnvSchema.properties).sort())
  })

  it('is a valid configuration as it stands', () => {
    const env: Record<string, string> = {}
    for (const m of text.matchAll(/^([A-Z][A-Z0-9_]+)=([^#\n]*?)\s*(?:#.*)?$/gm)) if (m[2]) env[m[1]!] = m[2]
    const config = loadConfig(env)
    expect(config.keycloak.admin).not.toBeNull() // organizations work out of the box in development
  })
})

describe('deploy/.env.example', () => {
  it('describes every variable docker-compose.yml takes from it', () => {
    const compose = read('../../deploy/docker-compose.yml')
    const used = new Set([...compose.matchAll(/\$\{([A-Z][A-Z0-9_]+)/g)].map((m) => m[1]!))
    const listed = names(read('../../deploy/.env.example'))
    expect([...used].filter((n) => !listed.has(n)).sort(), 'missing from deploy/.env.example').toEqual([])
    expect([...listed].filter((n) => !used.has(n)).sort(), 'listed but not used by the compose file').toEqual([])
  })
})
