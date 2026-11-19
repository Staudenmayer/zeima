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
      ['template', 'zeima', chart, '--set', 'publicUrl=https://zeima.example.com', '--set', 'idUrl=https://id.zeima.example.com', '--set', 'smtp.host=mail.example.com', '--set', 'keycloak.registrationAllowed=true', ...sets.flatMap((s) => ['--set', s]), ...strings.flatMap((s) => ['--set-string', s])],
      { encoding: 'utf8' },
    )

  it('lints', () => {
    const out = execFileSync('helm', ['lint', chart, '--set', 'publicUrl=https://zeima.example.com', '--set', 'idUrl=https://id.zeima.example.com', '--set', 'smtp.host=mail.example.com', '--set', 'keycloak.registrationAllowed=true'], { encoding: 'utf8' })
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

  it('does not publish Keycloak\'s administration console or the master realm on the sign-in host', () => {
    const ingress = render().split('\n---\n').find((d) => d.includes('kind: Ingress'))!
    const idRule = ingress.slice(ingress.indexOf('host: "id.zeima.example.com"'))
    expect(idRule).toContain('path: /realms/zeima')
    expect(idRule).toContain('path: /resources')
    expect(idRule).not.toMatch(/path: \/\n/)
    expect(idRule).not.toContain('/admin')
    // The API host: nothing for the internal port (3001), which the ingress never routes to.
    expect(ingress).not.toContain('3001')
    expect(render(['exposeKeycloakAdmin=true']).split('\n---\n').find((d) => d.includes('kind: Ingress'))!).toMatch(/host: "id.zeima.example.com"[\s\S]*path: \//)
  })

  it('keeps Keycloak\'s events off the public API port and limits who talks to whom', () => {
    const out = render()
    expect(out).toContain('name: INTERNAL_PORT')
    expect(out).toContain('keycloak-events')
    expect(out).toContain(':3001/v1/internal/keycloak-events')
    expect(out).toMatch(/kind: NetworkPolicy[\s\S]*default-deny/)
    expect((out.match(/kind: NetworkPolicy/g) ?? []).length).toBe(6)
    expect(render(['networkPolicy.enabled=false'])).not.toContain('kind: NetworkPolicy')
  })

  it('lets the API and Keycloak reach only public addresses outside the release, and what the administrator lists', () => {
    const out = render()
    expect(out).toContain('name: zeima-api-egress')
    expect(out).toContain('name: zeima-keycloak-egress')
    for (const cidr of ['10.0.0.0/8', '172.16.0.0/12', '192.168.0.0/16', '169.254.0.0/16']) expect(out).toContain(`"${cidr}"`)
    // The API: DNS, Keycloak, the database and public https. Keycloak: also the mail port.
    const api = out.split('\n---\n').find((d) => d.includes('name: zeima-api-egress'))!
    expect(api).toContain('port: 443')
    expect(api).not.toContain('port: 25')
    expect(render(['networkPolicy.egressAllowCidrs={10.1.0.0/24}'])).toContain('cidr: "10.1.0.0/24"')
    expect(render(['networkPolicy.restrictEgress=false'])).not.toContain('-egress')
  })

  it('asks people to confirm their address, and refuses to install that way without a mail server', () => {
    expect(render()).toMatch(/name: ZEIMA_VERIFY_EMAIL, value: "true"/)
    expect(render()).toMatch(/name: ZEIMA_REQUIRE_MFA, value: "false"/)
    const base = ['template', 'zeima', chart, '--set', 'publicUrl=https://zeima.example.com', '--set', 'idUrl=https://id.zeima.example.com', '--set', 'keycloak.registrationAllowed=true']
    const run = spawnSync('helm', base, { encoding: 'utf8' })
    expect(run.status).not.toBe(0)
    expect(run.stderr).toContain('smtp.host')
    expect(spawnSync('helm', [...base, '--set', 'keycloak.verifyEmail=false'], { encoding: 'utf8' }).status).toBe(0)
  })

  it('makes closed or open registration an explicit decision', () => {
    const undecided = spawnSync('helm', ['template', 'zeima', chart, '--set', 'publicUrl=https://zeima.example.com', '--set', 'idUrl=https://id.zeima.example.com', '--set', 'smtp.host=mail.example.com'], { encoding: 'utf8' })
    expect(undecided.status).not.toBe(0)
    expect(undecided.stderr).toContain('registrationAllowed')
    expect(render(['keycloak.registrationAllowed=false'])).toMatch(/name: ZEIMA_REGISTRATION_ALLOWED, value: "false"/)
    expect(compose).toMatch(/REGISTRATION_ALLOWED:\?decide/)
  })

  it('runs every pod without privileges it does not need', () => {
    const out = render()
    for (const kind of ['zeima-api', 'zeima-keycloak', 'zeima-postgres']) {
      const doc = out.split('\n---\n').find((d) => d.includes(`name: ${kind}`) && /kind: (Deployment|StatefulSet)/.test(d))!
      expect(doc, kind).toContain('allowPrivilegeEscalation: false')
      expect(doc, kind).toContain('runAsNonRoot: true')
      expect(doc, kind).toContain('capabilities: { drop: ["ALL"] }')
    }
  })

  it('pins the PostgreSQL image by digest', () => {
    expect(render()).toMatch(/image: docker\.io\/library\/postgres:17@sha256:[0-9a-f]{64}/)
  })
})

describe('the compose file and the proxy', () => {
  const caddy = readFileSync(join(root, 'deploy/Caddyfile'), 'utf8')
  const airgap = readFileSync(join(root, 'deploy/Caddyfile.airgap'), 'utf8')

  it('publish only the realm and its static files on the sign-in host', () => {
    for (const file of [caddy, airgap]) {
      const idBlock = file.slice(file.indexOf('id.{$DOMAIN}')).split('\n').filter((l) => !l.trim().startsWith('#')).join('\n')
      expect(idBlock).toContain('@signin path /realms/zeima /realms/zeima/* /resources/*')
      expect(idBlock).toContain('respond 404')
      expect(idBlock).not.toMatch(/\/admin|\/realms\/master/)
    }
  })

  it('do not publish the metrics, and send Keycloak\'s events to the internal port', () => {
    expect(caddy).toContain('respond @metrics 404')
    expect(compose).toContain('api:3001/v1/internal/keycloak-events')
    expect(compose).toContain('INTERNAL_PORT: "3001"')
  })

  it('check email addresses by default, trust one proxy, and restrict the containers', () => {
    expect(compose).toContain('VERIFY_EMAIL:-true')
    expect(compose).toContain('REQUIRE_MFA:-false')
    expect(compose).toContain('TRUST_PROXY: "1"')
    expect((compose.match(/no-new-privileges:true/g) ?? []).length).toBeGreaterThanOrEqual(4)
    expect(compose).toMatch(/postgres:17@sha256:[0-9a-f]{64}/)
    expect(compose).toMatch(/caddy:2@sha256:[0-9a-f]{64}/)
    expect(compose).toContain('APP_DATABASE_PASSWORD')
    // The database has no way out: it is on an internal network, with only the API and Keycloak joined to it.
    expect(compose).toMatch(/networks:\n  db:\n    internal: true/)
    expect(compose.match(/networks: \[(?:default, )?db\]/g)?.length).toBe(3)
  })
})

