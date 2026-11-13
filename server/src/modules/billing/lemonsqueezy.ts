// Lemon Squeezy subscriptions (a merchant of record that handles EU VAT,
// SPEC §2) as entitlements. Pure: a webhook event in, what to store out.
//
// Events carry `meta.event_name`, `meta.custom_data` (what we put into the
// checkout link: the Keycloak user id and optionally an organization id) and
// `data.attributes` (variant, status, renewal and end dates, portal link).

import { createHash, createHmac, timingSafeEqual } from 'node:crypto'

/** Days a renewing subscription keeps working after its renewal date while the payment goes through. */
export const RENEWAL_SLACK_DAYS = 3

export interface BillingPlan {
  key: string
  plan: string
  variantId: string
  checkoutId: string
}

/** `X-Signature`: hex HMAC-SHA256 of the raw body with the webhook secret. */
export function signatureValid(rawBody: Buffer, header: string | undefined, secret: string): boolean {
  if (!header) return false
  const expected = createHmac('sha256', secret).update(rawBody).digest()
  const given = Buffer.from(header, 'hex')
  return given.length === expected.length && timingSafeEqual(given, expected)
}

/** A stable UUID for a provider id, so a subscription always maps to one entitlement row. */
export function uuidFor(text: string): string {
  const h = createHash('sha1').update(`zeima.lemonsqueezy:${text}`).digest()
  h[6] = (h[6]! & 0x0f) | 0x50 // version 5
  h[8] = (h[8]! & 0x3f) | 0x80 // variant
  const x = h.subarray(0, 16).toString('hex')
  return `${x.slice(0, 8)}-${x.slice(8, 12)}-${x.slice(12, 16)}-${x.slice(16, 20)}-${x.slice(20, 32)}`
}

export interface Update {
  /** Entitlement row id. */
  id: string
  userId: string | null
  orgId: string | null
  email: string | null
  plan: string
  /** Unix ms the features run until; in the past for a lapsed subscription. */
  validUntil: number
  billing: {
    provider: 'lemonsqueezy'
    subscriptionId: string
    customerId: string | null
    status: string
    portalUrl: string | null
    renewsAt: number | null
    endsAt: number | null
  }
  /** When the provider last changed the subscription, to ignore events that arrive late. */
  updatedAt: number
}

export type Parsed = { ok: true; update: Update } | { ok: false; reason: string }

const asObject = (v: unknown): Record<string, unknown> => (typeof v === 'object' && v !== null ? (v as Record<string, unknown>) : {})
const asString = (v: unknown) => (typeof v === 'string' && v !== '' ? v : null)
const asTime = (v: unknown) => {
  const t = typeof v === 'string' ? Date.parse(v) : Number.NaN
  return Number.isNaN(t) ? null : t
}

/** Statuses in which the customer has paid for the current period. */
const PAYING = new Set(['active', 'on_trial', 'past_due'])

export function parseEvent(body: unknown, plans: readonly BillingPlan[], now: number): Parsed {
  const root = asObject(body)
  const meta = asObject(root.meta)
  const event = asString(meta.event_name)
  if (!event?.startsWith('subscription_')) return { ok: false, reason: `not a subscription event (${event ?? 'unknown'})` }

  const data = asObject(root.data)
  const attributes = asObject(data.attributes)
  const subscriptionId = asString(data.id)
  if (!subscriptionId) return { ok: false, reason: 'no subscription id' }

  const custom = asObject(meta.custom_data)
  const userId = asString(custom.user_id)
  const orgId = asString(custom.org_id)
  if (!userId && !orgId) return { ok: false, reason: 'the checkout carried no user or organization' }

  const variantId = String(attributes.variant_id ?? '')
  const plan = plans.find((p) => p.variantId === variantId)
  if (!plan) return { ok: false, reason: `unknown variant ${variantId}` }

  const status = asString(attributes.status) ?? 'expired'
  const renewsAt = asTime(attributes.renews_at)
  const endsAt = asTime(attributes.ends_at)

  let validUntil: number
  if (event === 'subscription_expired' || !(PAYING.has(status) || status === 'cancelled')) {
    validUntil = Math.min(now, endsAt ?? now) // paused, unpaid or expired: over now
  } else if (status === 'cancelled') {
    validUntil = endsAt ?? now // paid until the period ends
  } else {
    validUntil = (renewsAt ?? now) + RENEWAL_SLACK_DAYS * 86_400_000
  }

  const urls = asObject(attributes.urls)
  return {
    ok: true,
    update: {
      id: uuidFor(subscriptionId),
      userId: orgId ? null : userId,
      orgId,
      email: asString(attributes.user_email),
      plan: plan.plan,
      validUntil,
      billing: {
        provider: 'lemonsqueezy',
        subscriptionId,
        customerId: asString(attributes.customer_id) ?? (typeof attributes.customer_id === 'number' ? String(attributes.customer_id) : null),
        status,
        portalUrl: asString(urls.customer_portal),
        renewsAt,
        endsAt,
      },
      updatedAt: asTime(attributes.updated_at) ?? now,
    },
  }
}

/** The link that starts a purchase; the user and organization ride along as custom data. */
export function checkoutUrl(storeUrl: string, plan: BillingPlan, who: { userId: string; orgId?: string; email?: string | null }): string {
  const url = new URL(`${storeUrl}/checkout/buy/${plan.checkoutId}`)
  url.searchParams.set('checkout[custom][user_id]', who.userId)
  if (who.orgId) url.searchParams.set('checkout[custom][org_id]', who.orgId)
  if (who.email) url.searchParams.set('checkout[email]', who.email)
  return url.toString()
}
