// Per-key budgets over a sliding minute, for what the per-address rate limit cannot see: one signed-in
// account (or one SCIM key) using up storage, CPU or connections. Counts are in memory; one API process.

import { HttpProblem } from './errors.ts'

export class Quota {
  private readonly buckets = new Map<string, { used: number; resetAt: number }>()
  private readonly max: number
  private readonly windowMs: number

  constructor(max: number, windowMs = 60_000) {
    this.max = max
    this.windowMs = windowMs
  }

  /** Spends `cost` from the key's budget; false if that would go over it. */
  take(key: string, cost = 1, now = Date.now()): boolean {
    let bucket = this.buckets.get(key)
    if (!bucket || bucket.resetAt <= now) {
      bucket = { used: 0, resetAt: now + this.windowMs }
      this.buckets.set(key, bucket)
      if (this.buckets.size > 10_000) this.sweep(now)
    }
    if (bucket.used + cost > this.max) return false
    bucket.used += cost
    return true
  }

  /** Seconds until the key's budget starts again. */
  retryAfter(key: string, now = Date.now()): number {
    const bucket = this.buckets.get(key)
    return bucket ? Math.max(1, Math.ceil((bucket.resetAt - now) / 1000)) : 1
  }

  /** Spends or throws the 429 the clients already know how to wait out. */
  spend(key: string, cost = 1, what = 'requests'): void {
    if (!this.take(key, cost)) {
      throw new HttpProblem(429, 'quota_exceeded', `this account has used its budget of ${what} for the minute; try again shortly`, { 'retry-after': String(this.retryAfter(key)) })
    }
  }

  private sweep(now: number) {
    for (const [key, bucket] of this.buckets) if (bucket.resetAt <= now) this.buckets.delete(key)
  }
}
