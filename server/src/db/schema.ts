// Server database schema (Drizzle, PostgreSQL).
//
// Two groups of tables:
//  - Synced tables mirror the client's SQLite schema
//    (crates/zeima-core/migrations): the same columns and names, with uuid ids
//    and jsonb in place of text ids and JSON text. Times stay unix ms (bigint)
//    so the client and server compare values without conversion.
//  - Server-only tables: organizations, the Keycloak mirror (`kc_*`),
//    devices, entitlements and the audit log.
//
// Row-level security is added in the migration after the tables
// (drizzle/0001_rls.sql). Keep the two in sync when adding a table.

import { sql } from 'drizzle-orm'
import {
  bigint,
  bigserial,
  boolean,
  check,
  index,
  integer,
  jsonb,
  pgSequence,
  pgTable,
  primaryKey,
  text,
  uniqueIndex,
  uuid,
} from 'drizzle-orm/pg-core'

const ms = (name: string) => bigint(name, { mode: 'number' })

/** Columns every synced row carries (SPEC §4). */
const synced = () => ({
  createdAt: ms('created_at').notNull(),
  updatedAt: ms('updated_at').notNull(),
  /** Tombstone: deleted rows stay so other devices learn about the delete. */
  deletedAt: ms('deleted_at'),
  /** Hybrid logical clock of the last write; decides field-level conflicts. */
  hlc: text('hlc').notNull(),
  originDevice: uuid('origin_device').notNull(),
})

// ---------------------------------------------------------------- server only

export const organization = pgTable('organization', {
  /** Same id as the Keycloak organization. */
  id: uuid('id').primaryKey(),
  name: text('name').notNull(),
  createdAt: ms('created_at').notNull(),
})

/** Mirror of the Keycloak user, refreshed from tokens and webhooks. */
export const kcUser = pgTable('kc_user', {
  /** Keycloak `sub`. */
  id: uuid('id').primaryKey(),
  email: text('email'),
  displayName: text('display_name'),
  locale: text('locale'),
  active: boolean('active').notNull().default(true),
  lastSeenAt: ms('last_seen_at'),
})

export const kcGroup = pgTable(
  'kc_group',
  {
    id: uuid('id').primaryKey(),
    orgId: uuid('org_id')
      .notNull()
      .references(() => organization.id, { onDelete: 'cascade' }),
    /** Full Keycloak path, e.g. `/orgs/<orgId>/groups/dev/backend`. */
    path: text('path').notNull(),
  },
  (t) => [uniqueIndex('kc_group_path').on(t.path)],
)

export const kcGroupMember = pgTable(
  'kc_group_member',
  {
    groupId: uuid('group_id')
      .notNull()
      .references(() => kcGroup.id, { onDelete: 'cascade' }),
    userId: uuid('user_id')
      .notNull()
      .references(() => kcUser.id, { onDelete: 'cascade' }),
  },
  (t) => [primaryKey({ columns: [t.groupId, t.userId] })],
)

export const kcOrgMember = pgTable(
  'kc_org_member',
  {
    orgId: uuid('org_id')
      .notNull()
      .references(() => organization.id, { onDelete: 'cascade' }),
    userId: uuid('user_id')
      .notNull()
      .references(() => kcUser.id, { onDelete: 'cascade' }),
    role: text('role').notNull(),
  },
  (t) => [
    primaryKey({ columns: [t.orgId, t.userId] }),
    check('kc_org_member_role', sql`${t.role} IN ('owner', 'admin', 'manager', 'member')`),
  ],
)

/** An invitation that has been sent and not yet accepted; the role is given when the invitee signs in. */
export const orgInvitation = pgTable(
  'org_invitation',
  {
    id: uuid('id').primaryKey().defaultRandom(),
    orgId: uuid('org_id')
      .notNull()
      .references(() => organization.id, { onDelete: 'cascade' }),
    /** Lower case. */
    email: text('email').notNull(),
    role: text('role').notNull(),
    invitedBy: uuid('invited_by'),
    createdAt: ms('created_at').notNull(),
    acceptedAt: ms('accepted_at'),
  },
  (t) => [
    uniqueIndex('org_invitation_pending').on(t.orgId, t.email).where(sql`${t.acceptedAt} IS NULL`),
    check('org_invitation_role', sql`${t.role} IN ('admin', 'manager', 'member')`),
  ],
)

export const device = pgTable(
  'device',
  {
    /** The client's per-install `device_id`. */
    id: uuid('id').primaryKey(),
    userId: uuid('user_id')
      .notNull()
      .references(() => kcUser.id, { onDelete: 'cascade' }),
    name: text('name').notNull(),
    platform: text('platform').notNull(),
    appVersion: text('app_version'),
    createdAt: ms('created_at').notNull(),
    lastSeenAt: ms('last_seen_at'),
  },
  (t) => [index('device_user').on(t.userId)],
)

/** What a user or an organization may use (licensing, SPEC §6). */
export const entitlement = pgTable(
  'entitlement',
  {
    id: uuid('id').primaryKey(),
    userId: uuid('user_id').references(() => kcUser.id, { onDelete: 'cascade' }),
    orgId: uuid('org_id').references(() => organization.id, { onDelete: 'cascade' }),
    plan: text('plan').notNull(),
    seats: integer('seats'),
    features: jsonb('features').notNull().default({}),
    validUntil: ms('valid_until'),
    /** A vendor-signed license token (self-hosted); served to clients as it is. */
    token: text('token'),
    /** Subscription details from the billing provider (cloud). */
    billing: jsonb('billing'),
    updatedAt: ms('updated_at').notNull(),
  },
  (t) => [check('entitlement_subject', sql`(${t.userId} IS NULL) <> (${t.orgId} IS NULL)`)],
)

export const auditLog = pgTable(
  'audit_log',
  {
    id: bigserial('id', { mode: 'number' }).primaryKey(),
    orgId: uuid('org_id').references(() => organization.id, { onDelete: 'cascade' }),
    actorId: uuid('actor_id'),
    action: text('action').notNull(),
    target: text('target'),
    detail: jsonb('detail').notNull().default({}),
    at: ms('at').notNull(),
  },
  (t) => [index('audit_log_org_at').on(t.orgId, t.at)],
)

// --------------------------------------------------------------------- synced

/**
 * A workspace is owned by one user (`kind = 'personal'`) or by an
 * organization (`kind = 'org'`). Row-level security keys on these two columns.
 */
export const workspace = pgTable(
  'workspace',
  {
    id: uuid('id').primaryKey(),
    kind: text('kind').notNull(),
    ownerId: uuid('owner_id').references(() => kcUser.id),
    orgId: uuid('org_id').references(() => organization.id),
    name: text('name').notNull(),
    settings: jsonb('settings').notNull().default({}),
    ...synced(),
  },
  (t) => [
    check('workspace_kind', sql`${t.kind} IN ('personal', 'org')`),
    check(
      'workspace_owner',
      sql`(${t.kind} = 'personal' AND ${t.ownerId} IS NOT NULL AND ${t.orgId} IS NULL)
          OR (${t.kind} = 'org' AND ${t.orgId} IS NOT NULL AND ${t.ownerId} IS NULL)`,
    ),
  ],
)

const workspaceId = () =>
  uuid('workspace_id')
    .notNull()
    .references(() => workspace.id)

export const project = pgTable(
  'project',
  {
    id: uuid('id').primaryKey(),
    workspaceId: workspaceId(),
    name: text('name').notNull(),
    alias: text('alias'),
    color: text('color').notNull(),
    parentId: uuid('parent_id'),
    client: text('client'),
    budgetMinutes: integer('budget_minutes'),
    budgetPeriod: text('budget_period').notNull().default('total'),
    budgetWarnPct: integer('budget_warn_pct').notNull().default(80),
    billableDefault: boolean('billable_default').notNull().default(false),
    status: text('status').notNull().default('active'),
    ...synced(),
  },
  (t) => [
    uniqueIndex('project_alias_unique')
      .on(t.workspaceId, sql`lower(${t.alias})`)
      .where(sql`${t.alias} IS NOT NULL AND ${t.deletedAt} IS NULL`),
    check('project_budget_period', sql`${t.budgetPeriod} IN ('total', 'monthly', 'weekly')`),
    check('project_status', sql`${t.status} IN ('active', 'paused', 'archived')`),
  ],
)

export const tag = pgTable(
  'tag',
  {
    id: uuid('id').primaryKey(),
    workspaceId: workspaceId(),
    name: text('name').notNull(),
    color: text('color').notNull(),
    projectId: uuid('project_id').references(() => project.id),
    groupName: text('group_name'),
    ...synced(),
  },
  (t) => [
    uniqueIndex('tag_name_unique')
      .on(t.workspaceId, sql`coalesce(${t.projectId}::text, '')`, sql`lower(${t.name})`)
      .where(sql`${t.deletedAt} IS NULL`),
  ],
)

export const timeEntry = pgTable(
  'time_entry',
  {
    id: uuid('id').primaryKey(),
    workspaceId: workspaceId(),
    userId: uuid('user_id')
      .notNull()
      .references(() => kcUser.id),
    projectId: uuid('project_id').references(() => project.id),
    startAt: ms('start_at').notNull(),
    endAt: ms('end_at'),
    tz: text('tz').notNull(),
    note: text('note').notNull().default(''),
    billable: boolean('billable').notNull().default(false),
    source: text('source').notNull(),
    sourceRef: text('source_ref'),
    locked: boolean('locked').notNull().default(false),
    ...synced(),
  },
  (t) => [
    index('time_entry_range')
      .on(t.userId, t.startAt)
      .where(sql`${t.deletedAt} IS NULL`),
    // At most one running entry per user.
    uniqueIndex('time_entry_one_running')
      .on(t.userId)
      .where(sql`${t.endAt} IS NULL AND ${t.deletedAt} IS NULL`),
    check('time_entry_range_ok', sql`${t.endAt} IS NULL OR ${t.endAt} >= ${t.startAt}`),
  ],
)

/** Tags of an entry; sync treats the tag set as one field of the entry. */
export const timeEntryTag = pgTable(
  'time_entry_tag',
  {
    entryId: uuid('entry_id')
      .notNull()
      .references(() => timeEntry.id, { onDelete: 'cascade' }),
    tagId: uuid('tag_id')
      .notNull()
      .references(() => tag.id),
  },
  (t) => [primaryKey({ columns: [t.entryId, t.tagId] })],
)

export const pause = pgTable(
  'pause',
  {
    id: uuid('id').primaryKey(),
    workspaceId: workspaceId(),
    userId: uuid('user_id')
      .notNull()
      .references(() => kcUser.id),
    startAt: ms('start_at').notNull(),
    endAt: ms('end_at'),
    kind: text('kind').notNull(),
    ruleId: text('rule_id'),
    accepted: boolean('accepted'),
    resumeEntryId: uuid('resume_entry_id').references(() => timeEntry.id),
    ...synced(),
  },
  (t) => [
    index('pause_range')
      .on(t.userId, t.startAt)
      .where(sql`${t.deletedAt} IS NULL`),
    uniqueIndex('pause_one_open')
      .on(t.userId)
      .where(sql`${t.endAt} IS NULL AND ${t.deletedAt} IS NULL`),
    check('pause_range_ok', sql`${t.endAt} IS NULL OR ${t.endAt} >= ${t.startAt}`),
  ],
)

// ----------------------------------------------------------------------- sync

/** Orders changes for pull cursors; one sequence for all workspaces. */
export const syncSeq = pgSequence('sync_seq')

/**
 * The merged state of every synced row, as the sync merge keeps it
 * (src/sync/merge.ts): each field with the HLC that set it, and the latest
 * delete. This is the source of truth for pull. The typed tables above are
 * filled from it for server-side reports and are not read by sync.
 */
export const syncRow = pgTable(
  'sync_row',
  {
    workspaceId: workspaceId(),
    tableName: text('table_name').notNull(),
    rowId: uuid('row_id').notNull(),
    /** `{ [field]: { v, h } }` */
    fields: jsonb('fields').notNull().default({}),
    deleted: text('deleted'),
    /** Set to the next sequence value whenever the state changes. */
    seq: bigint('seq', { mode: 'number' })
      .notNull()
      .default(sql`nextval('sync_seq')`),
  },
  (t) => [
    primaryKey({ columns: [t.workspaceId, t.tableName, t.rowId] }),
    index('sync_row_seq').on(t.seq),
  ],
)
