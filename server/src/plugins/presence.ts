// Who is online, tracking or away in an organization (SPEC §5.10). Presence
// is kept in memory: it is only true while a device is connected and
// reporting, so it is worth nothing after a restart. Devices report on every
// change and about once a minute; a report older than `TTL_MS` no longer counts.

export const PRESENCE_STATES = ['online', 'in_meeting', 'traveling', 'away'] as const
export type PresenceState = (typeof PRESENCE_STATES)[number]

/** A device reports at least every minute. */
export const TTL_MS = 150_000

export interface PresenceEntry {
  state: PresenceState
  /** Set only when the person chose to share what they work on. */
  projectId: string | null
  /** When the state last changed, unix ms. */
  since: number
  /** Last report, unix ms. */
  seen: number
}

export class PresenceBoard {
  private readonly orgs = new Map<string, Map<string, PresenceEntry>>()

  /** Records a report; returns whether something visible changed. */
  set(orgId: string, userId: string, state: PresenceState, projectId: string | null, now: number): boolean {
    let org = this.orgs.get(orgId)
    if (!org) {
      org = new Map()
      this.orgs.set(orgId, org)
    }
    const before = org.get(userId)
    const changed = !before || before.state !== state || before.projectId !== projectId || now - before.seen > TTL_MS
    org.set(userId, { state, projectId, since: before && before.state === state ? before.since : now, seen: now })
    return changed
  }

  /** Removes the person from one organization (or all of them). Returns the organizations they were in. */
  remove(userId: string, orgId?: string): string[] {
    const was: string[] = []
    for (const [id, org] of this.orgs) {
      if ((orgId === undefined || orgId === id) && org.delete(userId)) was.push(id)
      if (org.size === 0) this.orgs.delete(id)
    }
    return was
  }

  /** Who is present in the organization: reports that are fresh, from people with a connected device. */
  list(orgId: string, now: number, connected: (userId: string) => boolean) {
    const out: { userId: string; state: PresenceState; projectId: string | null; since: number }[] = []
    for (const [userId, e] of this.orgs.get(orgId) ?? []) {
      if (now - e.seen > TTL_MS || !connected(userId)) continue
      out.push({ userId, state: e.state, projectId: e.projectId, since: e.since })
    }
    return out.sort((a, b) => a.userId.localeCompare(b.userId))
  }
}
