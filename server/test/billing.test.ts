import { createHmac } from 'node:crypto'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { eq } from 'drizzle-orm'
import { buildApp } from '../src/app.ts'
import { schema } from '../src/db/index.ts'
import { ConfigError, loadConfig } from '../src/config.ts'
import { writeSubscription } from '../src/modules/billing/routes.ts'
import { checkoutToken, checkoutUrl, parseEvent, signatureValid, uuidFor } from '../src/modules/billing/lemonsqueezy.ts'
import { keys, syncMirror, token } from './helpers.ts'
import { createTestDatabase, reachable } from './pg.ts'

const PLANS = [
  { key: 'pro_monthly', plan: 'pro', variantId: '111', checkoutId: '11111111-1111-4111-8111-111111111111' },
  { key: 'team_yearly', plan: 'team', variantId: '222', checkoutId: '22222222-2222-4222-8222-222222222222' },
]
const SECRET = 'whsec-test-secret'
const ADA = '2c8f1f5e-0000-4000-8000-000000000001'
const ORG = '00000000-0000-4000-8000-000000000500'
const DAY = 86_400_000
const now = Date.parse('2026-11-13T12:00:00Z')
const iso = (ms: number) => new Date(ms).toISOString()

function event(name: string, attributes: Record<string, unknown> = {}, custom: Record<string, unknown> = { user_id: ADA, token: checkoutToken(SECRET, ADA, null) }, id = '9001') {
  return {
    meta: { event_name: name, custom_data: custom },
    data: {
      id,
      type: 'subscriptions',
      attributes: {
        variant_id: 111,
        status: 'active',
        user_email: 'ada@example.com',
        customer_id: 77,
        renews_at: iso(now + 30 * DAY),
        ends_at: null,
        updated_at: iso(now),
        urls: { customer_portal: 'https://store.example/portal/abc' },
        ...attributes,
      },
    },
  }
}

describe('Lemon Squeezy events', () => {
  it('verifies the signature over the exact bytes', () => {
    const body = Buffer.from('{"a":1}')
    const good = createHmac('sha256', SECRET).update(body).digest('hex')
    expect(signatureValid(body, good, SECRET)).toBe(true)
    expect(signatureValid(Buffer.from('{"a":2}'), good, SECRET)).toBe(false)
    expect(signatureValid(body, good, 'other-secret')).toBe(false)
    expect(signatureValid(body, undefined, SECRET)).toBe(false)
    expect(signatureValid(body, 'zz', SECRET)).toBe(false)
  })

  it('maps an active subscription to its plan, paid until renewal plus a few days', () => {
    const r = parseEvent(event('subscription_created'), PLANS, now, SECRET)
    expect(r.ok && r.update).toMatchObject({ userId: ADA, orgId: null, plan: 'pro', validUntil: now + 33 * DAY })
    expect(r.ok && r.update.billing.portalUrl).toBe('https://store.example/portal/abc')
  })

  it('keeps a cancelled subscription until its period ends', () => {
    const r = parseEvent(event('subscription_cancelled', { status: 'cancelled', ends_at: iso(now + 10 * DAY) }), PLANS, now, SECRET)
    expect(r.ok && r.update.validUntil).toBe(now + 10 * DAY)
  })

  it('ends an expired, paused or unpaid subscription at once', () => {
    for (const [name, status] of [['subscription_expired', 'expired'], ['subscription_updated', 'paused'], ['subscription_updated', 'unpaid']] as const) {
      const r = parseEvent(event(name, { status, ends_at: null }), PLANS, now, SECRET)
      expect(r.ok && r.update.validUntil).toBeLessThanOrEqual(now)
    }
  })

  it('assigns organization purchases to the organization', () => {
    const r = parseEvent(event('subscription_created', { variant_id: 222 }, { user_id: ADA, org_id: ORG, token: checkoutToken(SECRET, ADA, ORG) }), PLANS, now, SECRET)
    expect(r.ok && r.update).toMatchObject({ userId: null, orgId: ORG, plan: 'team' })
  })

  it('refuses what it cannot place', () => {
    expect(parseEvent(event('order_created'), PLANS, now, SECRET)).toMatchObject({ ok: false })
    expect(parseEvent(event('subscription_created', { variant_id: 999 }), PLANS, now, SECRET)).toMatchObject({ ok: false, reason: 'unknown variant 999' })
    expect(parseEvent(event('subscription_created', {}, {}), PLANS, now, SECRET)).toMatchObject({ ok: false })
    expect(parseEvent(null, PLANS, now, SECRET)).toMatchObject({ ok: false })
  })

  it('gives a subscription one stable id', () => {
    expect(uuidFor('9001')).toBe(uuidFor('9001'))
    expect(uuidFor('9001')).not.toBe(uuidFor('9002'))
    expect(uuidFor('9001')).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-5[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
  })

  it('refuses a purchase whose metadata the buyer edited', () => {
    const signed = { user_id: ADA, org_id: ORG, token: checkoutToken(SECRET, ADA, ORG) }
    expect(parseEvent(event('subscription_created', {}, signed), PLANS, now, SECRET)).toMatchObject({ ok: true })
    // Another organization's id in the link the buyer opened.
    const other = '00000000-0000-4000-8000-000000000999'
    expect(parseEvent(event('subscription_created', {}, { ...signed, org_id: other }), PLANS, now, SECRET)).toMatchObject({ ok: false, reason: expect.stringContaining('not signed') })
    // Another user's id.
    expect(parseEvent(event('subscription_created', {}, { ...signed, user_id: '2c8f1f5e-0000-4000-8000-000000000002' }), PLANS, now, SECRET)).toMatchObject({ ok: false })
    // The organization dropped, or no token, or a token from another server.
    expect(parseEvent(event('subscription_created', {}, { user_id: ADA, token: signed.token }), PLANS, now, SECRET)).toMatchObject({ ok: false })
    expect(parseEvent(event('subscription_created', {}, { user_id: ADA, org_id: ORG }), PLANS, now, SECRET)).toMatchObject({ ok: false })
    expect(parseEvent(event('subscription_created', {}, { ...signed, token: checkoutToken('another-secret', ADA, ORG) }), PLANS, now, SECRET)).toMatchObject({ ok: false })
    expect(parseEvent(event('subscription_created', {}, { org_id: ORG, token: signed.token }), PLANS, now, SECRET)).toMatchObject({ ok: false })
  })

  it('puts the buyer into the checkout link', () => {
    const url = new URL(checkoutUrl('https://store.example', PLANS[0]!, { userId: ADA, email: 'ada@example.com' }, SECRET))
    expect(url.pathname).toBe(`/checkout/buy/${PLANS[0]!.checkoutId}`)
    expect(url.searchParams.get('checkout[custom][user_id]')).toBe(ADA)
    expect(url.searchParams.get('checkout[email]')).toBe('ada@example.com')
  })
})

describe('BILLING_PLANS', () => {
  it('is checked at start-up', () => {
    expect(loadConfig({}).billing.plans).toEqual([])
    expect(loadConfig({ BILLING_PLANS: JSON.stringify(PLANS) }).billing.plans).toHaveLength(2)
    expect(() => loadConfig({ BILLING_PLANS: 'nope' })).toThrow(ConfigError)
    expect(() => loadConfig({ BILLING_PLANS: '[{"key":"x"}]' })).toThrow(/BILLING_PLANS\[0\]/)
  })
})

describe.skipIf(!(await reachable()))('billing routes', () => {
  let db: Awaited<ReturnType<typeof createTestDatabase>>
  beforeAll(async () => (db = await createTestDatabase()))
  afterAll(async () => db?.drop())

  const cloud = (extra: Record<string, string> = {}) =>
    buildApp(
      loadConfig({
        NODE_ENV: 'test',
        LOG_LEVEL: 'silent',
        METRICS_ENABLED: 'false',
        DEPLOYMENT_MODE: 'cloud',
        BILLING_WEBHOOK_SECRET: SECRET,
        BILLING_STORE_URL: 'https://store.example',
        BILLING_PLANS: JSON.stringify(PLANS),
        ...extra,
      }),
      { logger: false, keys, readiness: {}, db: db.db },
    )
  const webhook = (app: Awaited<ReturnType<typeof cloud>>, body: unknown, secret = SECRET) => {
    const raw = JSON.stringify(body)
    return app.inject({
      method: 'POST',
      url: '/v1/billing/webhook',
      headers: { 'content-type': 'application/json', 'x-signature': createHmac('sha256', secret).update(raw).digest('hex') },
      payload: raw,
    })
  }
  const authed = async (sub = ADA, groups: string[] = []) => {
    await syncMirror(db.db, sub, groups)
    return { authorization: `Bearer ${await token({ sub, groups })}` }
  }

  it('attaches a new subscription only for the organization\'s owner, and shows the portal link to owners only', async () => {
    const BOB = '2c8f1f5e-0000-4000-8000-000000000002'
    await db.db.insert(schema.organization).values({ id: ORG, name: 'Acme', createdAt: 1 })
    await db.db.insert(schema.kcUser).values([{ id: ADA, email: 'ada@example.com' }, { id: BOB, email: 'bob@example.com' }]).onConflictDoNothing()
    await db.db.insert(schema.kcOrgMember).values([{ orgId: ORG, userId: BOB, role: 'member' }])
    const app = await cloud()
    const forOrg = { user_id: ADA, org_id: ORG, token: checkoutToken(SECRET, ADA, ORG) }
    const team = (name: string) => event(name, { variant_id: 222 }, forOrg, '9100')
    // Ada is no (longer) a member: the genuine, signed event does not attach to the organization.
    expect((await webhook(app, team('subscription_created'))).json().status).toBe('ignored')
    await db.db.insert(schema.kcOrgMember).values({ orgId: ORG, userId: ADA, role: 'owner' })
    expect((await webhook(app, team('subscription_created'))).json()).toEqual({ status: 'applied' })
    const portal = async (sub: string, role: string) =>
      (await app.inject({ url: '/v1/billing', headers: await authed(sub, [`/orgs/${ORG}/roles/${role}`]) })).json().portalUrl
    expect(await portal(ADA, 'owner')).toBe('https://store.example/portal/abc')
    expect(await portal(BOB, 'member')).toBeNull()
    // Ada steps down: renewals of the subscription still go through.
    await db.db.update(schema.kcOrgMember).set({ role: 'member' }).where(eq(schema.kcOrgMember.userId, ADA))
    expect((await webhook(app, event('subscription_updated', { variant_id: 222, updated_at: iso(now + 5000) }, forOrg, '9100'))).json().status).toBe('applied')
    await app.close()
  })

  it('exists only in the cloud', async () => {
    const app = await buildApp(loadConfig({ NODE_ENV: 'test', LOG_LEVEL: 'silent', METRICS_ENABLED: 'false' }), { logger: false, keys, readiness: {}, db: db.db })
    expect((await app.inject({ url: '/v1/billing/plans', headers: await authed() })).statusCode).toBe(404)
    await app.close()
  })

  it('rejects an unsigned or wrongly signed webhook', async () => {
    const app = await cloud()
    expect((await webhook(app, event('subscription_created'), 'wrong-secret')).statusCode).toBe(401)
    const unsigned = await app.inject({ method: 'POST', url: '/v1/billing/webhook', headers: { 'content-type': 'application/json' }, payload: '{}' })
    expect(unsigned.statusCode).toBe(401)
    await app.close()
  })

  it('turns a subscription into a Pro entitlement the user then gets, and lapses it', async () => {
    const app = await cloud({ DEFAULT_PLAN: 'free' })
    const created = await webhook(app, event('subscription_created', { updated_at: iso(Date.now() - 5000), renews_at: iso(Date.now() + 30 * DAY) }))
    expect(created.json()).toEqual({ status: 'applied' })

    const ent = (await app.inject({ url: '/v1/entitlement', headers: await authed() })).json()
    expect(ent.plan).toBe('pro')
    expect(ent.features).toEqual(['sync'])

    const sub = (await app.inject({ url: '/v1/billing', headers: await authed() })).json()
    expect(sub).toMatchObject({ plan: 'pro', status: 'active', portalUrl: 'https://store.example/portal/abc' })

    // A late, older event does not undo a newer one.
    const stale = await webhook(app, event('subscription_updated', { status: 'expired', updated_at: iso(Date.now() - 60_000) }))
    expect(stale.json().status).toBe('ignored')
    expect((await app.inject({ url: '/v1/entitlement', headers: await authed() })).json().plan).toBe('pro')

    // Expiry ends it.
    const expired = await webhook(app, event('subscription_expired', { status: 'expired', ends_at: iso(Date.now() - 1000), updated_at: iso(Date.now() + 1000) }))
    expect(expired.json().status).toBe('applied')
    expect((await app.inject({ url: '/v1/entitlement', headers: await authed() })).json().plan).toBe('free')
    await app.close()
  })

  it('does not let an older event overwrite a newer stored one, even when the earlier check passed', async () => {
    const app = await cloud()
    const parse = (name: string, attributes: Record<string, unknown>) => {
      const r = parseEvent(event(name, attributes, undefined, '9777'), PLANS, Date.now(), SECRET)
      if (!r.ok) throw new Error(r.reason)
      return r.update
    }
    const newer = parse('subscription_updated', { updated_at: iso(Date.now() - 1000) })
    const older = parse('subscription_updated', { status: 'expired', ends_at: iso(Date.now() - 1000), updated_at: iso(Date.now() - 60_000) })
    expect((await webhook(app, event('subscription_updated', { updated_at: iso(newer.updatedAt) }, undefined, '9777'))).json().status).toBe('applied')
    const stored = () => db.db.select().from(schema.entitlement).where(eq(schema.entitlement.id, newer.id))
    const before = await stored()

    // The write step after a pre-check that passed before the newer event landed.
    expect(await db.db.transaction((tx) => writeSubscription(tx, older))).toBe(false)
    expect(await stored()).toEqual(before)
    // A replay of the stored event (equal timestamp) still applies.
    expect(await db.db.transaction((tx) => writeSubscription(tx, newer))).toBe(true)
    await app.close()
  })

  it('ignores events it cannot place, with a 200 so the provider stops retrying', async () => {
    const app = await cloud()
    const res = await webhook(app, event('subscription_created', { variant_id: 999 }))
    expect(res.statusCode).toBe(200)
    expect(res.json().status).toBe('ignored')
    await app.close()
  })

  it('lists plans and builds a checkout link; organizations are for their owner', async () => {
    const app = await cloud()
    expect((await app.inject({ url: '/v1/billing/plans', headers: await authed() })).json()).toEqual([
      { key: 'pro_monthly', plan: 'pro' },
      { key: 'team_yearly', plan: 'team' },
    ])
    const link = (await app.inject({ method: 'POST', url: '/v1/billing/checkout', headers: await authed(), payload: { key: 'pro_monthly' } })).json()
    expect(new URL(link.url).searchParams.get('checkout[custom][user_id]')).toBe(ADA)
    expect((await app.inject({ method: 'POST', url: '/v1/billing/checkout', headers: await authed(), payload: { key: 'nope' } })).statusCode).toBe(404)

    const forOrg = async (role: string) =>
      app.inject({
        method: 'POST',
        url: '/v1/billing/checkout',
        headers: await authed(ADA, [`/orgs/${ORG}/roles/${role}`]),
        payload: { key: 'team_yearly', orgId: ORG },
      })
    expect((await forOrg('member')).statusCode).toBe(403)
    expect((await forOrg('owner')).statusCode).toBe(200)
    await app.close()
  })

  it('knows when purchases are not set up', async () => {
    const app = await cloud({ BILLING_STORE_URL: '', BILLING_WEBHOOK_SECRET: '' })
    expect((await app.inject({ method: 'POST', url: '/v1/billing/checkout', headers: await authed(), payload: { key: 'pro_monthly' } })).statusCode).toBe(503)
    expect((await app.inject({ method: 'POST', url: '/v1/billing/webhook', headers: { 'content-type': 'application/json' }, payload: '{}' })).statusCode).toBe(503)
    await app.close()
  })
})
