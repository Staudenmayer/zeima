import { execFileSync, spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { EnvSchema } from '../src/config.ts'

// The Helm chart and the compose files must set what the API and the realm read; a rename in one place
// that is missed in the other shows up here.
const root = join(import.meta.dirname, '../..')
const chart = join(root, 'deploy/helm/zeima')
const haveHelm = spawnSync('helm', ['version']).status === 0
const realm = readFileSync(join(root, 'deploy/keycloak/realm-zeima.json'), 'utf8')
const compose = readFileSync(join(root, 'deploy/docker-compose.yml'), 'utf8')
// The defaults in the realm are for development (Swagger on, port 1025): production sets them all,
// except these two, whose default is right.
const KEEP_DEFAULT = new Set(['ZEIMA_SSL_REQUIRED', 'ZEIMA_SMTP_SSL'])
const placeholders = [...new Set([...realm.matchAll(/\$\{(ZEIMA_[A-Z_]+)(?::[^}]*)?\}/g)].map((m) => m[1]!))].filter((n) => !KEEP_DEFAULT.has(n))

describe('the realm placeholders', () => {
  it('are set by the compose file', () => {
    expect(placeholders.length).toBeGreaterThan(3)
    for (const name of placeholders) expect(compose, name).toContain(`${name}:`)
  })
})

describe.skipIf(!haveHelm)('the Helm chart', () => {
  const render = (sets: string[] = [], strings: string[] = []) =>
    execFileSync(
      'helm',
      ['template', 'zeima', chart, '--set', 'publicUrl=https://zeima.example.com', '--set', 'idUrl=https://id.zeima.example.com', ...sets.flatMap((s) => ['--set', s]), ...strings.flatMap((s) => ['--set-string', s])],
      { encoding: 'utf8' },
    )

  it('lints', () => {
    const out = execFileSync('helm', ['lint', chart, '--set', 'publicUrl=https://zeima.example.com', '--set', 'idUrl=https://id.zeima.example.com'], { encoding: 'utf8' })
    expect(out).toContain('0 chart(s) failed')
  })

  it('sets only environment variables the API reads', () => {
    const api = render().split('\n---\n').find((d) => d.includes('name: zeima-api') && d.includes('kind: Deployment'))!
    const names = [...api.matchAll(/- \{ name: ([A-Z_]+),|- name: ([A-Z_]+)\n/g)].map((m) => m[1] ?? m[2]!).filter((n) => n !== 'POSTGRES_PASSWORD')
    expect(names).toContain('DATABASE_URL')
    const known = new Set(Object.keys(EnvSchema.properties))
    for (const n of names) expect(known.has(n), `${n} is not an API setting`).toBe(true)
  })

  it('sets every placeholder of the realm for Keycloak', () => {
    const kc = render().split('\n---\n').find((d) => d.includes('name: zeima-keycloak') && d.includes('kind: Deployment'))!
    for (const name of placeholders) expect(kc, name).toContain(`name: ${name}`)
  })

  it('needs the two addresses', () => {
    const run = spawnSync('helm', ['template', 'zeima', chart], { encoding: 'utf8' })
    expect(run.status).not.toBe(0)
    expect(run.stderr).toContain('idUrl is required')
  })

  it('can use an external database, the addon registry and extra country profiles', () => {
    const out = render(['postgres.enabled=false', 'externalDatabase.host=db.example.com', 'addonRegistry.enabled=true'], ['jurisdictions.files.LU\\.toml=id = "LU"'])
    expect(out).not.toContain('kind: StatefulSet')
    expect(out).toContain('@db.example.com:5432/zeima')
    expect(out).toContain('ADDON_REGISTRY_DIR')
    expect(out).toContain('JURISDICTIONS_DIR')
    expect(out).toContain('LU.toml: |')
  })
})
