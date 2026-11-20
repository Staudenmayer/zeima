// Workspaces and sync (SPEC §6).
//
//   GET  /v1/workspaces       the workspaces the caller can use
//   POST /v1/workspaces       create the caller's personal workspace (idempotent)
//   POST /v1/sync/push        send changes; each is merged field by field (src/sync/merge.ts)
//   GET  /v1/sync/pull        rows changed since a cursor, in the workspaces the caller can see
//
// Access is checked twice: row-level security limits every query to the
// caller's workspaces, and the handlers apply the rules in the merge and the
// permission table. Sync state is kept in `sync_row` (see db/schema.ts).

import type { FastifyPluginAsyncTypebox } from '@fastify/type-provider-typebox'
import { and, asc, eq, gt, inArray, sql } from 'drizzle-orm'
import { Type } from 'typebox'
import { callerOf, type Db, schema, withUser } from '../../db/index.ts'
import type { AuthUser } from '../../plugins/auth.ts'
import { SECURITY } from '../../plugins/auth.ts'
import { HttpProblem, problems } from '../../plugins/errors.ts'
import { Quota } from '../../plugins/quota.ts'
import { typesAcceptable } from '../../sync/schema.ts'
import { ensureOrgWorkspace } from '../orgs/service.ts'
import { checkOrgWrite, visibleToCaller } from './orgrules.ts'
import { type Change, hlcWallMs, type Json, MAX_CLOCK_AHEAD_MS, merge, type Outcome, type RowState } from '../../sync/merge.ts'

/** Tables that sync, all scoped to a workspace. */
// Keep in step with SYNCED_TABLES in crates/zeima-core/src/repo/sync_outbox.rs.
export const SYNCED_TABLES = [
  'project',
  'tag',
  'time_entry',
  'pause',
  'activity',
  'cost_center',
  'cost_center_assignment',
  'jurisdiction_selection',
  'work_settings',
  'work_schedule',
  'absence_type',
  'absence',
  'custom_holiday',
  'balance_adjustment',
  'display_settings',
  'work_location',
  'org_policy',
  'office',
  'room',
  'meeting',
  'meeting_response',
] as const

const Uuid = Type.String({ format: 'uuid' })
const Hlc = Type.String({ pattern: '^[0-9]{15}-[0-9]{6}-[A-Za-z0-9._:-]{1,64}$', description: '`{wall_ms:015}-{counter:06}-{node}`' })

const ChangeSchema = Type.Object({
  table: Type.Union(SYNCED_TABLES.map((t) => Type.Literal(t))),
  rowId: Uuid,
  op: Type.Union([Type.Literal('upsert'), Type.Literal('delete')]),
  hlc: Hlc,
  fields: Type.Optional(Type.Record(Type.String(), Type.Any(), { description: 'Only the fields that changed' })),
})

const OutcomeSchema = Type.Union([
  Type.Object({ status: Type.Literal('applied') }),
  Type.Object({ status: Type.Literal('ignored') }),
  Type.Object({ status: Type.Literal('rejected'), reason: Type.String() }),
])

const WorkspaceSchema = Type.Object({
  id: Uuid,
  kind: Type.Union([Type.Literal('personal'), Type.Literal('org')]),
  name: Type.String(),
  orgId: Type.Union([Uuid, Type.Null()]),
})

const FieldsSchema = Type.Record(Type.String(), Type.Object({ v: Type.Any(), h: Type.String() }))

/** Personal workspaces one account may have (the app uses one). */
export const MAX_PERSONAL_WORKSPACES = 5
/** Advisory lock that keeps the commit order of pushes equal to the order of their sequence numbers. */
const PUSH_ORDER_LOCK = 7_240_002
const MAX_CHANGES = 500
/** What one account may push in a minute: far more than a person produces, far less than it takes to fill a disk. */
const PUSH_REQUESTS_PER_MINUTE = 120
const PUSH_CHANGES_PER_MINUTE = 5_000
const PULL_REQUESTS_PER_MINUTE = 600
const MAX_PULL = 500
/** Bytes one account may push in a minute, and the most one request, one field and one change may carry. */
const PUSH_BYTES_PER_MINUTE = 8 * 1024 * 1024
const MAX_PUSH_BODY = 2 * 1024 * 1024
const MAX_FIELD_BYTES = 64 * 1024
const MAX_CHANGE_BYTES = 256 * 1024
/** Field names of every synced table are plain camel-case words. */
const FIELD_NAME = /^[A-Za-z][A-Za-z0-9]{0,63}$/

/** What one account may write in a day: far more than a person produces, far less than a disk. */
const PUSH_BYTES_PER_DAY = 100 * 1024 * 1024

/** Whether a change carries only plausible fields: sane names, and values and totals within bounds. */
export function fieldsAcceptable(fields: Record<string, unknown> | undefined, table = ''): boolean {
  if (!fields) return true
  if (!typesAcceptable(table, fields)) return false
  let total = 0
  for (const [name, value] of Object.entries(fields)) {
    if (!FIELD_NAME.test(name)) return false
    const size = JSON.stringify(value ?? null).length
    total += size
    if (size > MAX_FIELD_BYTES || total > MAX_CHANGE_BYTES) return false
  }
  return true
}

/** May the caller change rows of this workspace as a manager would (lock, edit locked)? */
function isManager(user: AuthUser, ws: { kind: string; orgId: string | null }): boolean {
  if (ws.kind === 'personal') return true
  // Whoever may edit locked entries of others (some of them) acts as a manager here.
  const scope = ws.orgId ? user.orgs[ws.orgId]?.permissions['entry.write_locked'] : undefined
  return scope !== undefined
}

type Tx = Db

/** What one workspace may hold: rows and bytes. A person produces a few thousand rows a year; these are generous. */
export const WORKSPACE_LIMITS = { personal: { rows: 200_000, bytes: 256 * 1024 * 1024 }, org: { rows: 2_000_000, bytes: 1024 * 1024 * 1024 } }
const USAGE_TTL_MS = 30_000
const usageCache = new Map<string, { at: number; rows: number; bytes: number }>()

/** The rows and bytes a workspace holds (looked up at most every 30 seconds, and counted up in between). */
async function usageOf(tx: Tx, workspaceId: string, now = Date.now()) {
  let usage = usageCache.get(workspaceId)
  if (!usage || now - usage.at > USAGE_TTL_MS) {
    const [row] = await tx
      .select({ rows: sql<number>`count(*)::int`, bytes: sql<number>`coalesce(sum(pg_column_size(${schema.syncRow.fields})), 0)::bigint` })
      .from(schema.syncRow)
      .where(eq(schema.syncRow.workspaceId, workspaceId))
    usage = { at: now, rows: row?.rows ?? 0, bytes: Number(row?.bytes ?? 0) }
    usageCache.set(workspaceId, usage)
    if (usageCache.size > 5_000) usageCache.clear()
  }
  return usage
}

async function loadRow(tx: Tx, workspaceId: string, table: string, rowId: string): Promise<RowState | null> {
  const [row] = await tx
    .select()
    .from(schema.syncRow)
    .where(and(eq(schema.syncRow.workspaceId, workspaceId), eq(schema.syncRow.tableName, table), eq(schema.syncRow.rowId, rowId)))
    .for('update')
  return row ? { fields: row.fields as RowState['fields'], deleted: row.deleted } : null
}

async function saveRow(tx: Tx, workspaceId: string, table: string, rowId: string, state: RowState, exists: boolean) {
  if (exists) {
    await tx
      .update(schema.syncRow)
      .set({ fields: state.fields, deleted: state.deleted, seq: sql`nextval('sync_seq')` })
      .where(and(eq(schema.syncRow.workspaceId, workspaceId), eq(schema.syncRow.tableName, table), eq(schema.syncRow.rowId, rowId)))
  } else {
    await tx.insert(schema.syncRow).values({ workspaceId, tableName: table, rowId, fields: state.fields, deleted: state.deleted })
  }
}

export const syncRoutes: FastifyPluginAsyncTypebox = async (app) => {
  const pushRequests = new Quota(PUSH_REQUESTS_PER_MINUTE)
  const pushChanges = new Quota(PUSH_CHANGES_PER_MINUTE)
  const pushBytes = new Quota(PUSH_BYTES_PER_MINUTE)
  const pushBytesDay = new Quota(PUSH_BYTES_PER_DAY, 24 * 60 * 60_000)
  const pullRequests = new Quota(PULL_REQUESTS_PER_MINUTE)
  const db = () => {
    if (!app.db) throw new HttpProblem(503, 'unavailable', 'the database is not configured')
    return app.db
  }

  app.get(
    '/workspaces',
    {
      schema: {
        tags: ['sync'],
        summary: 'The workspaces the caller can use',
        description: 'A second device finds the personal workspace here and syncs into it instead of creating another.',
        security: SECURITY,
        response: { 200: Type.Array(WorkspaceSchema), ...problems(401, 403) },
      },
    },
    async (request) => {
      // Organizations made before shared workspaces existed get theirs here.
      const orgIds = Object.keys(request.user!.orgs)
      if (orgIds.length) {
        const known = await db().select({ id: schema.organization.id, name: schema.organization.name }).from(schema.organization).where(inArray(schema.organization.id, orgIds))
        for (const org of known) await ensureOrgWorkspace(db(), org.id, org.name)
      }
      return withUser(db(), callerOf(request.user!), async (tx) => {
        const rows = await tx.select().from(schema.workspace).orderBy(asc(schema.workspace.createdAt), asc(schema.workspace.id))
        return rows.map((w) => ({ id: w.id, kind: w.kind as 'personal' | 'org', name: w.name, orgId: w.orgId }))
      })
    },
  )

  app.post(
    '/workspaces',
    {
      schema: {
        tags: ['sync'],
        summary: 'Create the personal workspace (idempotent)',
        description: 'The first sync of a local workspace uploads it as the caller\'s personal workspace with this id.',
        security: SECURITY,
        body: Type.Object({ id: Uuid, name: Type.String({ minLength: 1, maxLength: 200 }) }),
        response: { 200: WorkspaceSchema, ...problems(401, 403, 409) },
      },
    },
    async (request) => {
      const user = request.user!
      const { id, name } = request.body
      await app.licensing.require(db(), user, 'sync')
      return withUser(db(), callerOf(user), async (tx) => {
        const [existing] = await tx.select().from(schema.workspace).where(eq(schema.workspace.id, id))
        if (existing) {
          if (existing.kind !== 'personal' || existing.ownerId !== user.id) {
            throw new HttpProblem(409, 'conflict', 'that workspace id is taken')
          }
          return { id, kind: 'personal' as const, name: existing.name, orgId: null }
        }
        // The app uses one personal workspace; a few more are allowed (a reinstall), not an unlimited number.
        const [owned] = await tx.select({ n: sql<number>`count(*)::int` }).from(schema.workspace).where(and(eq(schema.workspace.ownerId, user.id), eq(schema.workspace.kind, 'personal')))
        if ((owned?.n ?? 0) >= MAX_PERSONAL_WORKSPACES) throw new HttpProblem(409, 'conflict', 'that workspace id is taken')
        const now = Date.now()
        try {
          await tx.insert(schema.workspace).values({
            id,
            kind: 'personal',
            ownerId: user.id,
            name,
            createdAt: now,
            updatedAt: now,
            hlc: `${String(now).padStart(15, '0')}-000000-server`,
            originDevice: id,
          })
        } catch (err) {
          // Someone else owns this id; row-level security hid it from the lookup above.
          if ((err as { cause?: { code?: string } }).cause?.code === '23505') {
            throw new HttpProblem(409, 'conflict', 'that workspace id is taken')
          }
          throw err
        }
        return { id, kind: 'personal' as const, name, orgId: null }
      })
    },
  )

  app.post(
    '/sync/push',
    {
      bodyLimit: MAX_PUSH_BODY,
      schema: {
        tags: ['sync'],
        summary: 'Send changes to a workspace',
        description:
          'Each change is merged field by field (last write wins by HLC). The response has one result per change, in order. ' +
          'Rejected changes are not applied; the reason says why.',
        security: SECURITY,
        body: Type.Object({
          workspaceId: Uuid,
          deviceId: Uuid,
          changes: Type.Array(ChangeSchema, { maxItems: MAX_CHANGES }),
        }),
        response: { 200: Type.Object({ results: Type.Array(OutcomeSchema) }), ...problems(401, 403, 404) },
      },
    },
    async (request) => {
      const user = request.user!
      const { workspaceId, changes } = request.body
      pushRequests.spend(user.id, 1, 'pushes')
      pushChanges.spend(user.id, changes.length, 'changes')
      // The size of what was parsed, not the header (a chunked request has none).
      const size = Math.max(1, JSON.stringify(request.body).length)
      pushBytes.spend(user.id, size, 'bytes')
      pushBytesDay.spend(user.id, size, 'bytes today')
      await app.licensing.require(db(), user, 'sync')
      return withUser(db(), callerOf(user), async (tx) => {
        const [ws] = await tx.select().from(schema.workspace).where(eq(schema.workspace.id, workspaceId))
        if (!ws) throw new HttpProblem(404, 'not_found', 'no such workspace')
        // Pull pages by `seq`, which is taken when a row is written but only visible when its transaction commits. Pushes
        // therefore commit one at a time in the order of their numbers; otherwise a device could pass a number whose row
        // is not visible yet and never receive it. (One API process; a push is short.)
        await tx.execute(sql`select pg_advisory_xact_lock(${PUSH_ORDER_LOCK})`)
        const actor = { manager: isManager(user, ws) }
        const orgActor = ws.kind === 'org' && ws.orgId ? { user, orgId: ws.orgId } : null

        const results: Outcome[] = []
        const limits = ws.kind === 'org' ? WORKSPACE_LIMITS.org : WORKSPACE_LIMITS.personal
        const usage = await usageOf(tx, workspaceId)
        const horizon = Date.now() + MAX_CLOCK_AHEAD_MS
        // Order matters inside a batch (a later change may depend on an earlier one).
        for (const change of changes) {
          // A stamp from the far future would win every later write to its fields.
          if (hlcWallMs(change.hlc) > horizon) {
            results.push({ status: 'rejected', reason: 'clock_ahead' })
            continue
          }
          if (!fieldsAcceptable(change.fields, change.table)) {
            results.push({ status: 'rejected', reason: 'forbidden_field' })
            continue
          }
          const current = await loadRow(tx, workspaceId, change.table, change.rowId)
          // A full workspace takes no new rows (changes to rows it has still go through).
          if (current === null && change.op === 'upsert' && (usage.rows >= limits.rows || usage.bytes >= limits.bytes)) {
            results.push({ status: 'rejected', reason: 'workspace_full' })
            continue
          }
          const mergeable: Change = { ...change, fields: change.fields as Record<string, Json> | undefined }
          // Someone who may not change the lock writes `locked` as it already is (a new entry says `false`): that
          // changes nothing, and must not carry a stamp that would outrank the manager's later decision.
          if (!actor.manager && mergeable.table === 'time_entry' && mergeable.fields && 'locked' in mergeable.fields) {
            const effective = current?.fields.locked?.v === true
            if ((mergeable.fields.locked === true) === effective) {
              const { locked: _ignored, ...rest } = mergeable.fields
              mergeable.fields = rest
            }
          }
          if (orgActor) {
            const refused = user.orgs[orgActor.orgId] ? await checkOrgWrite(tx, orgActor, orgActor.orgId, mergeable, current) : ({ status: 'rejected', reason: 'forbidden' } as const)
            if (refused) {
              results.push(refused)
              continue
            }
          }
          const { state, outcome } = merge(current, mergeable, actor)
          if (outcome.status === 'applied') {
            if (current === null) usage.rows += 1
            usage.bytes += JSON.stringify(change.fields ?? {}).length
            await saveRow(tx, workspaceId, change.table, change.rowId, state, current !== null)
          }
          results.push(outcome)
        }
        return { results, wsOwner: ws.ownerId, wsOrg: ws.orgId }
      }).then(async ({ results, wsOwner, wsOrg }) => {
        // After the commit, so a device that pulls at once sees the changes.
        if (results.some((r) => r.status === 'applied')) {
          const users = new Set<string>()
          if (wsOwner) users.add(wsOwner)
          if (wsOrg) {
            const members = await db().select({ id: schema.kcOrgMember.userId }).from(schema.kcOrgMember).where(eq(schema.kcOrgMember.orgId, wsOrg))
            for (const m of members) users.add(m.id)
          }
          app.hub.notify(users, request.body.deviceId)
        }
        return { results }
      })
    },
  )

  app.get(
    '/sync/pull',
    {
      schema: {
        tags: ['sync'],
        summary: 'Rows changed since a cursor',
        description:
          'Returns the merged state of rows changed after `cursor`, oldest first, across all workspaces the caller can see. ' +
          'Merge each field into the local row with the same rule as push. Pass the returned `cursor` next time; `more` says to ask again.',
        security: SECURITY,
        querystring: Type.Object({
          cursor: Type.Integer({ minimum: 0, default: 0 }),
          limit: Type.Integer({ minimum: 1, maximum: MAX_PULL, default: MAX_PULL }),
        }),
        response: {
          200: Type.Object({
            rows: Type.Array(
              Type.Object({
                workspaceId: Uuid,
                table: Type.String(),
                rowId: Uuid,
                fields: FieldsSchema,
                deleted: Type.Union([Type.String(), Type.Null()]),
              }),
            ),
            cursor: Type.Integer(),
            more: Type.Boolean(),
          }),
          ...problems(401, 403),
        },
      },
    },
    async (request) => {
      const { cursor, limit } = request.query
      pullRequests.spend(request.user!.id, 1, 'pulls')
      await app.licensing.require(db(), request.user!, 'sync')
      return withUser(db(), callerOf(request.user!), async (tx) => {
        const rows = await tx
          .select()
          .from(schema.syncRow)
          .where(gt(schema.syncRow.seq, cursor))
          .orderBy(asc(schema.syncRow.seq))
          .limit(limit + 1)
        const page = rows.slice(0, limit)
        const orgWorkspaces = new Map(
          (await tx.select({ id: schema.workspace.id, orgId: schema.workspace.orgId }).from(schema.workspace).where(eq(schema.workspace.kind, 'org'))).map((w) => [w.id, w.orgId!]),
        )
        const candidates = page.map((r) => ({ ...r, table: r.tableName, fields: r.fields as RowState['fields'] }))
        // What this person may see of the rows, some of them trimmed (see orgrules.ts).
        const visible = orgWorkspaces.size ? await visibleToCaller(tx, request.user!, orgWorkspaces, candidates) : candidates
        return {
          rows: visible.map((r) => ({
            workspaceId: r.workspaceId,
            table: r.tableName,
            rowId: r.rowId,
            fields: r.fields,
            deleted: r.deleted,
          })),
          cursor: page.length ? page[page.length - 1]!.seq : cursor,
          more: rows.length > limit,
        }
      })
    },
  )
}
