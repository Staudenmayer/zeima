// Shared steps of the end-to-end checks: users in Keycloak and signing in the
// way the desktop app does (Authorization Code + PKCE, loopback redirect).

import { createHash, randomBytes } from 'node:crypto'

export const json = async (res) => {
  const text = await res.text()
  try {
    return JSON.parse(text)
  } catch {
    return text
  }
}

export function reporter() {
  let step = 0
  const ok = (message) => console.log(`ok ${++step} - ${message}`)
  const fail = (message, detail) => {
    console.error(`not ok ${++step} - ${message}`)
    if (detail !== undefined) console.error(typeof detail === 'string' ? detail : JSON.stringify(detail, null, 2))
    process.exit(1)
  }
  const must = (condition, message, detail) => (condition ? ok(message) : fail(message, detail))
  return { ok, fail, must }
}

export async function adminToken(idUrl, password) {
  const res = await json(
    await fetch(`${idUrl}/realms/master/protocol/openid-connect/token`, {
      method: 'POST',
      body: new URLSearchParams({ client_id: 'admin-cli', grant_type: 'password', username: 'admin', password }),
    }),
  )
  return res.access_token
}

/** A user with a verified email address; returns what is needed to sign in. */
export async function createUser(idUrl, token, label) {
  const username = `e2e-${label}-${randomBytes(3).toString('hex')}`
  const password = randomBytes(12).toString('base64url')
  const res = await fetch(`${idUrl}/admin/realms/zeima/users`, {
    method: 'POST',
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: JSON.stringify({
      username,
      email: `${username}@example.com`,
      firstName: label,
      lastName: 'E2E',
      enabled: true,
      emailVerified: true,
      credentials: [{ type: 'password', value: password, temporary: false }],
    }),
  })
  if (res.status !== 201) throw new Error(`could not create ${username}: ${res.status} ${await res.text()}`)
  return { username, email: `${username}@example.com`, password }
}

/** Signs in with the realm's desktop client; returns the tokens and a way to refresh them. */
export async function signIn(discovery, user) {
  const oidc = await json(await fetch(`${discovery.issuer}/.well-known/openid-configuration`))
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
  const page = await fetch(authUrl, { redirect: 'manual' })
  remember(page)
  const html = await page.text()
  const formAction = (page) => page.match(/<form[^>]*id="kc-form-login"[^>]*>/)?.[0]?.match(/\saction="([^"]+)"/)?.[1]?.replaceAll('&amp;', '&')
  const post = async (action, fields) => {
    const res = await fetch(action, {
      method: 'POST',
      redirect: 'manual',
      headers: { cookie: [...jar.values()].join('; '), 'content-type': 'application/x-www-form-urlencoded' },
      body: new URLSearchParams(fields),
    })
    remember(res)
    return res
  }
  let action = formAction(html)
  if (!action) throw new Error('the login page has no form')
  // Once an organization exists, Keycloak asks for the address first and the password on a second page.
  let login
  if (!/name="password"/.test(html)) {
    const first = await post(action, { username: user.email })
    const second = await first.text()
    action = formAction(second)
    if (!action) throw new Error('there is no password page after the address')
    login = await post(action, { password: user.password })
  } else {
    login = await post(action, { username: user.email, password: user.password })
  }
  const location = login.headers.get('location') ?? ''
  if (!location.startsWith(redirect)) {
    // Keycloak shows why a sign-in failed in an alert on the form.
    const body = location ? '' : await login.text()
    const reason = body.match(/kc-feedback-text[^>]*>([^<]+)</)?.[1]?.trim() ?? body.match(/<title>([^<]*)/)?.[1]
    throw new Error(`sign-in did not redirect to the loopback address (${login.status}: ${reason ?? location})`)
  }
  const code = new URL(location).searchParams.get('code')
  const tokens = await json(
    await fetch(oidc.token_endpoint, {
      method: 'POST',
      body: new URLSearchParams({ grant_type: 'authorization_code', client_id: discovery.clientId, code, redirect_uri: redirect, code_verifier: verifier }),
    }),
  )
  if (!tokens.access_token) throw new Error(`no token: ${JSON.stringify(tokens)}`)
  let refresh = tokens.refresh_token
  let access = tokens.access_token
  return {
    /** A fresh access token, so changes to groups show up in it. */
    async renew() {
      const next = await json(
        await fetch(oidc.token_endpoint, {
          method: 'POST',
          body: new URLSearchParams({ grant_type: 'refresh_token', client_id: discovery.clientId, refresh_token: refresh }),
        }),
      )
      if (!next.access_token) throw new Error(`could not renew: ${JSON.stringify(next)}`)
      access = next.access_token
      refresh = next.refresh_token ?? refresh
    },
    get token() {
      return access
    },
    get sub() {
      return JSON.parse(Buffer.from(access.split('.')[1], 'base64url').toString()).sub
    },
    get claims() {
      return JSON.parse(Buffer.from(access.split('.')[1], 'base64url').toString())
    },
  }
}
