// End-to-end check of the organization features against a running API and a
// real Keycloak: two people, an organization, an invitation, roles, the
// shared workspace and its rules, and a timesheet that gets approved.
//
//   PUBLIC_URL=http://localhost:3000 ID_URL=http://localhost:8080 KEYCLOAK_ADMIN_PASSWORD=admin \
//   node deploy/e2e/org-smoke.mjs
//
// Add E2E_SSO=1 to also check single sign-on; the API then needs SSO_ALLOW_PRIVATE_URLS=true and SSO_VERIFY_DOMAINS=false.
//
// The API needs KEYCLOAK_ADMIN_CLIENT_SECRET (see deploy/compose.dev.yml) and
// the realm from deploy/keycloak applied. Needs Node 24. Exit code 1 on the
// first failed step.

import { randomUUID } from 'node:crypto'
import { adminToken, createUser, json, reporter, signIn } from './lib.mjs'

const { PUBLIC_URL, ID_URL, KEYCLOAK_ADMIN_PASSWORD } = process.env
// Keycloak's administration console is not published on the public sign-in host: give its address when it is only
// reachable inside the network.
const ADMIN_URL = process.env.KEYCLOAK_ADMIN_URL ?? ID_URL
if (!PUBLIC_URL || !ID_URL || !KEYCLOAK_ADMIN_PASSWORD) {
  console.error('set PUBLIC_URL, ID_URL and KEYCLOAK_ADMIN_PASSWORD')
  process.exit(2)
}
const { must, fail } = reporter()

const discovery = await json(await fetch(`${PUBLIC_URL}/.well-known/zeima-server`))
const admin = await adminToken(ADMIN_URL, KEYCLOAK_ADMIN_PASSWORD)
must(admin, 'admin signed in to Keycloak')

const ownerUser = await createUser(ADMIN_URL, admin, 'owner')
const memberUser = await createUser(ADMIN_URL, admin, 'member')
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

// ---- single sign-on (needs an API that accepts a provider on this machine: SSO_ALLOW_PRIVATE_URLS=true)
// Keycloak's own master realm stands in for the organization's identity provider.
if (process.env.E2E_SSO) {
  const domain = `e2e-${randomUUID().slice(0, 8)}.example`
  const before = await expect(await call(owner, 'GET', `/v1/orgs/${org.id}/sso`), 200, 'the owner reads the single sign-on before setting it up')
  must(before.configured === false && before.redirectUri.includes(`/broker/sso-${org.id}/endpoint`), 'it names the address to point the provider at', before)
  await expect(await call(member, 'GET', `/v1/orgs/${org.id}/sso`), 403, 'a member may not read it')
  const set = await expect(
    await call(owner, 'PUT', `/v1/orgs/${org.id}/sso`, { protocol: 'oidc', name: 'E2E sign-in', sourceUrl: `${ID_URL}/realms/master`, clientId: 'e2e-client', clientSecret: 'e2e-secret', domains: [domain] }),
    200,
    'the owner set up an OIDC provider',
  )
  must(set.configured && set.domains[0] === domain && !JSON.stringify(set).includes('e2e-secret'), 'it is configured and the secret is not given back', set)
  // Keycloak: the provider exists, is linked to the organization and is hidden from the sign-in page.
  const kcIdp = await json(await fetch(`${ADMIN_URL}/admin/realms/zeima/identity-provider/instances/sso-${org.id}`, { headers: { authorization: `Bearer ${admin}` } }))
  must(kcIdp.hideOnLogin === true && kcIdp.config.authorizationUrl?.includes('/realms/master/'), 'Keycloak has the provider, read from its metadata and hidden from the sign-in page', kcIdp)
  const kcOrg = await json(await fetch(`${ADMIN_URL}/admin/realms/zeima/organizations/${org.id}`, { headers: { authorization: `Bearer ${admin}` } }))
  must(kcOrg.domains?.some((d) => d.name === domain && d.identityProviderAlias === `sso-${org.id}` && d.autoRedirect), 'the organization\'s domain is tied to the provider', kcOrg)
  // A person with an address in the domain is sent to the provider when they sign in.
  const redirected = await (async () => {
    const oidc = await json(await fetch(`${discovery.issuer}/.well-known/openid-configuration`))
    const authUrl = new URL(oidc.authorization_endpoint)
    for (const [k, v] of Object.entries({ response_type: 'code', client_id: discovery.clientId, redirect_uri: 'http://127.0.0.1:54321', scope: 'openid', state: 'x', code_challenge: 'x'.repeat(43), code_challenge_method: 'S256' })) authUrl.searchParams.set(k, v)
    const jar = new Map()
    const remember = (res) => res.headers.getSetCookie().forEach((c) => jar.set(c.split('=')[0], c.split(';')[0]))
    const page = await fetch(authUrl, { redirect: 'manual' })
    remember(page)
    const action = (await page.text()).match(/<form[^>]*id="kc-form-login"[^>]*>/)?.[0]?.match(/\saction="([^"]+)"/)?.[1]?.replaceAll('&amp;', '&')
    if (!action) return 'no login form'
    let res = await fetch(action, { method: 'POST', redirect: 'manual', headers: { cookie: [...jar.values()].join('; '), 'content-type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ username: `someone@${domain}` }) })
    for (let hop = 0; hop < 5; hop++) {
      remember(res)
      const next = res.headers.get('location')
      if (!next) return `stopped at ${res.status}`
      if (next.includes('/realms/master/protocol/openid-connect/auth')) return 'ok'
      res = await fetch(new URL(next, ID_URL), { redirect: 'manual', headers: { cookie: [...jar.values()].join('; ') } })
    }
    return 'too many redirects'
  })()
  must(redirected === 'ok', 'signing in with an address in the domain leads to the identity provider', redirected)
  await expect(await call(owner, 'DELETE', `/v1/orgs/${org.id}/sso`), 204, 'the owner removed the single sign-on')
  const gone = await fetch(`${ADMIN_URL}/admin/realms/zeima/identity-provider/instances/sso-${org.id}`, { headers: { authorization: `Bearer ${admin}` } })
  must(gone.status === 404, 'Keycloak no longer has the provider', gone.status)
}

// ---- leaving
await expect(await call(owner, 'DELETE', `/v1/orgs/${org.id}/members/${member.sub}`), 204, 'the owner removed the member')
const audit = await json(await call(owner, 'GET', `/v1/orgs/${org.id}/audit`))
must(['org.create', 'member.invite', 'member.join', 'member.remove'].every((a) => audit.some((e) => e.action === a)), 'the audit log has the steps', audit.map((e) => e.action))
await expect(await call(owner, 'DELETE', `/v1/orgs/${org.id}`), 204, 'the owner deleted the organization')
console.log('all steps passed')
