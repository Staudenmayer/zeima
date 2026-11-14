// End-to-end check of the organization features against a running API and a
// real Keycloak: two people, an organization, an invitation, roles, the
// shared workspace and its rules, and a timesheet that gets approved.
//
//   PUBLIC_URL=http://localhost:3000 ID_URL=http://localhost:8080 KEYCLOAK_ADMIN_PASSWORD=admin \
//   node deploy/e2e/org-smoke.mjs
//
// The API needs KEYCLOAK_ADMIN_CLIENT_SECRET (see deploy/compose.dev.yml) and
// the realm from deploy/keycloak applied. Needs Node 24. Exit code 1 on the
// first failed step.

import { randomUUID } from 'node:crypto'
import { adminToken, createUser, json, reporter, signIn } from './lib.mjs'

const { PUBLIC_URL, ID_URL, KEYCLOAK_ADMIN_PASSWORD } = process.env
if (!PUBLIC_URL || !ID_URL || !KEYCLOAK_ADMIN_PASSWORD) {
  console.error('set PUBLIC_URL, ID_URL and KEYCLOAK_ADMIN_PASSWORD')
  process.exit(2)
}
const { must, fail } = reporter()

const discovery = await json(await fetch(`${PUBLIC_URL}/.well-known/zeima-server`))
const admin = await adminToken(ID_URL, KEYCLOAK_ADMIN_PASSWORD)
must(admin, 'admin signed in to Keycloak')

const ownerUser = await createUser(ID_URL, admin, 'owner')
const memberUser = await createUser(ID_URL, admin, 'member')
const owner = await signIn(discovery, ownerUser)
const member = await signIn(discovery, memberUser)
must(owner.sub !== member.sub, 'two people signed in')

const call = (who, method, path, body) =>
  fetch(`${discovery.apiUrl}${path}`, {
    method,
    headers: { authorization: `Bearer ${who.token}`, ...(body === undefined ? {} : { 'content-type': 'application/json' }) },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
const expect = async (res, status, message) => {
  const body = await json(res)
  must(res.status === status, message, { status: res.status, body })
  return body
}

// ---- the organization
const org = await expect(await call(owner, 'POST', '/v1/orgs', { name: `E2E Corp ${randomUUID().slice(0, 6)}` }), 201, 'the owner created an organization')
await owner.renew()
const meOwner = await json(await call(owner, 'GET', '/v1/me'))
must(meOwner.orgs.some((o) => o.id === org.id && o.role === 'owner'), 'a fresh token shows the organization with the owner role', meOwner)

// ---- a department and an invitation
const dev = await expect(await call(owner, 'POST', `/v1/orgs/${org.id}/groups`, { name: 'dev' }), 201, 'created the department dev')
await expect(await call(owner, 'POST', `/v1/orgs/${org.id}/invitations`, { email: memberUser.email, role: 'member' }), 201, 'invited the second person')

// The invitation takes effect when they sign in with the verified address.
await member.renew()
const meMember = await json(await call(member, 'GET', '/v1/me'))
await member.renew()
const meMember2 = await json(await call(member, 'GET', '/v1/me'))
must(meMember2.orgs.some((o) => o.id === org.id && o.role === 'member'), 'the invited person has the member role after signing in', { before: meMember, after: meMember2 })

const members = await json(await call(owner, 'GET', `/v1/orgs/${org.id}/members`))
must(members.length === 2, 'the owner sees both members', members)
await expect(await call(owner, 'PUT', `/v1/orgs/${org.id}/groups/${dev.id}/members/${member.sub}`), 204, 'put the member into dev')
await member.renew()
must(member.claims.groups?.some((g) => g === `/orgs/${org.id}/groups/dev`), 'the department shows in the member\'s token', member.claims.groups)

// ---- the shared workspace and its rules
const workspaces = await json(await call(member, 'GET', '/v1/workspaces'))
const shared = workspaces.find((w) => w.kind === 'org')
must(shared?.orgId === org.id, 'the member sees the organization workspace', workspaces)
const device = randomUUID()
const hlc = (n) => `${String(Date.now() + n).padStart(15, '0')}-000000-${device}`
const push = async (who, changes) => (await json(await call(who, 'POST', '/v1/sync/push', { workspaceId: shared.id, deviceId: device, changes }))).results
const upsert = (table, rowId, n, fields) => ({ table, rowId, op: 'upsert', hlc: hlc(n), fields })

const project = randomUUID()
must((await push(member, [upsert('project', project, 1, { name: 'Nope' })]))[0].status === 'rejected', 'a member cannot create a project')
must((await push(owner, [upsert('project', project, 2, { name: 'Website' })]))[0].status === 'applied', 'the owner can')
const entry = randomUUID()
const start = Date.now() - 7200_000
must(
  (await push(member, [upsert('time_entry', entry, 3, { userId: member.sub, startAt: start, endAt: start + 3600_000, projectId: project, note: 'work', tagIds: [], locked: false })]))[0].status === 'applied',
  'the member tracked time',
)
const pulled = async (who) => (await json(await call(who, 'GET', '/v1/sync/pull?cursor=0'))).rows.filter((r) => r.workspaceId === shared.id)
must((await pulled(member)).some((r) => r.rowId === entry), 'the member reads their entry')
must((await pulled(owner)).some((r) => r.rowId === entry), 'the owner reads it too')

// ---- a timesheet, approved by the owner, locks the entry
const day = new Date(start).toISOString().slice(0, 10)
const sub = randomUUID()
const submission = { userId: member.sub, periodStart: day, periodEnd: day, tz: 'UTC', status: 'submitted', note: '', submittedAt: Date.now(), decidedBy: null, decidedAt: null, decisionNote: '' }
must((await push(member, [upsert('timesheet_submission', sub, 4, submission)]))[0].status === 'applied', 'the member submitted a timesheet')
must((await push(member, [upsert('timesheet_submission', sub, 5, { status: 'approved', decidedBy: member.sub })]))[0].status === 'rejected', 'they cannot approve their own')
must((await push(owner, [upsert('timesheet_submission', sub, 6, { status: 'approved', decidedBy: owner.sub })]))[0].status === 'applied', 'the owner approved it')
const locked = (await pulled(member)).find((r) => r.rowId === entry)?.fields.locked?.v
must(locked === true, 'the approved period locked the entry', locked)
must((await push(member, [upsert('time_entry', entry, 7, { note: 'late edit' })]))[0].reason === 'locked', 'the member can no longer edit it')

// ---- leaving
await expect(await call(owner, 'DELETE', `/v1/orgs/${org.id}/members/${member.sub}`), 204, 'the owner removed the member')
const audit = await json(await call(owner, 'GET', `/v1/orgs/${org.id}/audit`))
must(['org.create', 'member.invite', 'member.join', 'member.remove'].every((a) => audit.some((e) => e.action === a)), 'the audit log has the steps', audit.map((e) => e.action))
await expect(await call(owner, 'DELETE', `/v1/orgs/${org.id}`), 204, 'the owner deleted the organization')
console.log('all steps passed')
