// End-to-end check of a running Zeima stack (deploy/docker-compose.yml):
// creates a user in Keycloak, signs in the way the desktop app does
// (Authorization Code + PKCE with a loopback redirect), then uses the API:
// discovery, /me, a workspace, a push and a pull.
//
//   PUBLIC_URL=https://localhost:8443 ID_URL=https://id.localhost:8443 \
//   KEYCLOAK_ADMIN_PASSWORD=... NODE_TLS_REJECT_UNAUTHORIZED=0 node deploy/e2e/smoke.mjs
//
// Needs Node 24. Exit code 1 on the first failed step.

import { createHash, randomBytes, randomUUID } from 'node:crypto'

const { PUBLIC_URL, ID_URL, KEYCLOAK_ADMIN_PASSWORD } = process.env
if (!PUBLIC_URL || !ID_URL || !KEYCLOAK_ADMIN_PASSWORD) {
  console.error('set PUBLIC_URL, ID_URL and KEYCLOAK_ADMIN_PASSWORD')
  process.exit(2)
}

let step = 0
const ok = (message) => console.log(`ok ${++step} - ${message}`)
const fail = (message) => {
  console.error(`not ok ${++step} - ${message}`)
  process.exit(1)
}
const must = (condition, message) => (condition ? ok(message) : fail(message))

const json = async (res) => {
  const text = await res.text()
  try {
    return JSON.parse(text)
  } catch {
    return text
  }
}

// ---- 1. a user, created through the admin API
const adminToken = (
  await json(
    await fetch(`${ID_URL}/realms/master/protocol/openid-connect/token`, {
      method: 'POST',
      body: new URLSearchParams({ client_id: 'admin-cli', grant_type: 'password', username: 'admin', password: KEYCLOAK_ADMIN_PASSWORD }),
    }),
  )
).access_token
must(adminToken, 'admin signed in to Keycloak')

const username = `e2e-${randomBytes(4).toString('hex')}`
const password = randomBytes(12).toString('base64url')
const created = await fetch(`${ID_URL}/admin/realms/zeima/users`, {
  method: 'POST',
  headers: { authorization: `Bearer ${adminToken}`, 'content-type': 'application/json' },
  body: JSON.stringify({
    username,
    email: `${username}@example.com`,
    firstName: 'E2E',
    lastName: 'User',
    enabled: true,
    emailVerified: true,
    credentials: [{ type: 'password', value: password, temporary: false }],
  }),
})
must(created.status === 201, `created user ${username}`)

// ---- 2. discovery, as the app does it
const discovery = await json(await fetch(`${PUBLIC_URL}/.well-known/zeima-server`))
must(discovery.clientId === 'zeima-desktop' && discovery.issuer.startsWith(ID_URL), 'discovery names the issuer and client')
const oidc = await json(await fetch(`${discovery.issuer}/.well-known/openid-configuration`))
must(oidc.authorization_endpoint && oidc.token_endpoint, 'the issuer publishes its endpoints')

// ---- 3. sign in: Authorization Code + PKCE, loopback redirect
const verifier = randomBytes(48).toString('base64url')
const challenge = createHash('sha256').update(verifier).digest('base64url')
const redirect = 'http://127.0.0.1:54321'
const state = randomBytes(8).toString('hex')
const authUrl = new URL(oidc.authorization_endpoint)
for (const [k, v] of Object.entries({
  response_type: 'code', client_id: discovery.clientId, redirect_uri: redirect, state, scope: 'openid profile email offline_access',
  code_challenge: challenge, code_challenge_method: 'S256',
})) authUrl.searchParams.set(k, v)

const jar = new Map()
const remember = (res) => {
  for (const c of res.headers.getSetCookie()) {
    const [pair] = c.split(';')
    jar.set(pair.slice(0, pair.indexOf('=')), pair)
  }
}
const cookies = () => [...jar.values()].join('; ')

const page = await fetch(authUrl, { redirect: 'manual' })
remember(page)
const html = await page.text()
const form = html.match(/<form[^>]*id="kc-form-login"[^>]*>/)?.[0] ?? ''
const action = form.match(/\saction="([^"]+)"/)?.[1]?.replaceAll('&amp;', '&')
must(action, 'the login page has a form')
const login = await fetch(action, {
  method: 'POST',
  redirect: 'manual',
  headers: { cookie: cookies(), 'content-type': 'application/x-www-form-urlencoded' },
  // The realm uses the email address as the user name.
  body: new URLSearchParams({ username: `${username}@example.com`, password }),
})
const location = login.headers.get('location') ?? ''
const loginBody = location ? '' : await login.text()
// Keycloak shows why a sign-in failed in an alert on the form.
const reason = loginBody.match(/kc-feedback-text[^>]*>([^<]+)</)?.[1]?.trim() ?? loginBody.match(/<title>([^<]*)/)?.[1]
must(location.startsWith(redirect), location ? 'signing in redirects to the loopback address' : `signing in redirects to the loopback address (got ${login.status}: ${reason})`)
const back = new URL(location)
must(back.searchParams.get('state') === state, 'the state comes back')
const code = back.searchParams.get('code')
must(code, 'the redirect carries a code')

const tokens = await json(
  await fetch(oidc.token_endpoint, {
    method: 'POST',
    body: new URLSearchParams({ grant_type: 'authorization_code', client_id: discovery.clientId, code, redirect_uri: redirect, code_verifier: verifier }),
  }),
)
must(tokens.access_token && tokens.refresh_token, 'the code buys an access and a refresh token')

// ---- 4. the API with that token
const api = (path, init = {}) =>
  fetch(`${discovery.apiUrl}${path}`, { ...init, headers: { authorization: `Bearer ${tokens.access_token}`, 'content-type': 'application/json', ...init.headers } })

const me = await json(await api('/v1/me'))
must(me.email === `${username}@example.com`, '/v1/me knows who signed in')

const entitlement = await json(await api('/v1/entitlement'))
must(typeof entitlement.plan === 'string', `/v1/entitlement answers (plan ${entitlement.plan})`)

const workspaceId = randomUUID()
const ws = await api('/v1/workspaces', { method: 'POST', body: JSON.stringify({ id: workspaceId, name: 'Personal' }) })
must(ws.status === 200, 'created the personal workspace')

const rowId = randomUUID()
const hlc = `${String(Date.now()).padStart(15, '0')}-000000-${randomUUID()}`
const push = await api('/v1/sync/push', {
  method: 'POST',
  body: JSON.stringify({ workspaceId, deviceId: randomUUID(), changes: [{ table: 'project', rowId, op: 'upsert', hlc, fields: { name: 'E2E project' } }] }),
})
const pushed = await json(push)
must(push.status === 200 && pushed.results?.[0]?.status === 'applied', 'pushed a change')

const pulled = await json(await api('/v1/sync/pull?cursor=0'))
must(pulled.rows?.some((r) => r.rowId === rowId && r.fields.name.v === 'E2E project'), 'pulled it back')

console.log(`\n${step} checks passed`)
