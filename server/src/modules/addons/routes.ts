// The addon registry and the organizations' addon rules (Phase 7).
//
//   GET /v1/addons/registry                 the addons this server offers, and the publishers whose signatures to trust
//   GET /v1/addons/registry/files/:file     one package (.zip)
//   GET /v1/orgs/:orgId/addons              which addons the organization allows (every member may read it)
//   PUT /v1/orgs/:orgId/addons              change it (`org.settings`)
//
// The registry is a folder (`ADDON_REGISTRY_DIR`) made by `node scripts/addons.mjs publish`: signed
// packages and an `index.json`. The server only serves it; the app checks sizes, hashes and signatures
// itself, so a damaged or swapped file is caught there. The rules are enforced by the app, which
// refuses to install or run what an organization it belongs to does not allow.

import { readFile, stat } from 'node:fs/promises'
import { join } from 'node:path'
import type { FastifyPluginAsyncTypebox } from '@fastify/type-provider-typebox'
import { eq } from 'drizzle-orm'
import { type Static, Type } from 'typebox'
import type { Config } from '../../config.ts'
import { schema } from '../../db/index.ts'
import { SECURITY } from '../../plugins/auth.ts'
import { authorize } from '../../plugins/authz.ts'
import { HttpProblem, problems } from '../../plugins/errors.ts'
import { audit } from '../orgs/service.ts'

const OrgParams = Type.Object({ orgId: Type.String({ format: 'uuid' }) })

const Entry = Type.Object({
  id: Type.String(),
  name: Type.String(),
  version: Type.String(),
  description: Type.String(),
  author: Type.String(),
  minAppVersion: Type.Union([Type.String(), Type.Null()]),
  permissions: Type.Array(Type.String()),
  hooks: Type.Array(Type.String()),
  file: Type.String(),
  sha256: Type.String(),
  size: Type.Integer(),
  keyId: Type.Union([Type.String(), Type.Null()], { description: 'The publisher key that signed the package, if signed' }),
})

const Registry = Type.Object(
  {
    publishers: Type.Array(Type.Object({ keyId: Type.String(), name: Type.String(), publicKey: Type.String({ description: 'Ed25519, 32 bytes, base64url' }) })),
    addons: Type.Array(Entry),
  },
  { description: 'What the server offers; an empty list when no registry folder is set up' },
)

const Policy = Type.Object(
  {
    mode: Type.Union([Type.Literal('open'), Type.Literal('allowlist')], { description: '`open`: any addon; `allowlist`: only `allowed`' }),
    requireSigned: Type.Boolean({ description: 'Only addons signed by a publisher the app trusts' }),
    allowed: Type.Array(Type.String({ maxLength: 120 }), { maxItems: 200, description: 'Addon ids (used when mode is `allowlist`)' }),
  },
  { description: 'The addons an organization lets its people use' },
)

const FILE = /^[A-Za-z0-9][A-Za-z0-9._-]{0,150}\.zip$/
const ADDON_ID = /^[a-z0-9]+(\.[a-z0-9-]+)+$/

export const addonRoutes: FastifyPluginAsyncTypebox<{ config: Config }> = async (app, { config }) => {
  const dir = config.addonRegistryDir
  let cached: { mtime: number; value: Static<typeof Registry> } | null = null

  async function registry(): Promise<Static<typeof Registry>> {
    if (!dir) return { publishers: [], addons: [] }
    const path = join(dir, 'index.json')
    let mtime: number
    try {
      mtime = (await stat(path)).mtimeMs
    } catch {
      return { publishers: [], addons: [] }
    }
    if (cached?.mtime === mtime) return cached.value
    let raw: { publishers?: unknown; addons?: unknown }
    try {
      raw = JSON.parse(await readFile(path, 'utf8'))
    } catch (err) {
      app.log.warn({ err }, 'the addon registry index is not valid JSON')
      return { publishers: [], addons: [] }
    }
    const list = <T>(v: unknown): T[] => (Array.isArray(v) ? (v as T[]) : [])
    const value = {
      publishers: list<(Static<typeof Registry>)['publishers'][number]>(raw.publishers).filter((p) => p && typeof p.keyId === 'string' && typeof p.publicKey === 'string' && typeof p.name === 'string'),
      // An entry that points outside the folder, or lacks what the app checks, is not offered.
      addons: list<(Static<typeof Entry>)>(raw.addons).filter(
        (a) => a && typeof a.id === 'string' && ADDON_ID.test(a.id) && typeof a.file === 'string' && FILE.test(a.file) && typeof a.sha256 === 'string' && typeof a.version === 'string',
      ).map((a) => ({
        id: a.id,
        name: String(a.name ?? a.id),
        version: a.version,
        description: String(a.description ?? ''),
        author: String(a.author ?? ''),
        minAppVersion: typeof a.minAppVersion === 'string' ? a.minAppVersion : null,
        permissions: list<string>(a.permissions).map(String),
        hooks: list<string>(a.hooks).map(String),
        file: a.file,
        sha256: a.sha256,
        size: Number(a.size) || 0,
        keyId: typeof a.keyId === 'string' ? a.keyId : null,
      })),
    }
    cached = { mtime, value }
    return value
  }

  app.get(
    '/addons/registry',
    { schema: { tags: ['addons'], summary: 'The addons this server offers', security: SECURITY, response: { 200: Registry, ...problems(401, 403) } } },
    async () => registry(),
  )

  app.get(
    '/addons/registry/files/:file',
    {
      schema: {
        tags: ['addons'],
        summary: 'One addon package',
        description: 'A .zip; only files listed in the registry are served.',
        security: SECURITY,
        params: Type.Object({ file: Type.String({ maxLength: 160 }) }),
        response: { 200: Type.Any(), ...problems(401, 403, 404) },
      },
    },
    async (request, reply) => {
      const { file } = request.params
      if (!dir || !FILE.test(file) || !(await registry()).addons.some((a) => a.file === file)) throw new HttpProblem(404, 'not_found', 'there is no such package')
      const bytes = await readFile(join(dir, file)).catch(() => {
        throw new HttpProblem(404, 'not_found', 'there is no such package')
      })
      return reply.type('application/zip').header('cache-control', 'private, max-age=300').send(bytes)
    },
  )

  const database = () => {
    if (!app.db) throw new HttpProblem(503, 'unavailable', 'the database is not set up')
    return app.db
  }

  const view = async (orgId: string): Promise<Static<typeof Policy>> => {
    const [row] = await database().select().from(schema.orgAddonPolicy).where(eq(schema.orgAddonPolicy.orgId, orgId))
    return row ? { mode: row.mode === 'allowlist' ? 'allowlist' : 'open', requireSigned: row.requireSigned, allowed: row.allowed } : { mode: 'open', requireSigned: false, allowed: [] }
  }

  app.get(
    '/orgs/:orgId/addons',
    {
      schema: {
        tags: ['addons'],
        summary: 'Which addons the organization allows',
        security: SECURITY,
        params: OrgParams,
        response: { 200: Policy, ...problems(401, 403) },
      },
    },
    async (request) => {
      const { orgId } = request.params
      if (!request.user!.orgs[orgId]) throw new HttpProblem(403, 'org_forbidden', 'you are not a member of this organization')
      return view(orgId)
    },
  )

  app.put(
    '/orgs/:orgId/addons',
    {
      schema: {
        tags: ['addons'],
        summary: 'Changes which addons the organization allows',
        security: SECURITY,
        params: OrgParams,
        body: Policy,
        response: { 200: Policy, ...problems(400, 401, 403) },
      },
    },
    async (request) => {
      const { orgId } = request.params
      const user = request.user!
      if (!user.orgs[orgId]) throw new HttpProblem(403, 'org_forbidden', 'you are not a member of this organization')
      authorize({ user, orgId }, 'org.settings', { orgId })
      const allowed = [...new Set(request.body.allowed.map((a) => a.trim().toLowerCase()).filter(Boolean))].sort()
      const bad = allowed.find((a) => !ADDON_ID.test(a))
      if (bad) throw new HttpProblem(400, 'bad_request', `"${bad}" is not an addon id (like com.example.hours)`)
      const db = database()
      const values = { orgId, mode: request.body.mode, requireSigned: request.body.requireSigned, allowed, updatedAt: Date.now() }
      await db.insert(schema.orgAddonPolicy).values(values).onConflictDoUpdate({ target: schema.orgAddonPolicy.orgId, set: values })
      await audit(db, { orgId, actorId: user.id, action: 'addons.policy', detail: { mode: values.mode, requireSigned: values.requireSigned, allowed } })
      return view(orgId)
    },
  )
}
