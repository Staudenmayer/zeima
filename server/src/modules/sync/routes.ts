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
import { ensureOrgWorkspace } from '../orgs/service.ts'
import { checkOrgWrite, visibleToCaller } from './orgrules.ts'
import { type Change, type Json, merge, type Outcome, type RowState } from '../../sync/merge.ts'

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
  'meeting',
  'meeting_response',
] as const

const Uuid = Type.String({ format: 'uuid' })
const Hlc = Type.String({ pattern: '^[0-9]{15}-[0-9]{6}-[^\\s]+$', description: '`{wall_ms:015}-{counter:06}-{node}`' })

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

const MAX_CHANGES = 500
const MAX_PULL = 500

/** May the caller change rows of this workspace as a manager would (lock, edit locked)? */
function isManager(user: AuthUser, ws: { kind: string; orgId: string | null }): boolean {
  if (ws.kind === 'personal') return true
  const role = ws.orgId ? user.orgs[ws.orgId]?.role : undefined
  return role === 'owner' || role === 'admin' || role === 'manager'
}

type Tx = Db

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
      await app.licensing.require(db(), user, 'sync')
      return withUser(db(), callerOf(user), async (tx) => {
        const [ws] = await tx.select().from(schema.workspace).where(eq(schema.workspace.id, workspaceId))
        if (!ws) throw new HttpProblem(404, 'not_found', 'no such workspace')
        const actor = { manager: isManager(user, ws) }
        const orgActor = ws.kind === 'org' && ws.orgId ? { user, orgId: ws.orgId } : null

        const results: Outcome[] = []
        // Order matters inside a batch (a later change may depend on an earlier one).
        for (const change of changes) {
          const current = await loadRow(tx, workspaceId, change.table, change.rowId)
          const mergeable: Change = { ...change, fields: change.fields as Record<string, Json> | undefined }
          if (orgActor) {
            const refused = user.orgs[orgActor.orgId] ? await checkOrgWrite(tx, orgActor, orgActor.orgId, mergeable, current) : ({ status: 'rejected', reason: 'forbidden' } as const)
            if (refused) {
              results.push(refused)
              continue
            }
          }
          const { state, outcome } = merge(current, mergeable, actor)
          if (outcome.status === 'applied') {
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
