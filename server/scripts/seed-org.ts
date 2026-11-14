// Fills an organization of a local development setup with demo data: people
// in every role and in departments, the organization's activities, projects,
// tags and cost centers, three weeks of time entries per person, absences
// (open and decided) and meetings with answers. Safe to run again: every row
// has a fixed id, so a second run updates instead of duplicating.
//
//   cd server
//   ORG_ID=<organization id> OWNER_ID=<your user id> \
//   node --env-file=.env scripts/seed-org.ts
//
// Needs the API, Keycloak (admin password `admin` as in deploy/dev.env.example,
// override with KEYCLOAK_ADMIN_PASSWORD) and Postgres of `deploy/compose.dev.yml`.
// The demo people sign in with `seed-<name>@example.com` and the password
// printed at the end. Your own rows are not touched; the meetings invite you,
// so they show up in your timeline.

import { createHash, randomUUID } from 'node:crypto'
// @ts-expect-error plain JS helpers of the end-to-end checks
import { adminToken, json, signIn } from '../../deploy/e2e/lib.mjs'
import { loadConfig } from '../src/config.ts'
import { connect } from '../src/db/index.ts'
import { HttpKeycloakAdmin } from '../src/keycloak/admin.ts'
import { reconcileOrganization } from '../src/modules/orgs/reconcile.ts'

const ORG_ID = process.env.ORG_ID
const OWNER_ID = process.env.OWNER_ID
if (!ORG_ID || !OWNER_ID) {
  console.error('set ORG_ID and OWNER_ID')
  process.exit(2)
}
const PASSWORD = 'Zeima-seed-1'
const ID_URL = process.env.ID_URL ?? 'http://localhost:8080'
const PUBLIC_URL = process.env.PUBLIC_URL ?? 'http://localhost:3000'

const config = loadConfig()
if (!config.keycloak.admin) throw new Error('KEYCLOAK_ADMIN_CLIENT_SECRET is not set')
const { client, db } = connect(config.databaseUrl)
const kc = new HttpKeycloakAdmin(config.keycloak.admin)

/** A stable uuid from a name, so reruns hit the same rows. */
const uid = (name: string) => {
  const h = createHash('sha1').update(`zeima-seed/${ORG_ID}/${name}`).digest('hex')
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-5${h.slice(13, 16)}-a${h.slice(17, 20)}-${h.slice(20, 32)}`
}

// ---- people
type Role = 'admin' | 'manager' | 'member'
const PEOPLE: { key: string; first: string; last: string; role: Role; dept: string }[] = [
  { key: 'ada', first: 'Ada', last: 'Lovelace', role: 'admin', dept: 'Engineering' },
  { key: 'max', first: 'Max', last: 'Planck', role: 'manager', dept: 'Engineering' },
  { key: 'grace', first: 'Grace', last: 'Hopper', role: 'member', dept: 'Engineering' },
  { key: 'linus', first: 'Linus', last: 'Pauling', role: 'member', dept: 'Engineering' },
  { key: 'margaret', first: 'Margaret', last: 'Hamilton', role: 'member', dept: 'Operations' },
  { key: 'alan', first: 'Alan', last: 'Turing', role: 'member', dept: 'Operations' },
  { key: 'katherine', first: 'Katherine', last: 'Johnson', role: 'member', dept: 'Sales' },
]

const master = await adminToken(ID_URL, process.env.KEYCLOAK_ADMIN_PASSWORD ?? 'admin')
if (!master) throw new Error('could not sign in to Keycloak as admin')
const kcCall = (method: string, path: string, body?: unknown) =>
  fetch(`${ID_URL}/admin/realms/${config.keycloak.realm}${path}`, {
    method,
    headers: { authorization: `Bearer ${master}`, ...(body === undefined ? {} : { 'content-type': 'application/json' }) },
    body: body === undefined ? undefined : JSON.stringify(body),
  })

const ids = new Map<string, string>()
for (const p of PEOPLE) {
  const username = `seed-${p.key}`
  const res = await kcCall('POST', '/users', {
    username,
    email: `${username}@example.com`,
    firstName: p.first,
    lastName: p.last,
    enabled: true,
    emailVerified: true,
    credentials: [{ type: 'password', value: PASSWORD, temporary: false }],
  })
  if (res.status !== 201 && res.status !== 409) throw new Error(`could not create ${username}: ${res.status} ${await res.text()}`)
  const found = (await (await kcCall('GET', `/users?email=${username}@example.com&exact=true`)).json()) as { id: string }[]
  ids.set(p.key, found[0]!.id)
}

const groupIds = new Map<string, string>()
for (const dept of new Set(PEOPLE.map((p) => p.dept))) groupIds.set(dept, await kc.createGroup(ORG_ID, [dept]))
for (const p of PEOPLE) {
  const id = ids.get(p.key)!
  await kc.addMember(ORG_ID, id).catch((e: { status?: number }) => {
    if (e.status !== 409) throw e
  })
  await kc.setRole(ORG_ID, id, p.role)
  await kc.addToGroup(id, groupIds.get(p.dept)!)
}
console.log('reconciled:', await reconcileOrganization(db, kc, ORG_ID))

// ---- signing in and pushing
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))
/** Retries while the API's rate limit says wait. */
async function patient<T>(fn: () => Promise<T>, ok: (v: T) => boolean): Promise<T> {
  for (let i = 0; ; i++) {
    const v = await fn().catch(() => undefined as T)
    if (v !== undefined && ok(v)) return v
    if (i > 12) throw new Error('gave up waiting for the API')
    await sleep(10_000)
  }
}
interface Discovery {
  issuer?: string
  apiUrl: string
}
/** What `signIn` of the end-to-end helpers gives back. */
interface SignedIn {
  renew(): Promise<void>
  readonly token: string
}
const discovery = await patient(async () => (await json(await fetch(`${PUBLIC_URL}/.well-known/zeima-server`))) as Discovery, (d) => !!d.issuer)
const device = randomUUID()
let counter = 0
const hlc = () => `${String(Date.now()).padStart(15, '0')}-${String(counter++ % 1_000_000).padStart(6, '0')}-${device}`

type Session = SignedIn & { workspaceId: string; key: string }
const sessions = new Map<string, Session>()
for (const p of PEOPLE) {
  const s = await patient(async () => (await signIn(discovery, { email: `seed-${p.key}@example.com`, password: PASSWORD })) as SignedIn, () => true)
  await s.renew() // the token now has the organization and role
  const call = (path: string) => fetch(`${discovery.apiUrl}${path}`, { headers: { authorization: `Bearer ${s.token}` } })
  const list = await patient(async () => json(await call('/v1/workspaces')), (v: unknown) => Array.isArray(v))
  const ws = (list as { id: string; kind: string; orgId?: string }[]).find((w) => w.orgId === ORG_ID)
  if (!ws) throw new Error(`${p.key} cannot see the organization's workspace`)
  sessions.set(p.key, Object.assign(s, { workspaceId: ws.id, key: p.key }))
}

const tally = new Map<string, number>()
async function push(who: string, table: string, rows: { id: string; fields: Record<string, unknown> }[]) {
  const s = sessions.get(who)!
  for (let i = 0; i < rows.length; i += 100) {
    const changes = rows.slice(i, i + 100).map((r) => ({ table, rowId: r.id, op: 'upsert', hlc: hlc(), fields: r.fields }))
    const send = () =>
      fetch(`${discovery.apiUrl}/v1/sync/push`, {
        method: 'POST',
        headers: { authorization: `Bearer ${s.token}`, 'content-type': 'application/json' },
        body: JSON.stringify({ workspaceId: s.workspaceId, deviceId: device, changes }),
      })
    let res = await send()
    // The API limits requests per minute: wait and go on.
    while (res.status === 429) {
      await new Promise((r) => setTimeout(r, 10_000))
      res = await send()
    }
    const body = (await json(res)) as { results?: { status: string; reason?: string }[] }
    if (!body.results) throw new Error(`push of ${table} by ${who}: ${res.status} ${JSON.stringify(body)}`)
    for (const r of body.results) {
      const k = `${table} ${r.status}${r.reason ? ` (${r.reason})` : ''}`
      tally.set(k, (tally.get(k) ?? 0) + 1)
    }
  }
}

// ---- job descriptions the organization defines (by the admin), and who has which
const JOBS: [string, string][] = [
  ['Head of engineering', 'Leads the engineering teams and their roadmap.'],
  ['Engineering manager', 'Looks after a team: planning, people and delivery.'],
  ['Backend developer', 'Builds and runs the services behind the product.'],
  ['Frontend developer', 'Builds the user interface.'],
  ['Operations lead', 'Keeps the company running day to day.'],
  ['Site reliability engineer', 'Keeps systems available and fast.'],
  ['Account executive', 'Looks after customers and sales.'],
]
const HAS: Record<string, string> = {
  ada: 'Head of engineering',
  max: 'Engineering manager',
  grace: 'Backend developer',
  linus: 'Frontend developer',
  margaret: 'Operations lead',
  alan: 'Site reliability engineer',
  katherine: 'Account executive',
}
{
  const api = async (who: string, method: string, path: string, body?: unknown) => {
    const s = sessions.get(who)!
    for (;;) {
      const res = await fetch(`${discovery.apiUrl}/v1/orgs/${ORG_ID}${path}`, {
        method,
        headers: { authorization: `Bearer ${s.token}`, ...(body === undefined ? {} : { 'content-type': 'application/json' }) },
        body: body === undefined ? undefined : JSON.stringify(body),
      })
      if (res.status !== 429) return res
      await sleep(10_000)
    }
  }
  const have = (await (await api('ada', 'GET', '/jobs')).json()) as { id: string; name: string }[]
  for (const [name, description] of JOBS) {
    if (!have.some((j) => j.name === name)) await api('ada', 'POST', '/jobs', { name, description })
  }
  const all = (await (await api('ada', 'GET', '/jobs')).json()) as { id: string; name: string }[]
  for (const p of PEOPLE) {
    const job = all.find((j) => j.name === HAS[p.key])
    const res = await api('ada', 'PUT', `/members/${ids.get(p.key)}/job`, { jobId: job?.id ?? null })
    if (res.status !== 204) console.warn(`job of ${p.key}: ${res.status} (is the API up to date?)`)
  }
}

// ---- the organization's data, by the admin
// The organization may have activities of its own already (names are unique):
// use those, and only add what is missing.
const orgWorkspace = sessions.get('ada')!.workspaceId
const existing = await client<{ row_id: string; name: string }[]>`
  select row_id, fields->'name'->>'v' as name from sync_row
  where workspace_id = ${orgWorkspace} and table_name = 'activity' and deleted is null`
const own = new Map(existing.filter((r) => r.row_id !== uid(`activity/${r.name}`)).map((r) => [r.name, r.row_id]))
const act = (n: string) => own.get(n) ?? uid(`activity/${n}`)
// An earlier run may have added copies of activities that exist: remove them.
const copies = existing.filter((r) => own.has(r.name) && r.row_id === uid(`activity/${r.name}`))
if (copies.length) {
  const res = await fetch(`${discovery.apiUrl}/v1/sync/push`, {
    method: 'POST',
    headers: { authorization: `Bearer ${sessions.get('ada')!.token}`, 'content-type': 'application/json' },
    body: JSON.stringify({ workspaceId: orgWorkspace, deviceId: device, changes: copies.map((c) => ({ table: 'activity', rowId: c.row_id, op: 'delete', hlc: hlc() })) }),
  })
  console.log(`removed ${copies.length} duplicate activities:`, res.status)
}
const proj = (n: string) => uid(`project/${n}`)
const tag = (n: string) => uid(`tag/${n}`)
const cc = (n: string) => uid(`cc/${n}`)

await push('ada', 'activity', [
  ['Development', '#3b82f6'], ['Meeting', '#a855f7'], ['Review', '#f59e0b'], ['Planning', '#14b8a6'],
  ['Support', '#ef4444'], ['Documentation', '#64748b'], ['Sales', '#22c55e'],
].filter(([n]) => !own.has(n!)).map(([n, c]) => ({ id: act(n!), fields: { name: n, color: c, archived: false } })))
await push('ada', 'cost_center', [
  ['100', 'Engineering', true], ['200', 'Operations', true], ['300', 'Sales', true], ['900', 'Closed unit', false],
].map(([code, n, active]) => ({ id: cc(code as string), fields: { code, name: n, active } })))
await push('ada', 'tag', [
  ['urgent', '#ef4444', 'Priority'], ['bug', '#f97316', 'Kind'], ['feature', '#22c55e', 'Kind'], ['customer', '#3b82f6', null], ['onsite', '#a855f7', null],
].map(([n, c, g]) => ({ id: tag(n!), fields: { name: n, color: c, groupName: g } })))
const P = (name: string, alias: string, color: string, client: string | null, budget: number | null, period: string, billable: boolean, status: string, ccKey: string | null, extra: Record<string, unknown> = {}) => ({
  id: proj(name),
  fields: { name, alias, color, client, budgetMinutes: budget, budgetPeriod: period, budgetWarnPct: 80, billableDefault: billable, status, costCenterId: ccKey ? cc(ccKey) : null, visibility: 'org', memberIds: [], ...extra },
})
await push('ada', 'project', [
  P('Atlas', 'ATL', '#3b82f6', 'Acme Corp', 6000, 'monthly', true, 'active', '100', { managerId: ids.get('max') }),
  P('Customer Portal', 'PRT', '#22c55e', 'Globex', 2400, 'weekly', true, 'active', '100'),
  P('Internal Tools', 'TLS', '#f59e0b', null, null, 'total', false, 'active', '200'),
  P('Research', 'RES', '#a855f7', null, 1200, 'total', false, 'paused', null),
  P('Legacy Migration', 'LEG', '#78716c', 'Initech', null, 'total', true, 'archived', null),
  P('Sales Pipeline', 'SAL', '#14b8a6', null, null, 'total', false, 'active', '300', {
    visibility: 'members', memberIds: [ids.get('katherine'), ids.get('max')],
  }),
])

// ---- three weeks of entries per person
const DAY = 86_400_000
const today = new Date()
today.setUTCHours(0, 0, 0, 0)
const rnd = (seed: string) => {
  let h = createHash('sha1').update(seed).digest().readUInt32LE(0)
  return () => {
    h = (Math.imul(h, 1664525) + 1013904223) >>> 0
    return h / 2 ** 32
  }
}
const PLAN: [string, string, number, number, string[], string][] = [
  ['Planning', 'Atlas', 8.5, 9.5, [], 'Plan the day'],
  ['Development', 'Atlas', 9.5, 12.25, ['feature'], 'Implementation'],
  ['Review', 'Customer Portal', 13, 14, ['bug'], 'Code review'],
  ['Development', 'Customer Portal', 14, 16.5, ['feature', 'customer'], 'Portal work'],
  ['Support', 'Internal Tools', 16.5, 17.25, ['urgent', 'bug'], 'Tickets'],
]
const DEPT_PLAN: Record<string, [string, string][]> = {
  Operations: [['Support', 'Internal Tools'], ['Documentation', 'Internal Tools']],
  Sales: [['Sales', 'Sales Pipeline'], ['Meeting', 'Sales Pipeline']],
}
for (const p of PEOPLE) {
  const r = rnd(p.key)
  const rows: { id: string; fields: Record<string, unknown> }[] = []
  for (let back = 21; back >= 0; back--) {
    const day = new Date(today.getTime() - back * DAY)
    if ([0, 6].includes(day.getUTCDay()) || r() < 0.08) continue
    PLAN.forEach(([a, pr, from, to, tags, note], i) => {
      if (r() < 0.15) return
      const alt = DEPT_PLAN[p.dept]
      const [a2, pr2] = alt && i > 0 ? alt[i % alt.length]! : [a, pr]
      const start = day.getTime() + (from + (r() - 0.5) * 0.25) * 3_600_000
      const end = day.getTime() + (to + (r() - 0.5) * 0.25) * 3_600_000
      if (end > Date.now()) return
      rows.push({
        id: uid(`entry/${p.key}/${back}/${i}`),
        fields: {
          userId: ids.get(p.key), activityId: act(a2), projectId: proj(pr2), startAt: Math.round(start), endAt: Math.round(end),
          tz: 'Europe/Berlin', note, billable: ['Atlas', 'Customer Portal', 'Sales Pipeline'].includes(pr2) && i !== 4, source: 'manual',
          locked: false, tagIds: tags.map(tag), costCenterId: null,
        },
      })
    })
  }
  await push(p.key, 'time_entry', rows)
}

// ---- absences: open requests and decided ones
const ymd = (offset: number) => new Date(today.getTime() + offset * DAY).toISOString().slice(0, 10)
await push('ada', 'absence_type', [
  ['vacation', 'Vacation', '#22c55e', true, true], ['sick', 'Sick leave', '#ef4444', true, false], ['training', 'Training', '#3b82f6', true, false],
].map(([key, name, color, reduces, vac]) => ({ id: uid(`absencetype/${key}`), fields: { key, name, color, reducesTarget: reduces, affectsVacation: vac } })))
const vacation = uid('absencetype/vacation')
const sick = uid('absencetype/sick')
if (vacation && sick) {
  const ab = (who: string, n: string, type: string, from: number, to: number, status: string, note: string, half: string | null = null) => ({
    who, id: uid(`absence/${n}`),
    fields: { userId: ids.get(who), typeId: type, startDate: ymd(from), endDate: ymd(to), halfDay: half, status, note, decidedBy: status === 'requested' ? null : ids.get('ada'), decisionNote: '' },
  })
  const list = [
    ab('grace', 'g1', vacation, 7, 11, 'requested', 'Summer break'),
    ab('linus', 'l1', vacation, 14, 18, 'approved', 'Family trip'),
    ab('alan', 'a1', sick, -3, -2, 'approved', 'Flu'),
    ab('margaret', 'm1', vacation, 3, 3, 'requested', 'Appointment', 'pm'),
    ab('katherine', 'k1', vacation, 21, 25, 'rejected', 'Busy season'),
  ]
  // A person only requests; an admin decides.
  for (const a of list) {
    await push(a.who, 'absence', [{ id: a.id, fields: { ...a.fields, status: 'requested', decidedBy: null } }])
    if (a.fields.status !== 'requested') {
      await push('ada', 'absence', [{ id: a.id, fields: { status: a.fields.status, decidedBy: ids.get('ada'), decisionNote: '' } }])
    }
  }
} else console.log('no vacation/sick absence types found; skipped absences')

// ---- rooms for the meetings, by the admin
const room = (n: string) => uid(`room/${n}`)
await push('ada', 'room', [
  ['Blue room', 8],
  ['Green room', 4],
  ['Board room', 14],
].map(([n, seats]) => ({ id: room(n as string), fields: { name: n, capacity: seats } })))

// ---- meetings by Max (manager), inviting the owner and others
const at = (offsetDays: number, h: number, m = 0) => {
  const d = new Date(today.getTime() + offsetDays * DAY)
  return d.getTime() + (h * 60 + m) * 60_000 - 3_600_000 // 1 h: Europe/Berlin (winter) to UTC
}
const monday = -((today.getUTCDay() + 6) % 7)
const everyone = [OWNER_ID, ...PEOPLE.map((p) => ids.get(p.key)!)]
const M = (key: string, title: string, day: number, s: [number, number], e: [number, number], extra: Record<string, unknown>) => ({
  id: uid(`meeting/${key}`),
  fields: {
    userId: ids.get('max'), title, startAt: at(day, s[0], s[1]), endAt: at(day, e[0], e[1]), tz: 'Europe/Berlin', location: null, onlineUrl: null,
    roomId: null, projectId: null, activityId: act('Meeting'), tagIds: [], billable: false, costCenterId: null, note: '', rrule: null, attendeeIds: everyone, cancelled: false, ...extra,
  },
})
const meetings = [
  M('standup', 'Daily standup', monday, [9, 30], [9, 45], { rrule: 'FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR', onlineUrl: 'https://meet.example.com/standup', projectId: proj('Atlas') }),
  M('planning', 'Sprint planning', monday, [10, 0], [11, 30], { rrule: 'FREQ=WEEKLY;BYDAY=MO', roomId: room('Blue room'), location: 'Blue room', projectId: proj('Atlas'), activityId: act('Planning'), tagIds: [tag('feature')], costCenterId: cc('100') }),
  M('design', 'Design sync', monday + 1, [15, 0], [16, 0], { rrule: 'FREQ=WEEKLY;BYDAY=TU,TH', roomId: room('Green room'), location: 'Green room', onlineUrl: 'https://meet.example.com/design', projectId: proj('Customer Portal'), billable: true, tagIds: [tag('customer')] }),
  M('retro', 'Retrospective', monday + 4, [14, 0], [15, 0], { rrule: 'FREQ=WEEKLY;INTERVAL=2;BYDAY=FR', roomId: room('Board room'), location: 'Board room' }),
  M('demo', 'Customer demo', 1, [11, 0], [12, 0], { location: 'Globex HQ', projectId: proj('Customer Portal'), billable: true, tagIds: [tag('customer'), tag('onsite')], costCenterId: cc('100'), attendeeIds: [OWNER_ID, ids.get('ada'), ids.get('grace')] }),
  M('qreview', 'Quarterly review', 3, [13, 0], [15, 0], { roomId: room('Board room'), location: 'Board room', onlineUrl: 'https://meet.example.com/qr', activityId: act('Review'), tagIds: [tag('urgent')], costCenterId: cc('200') }),
  M('cancelled', 'Workshop (cancelled)', 2, [10, 0], [12, 0], { cancelled: true, roomId: room('Green room'), location: 'Green room' }),
]
await push('max', 'meeting', meetings)
const answers: [string, string, string][] = [
  ['ada', 'standup', 'accepted'], ['grace', 'standup', 'accepted'], ['linus', 'standup', 'tentative'], ['alan', 'standup', 'declined'],
  ['ada', 'planning', 'accepted'], ['grace', 'planning', 'accepted'], ['margaret', 'design', 'tentative'], ['grace', 'demo', 'accepted'], ['ada', 'demo', 'declined'],
]
for (const [who, m, status] of answers) {
  await push(who, 'meeting_response', [{ id: uid(`answer/${who}/${m}`), fields: { userId: ids.get(who), meetingId: uid(`meeting/${m}`), status } }])
}

console.log([...tally].map(([k, n]) => `${n} × ${k}`).join('\n'))
console.log(`\nDemo people sign in with seed-<name>@example.com (${PEOPLE.map((p) => p.key).join(', ')}) and the password ${PASSWORD}`)
await client.end()
