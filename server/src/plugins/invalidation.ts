// Where a cache invalidation is announced to the other replicas. The `forget*` functions drop an entry in this process
// and call `announce`; the bus plugin registers a sink that publishes it. Receivers of such a message use the local-only
// `drop*` functions, so an invalidation is never announced twice (no loops).

export type Invalidation =
  | { k: 'inv'; what: 'membership'; user: string }
  | { k: 'inv'; what: 'roles'; org: string }
  /** Everyone remembered as a member of `org` is dropped, and so are `users` (people just added, who the cache does not yet list there); `all` empties the whole membership cache. */
  | { k: 'inv'; what: 'orgMemberships'; org: string; users?: string[]; all?: true }
  /** One account, or (`all`) every remembered account (a mass switch-off). */
  | { k: 'acct'; user: string }
  | { k: 'acct'; all: true }

/** Most user ids in one membership invalidation (100 * 38 bytes is about 3.8 KB). */
export const MAX_INV_USERS = 100

type Sink = (message: Invalidation) => void
const sinks = new Set<Sink>()

/** Registers a receiver of announcements; returns the function that removes it. */
export function onAnnounce(sink: Sink): () => void {
  sinks.add(sink)
  return () => void sinks.delete(sink)
}

/** Tells the other replicas that something this process just invalidated must be invalidated there too. */
export function announce(message: Invalidation) {
  for (const sink of sinks) {
    try {
      sink(message)
    } catch {
      // A hint only: the cache entry expires on its own.
    }
  }
}
