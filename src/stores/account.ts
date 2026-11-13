import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { commands, events, type AccountStatus, type ConflictView, type JoinStrategy, type PlanInfo, type PlanOffer, type Subscription, type SyncStatus } from '@/bindings'
import { call } from '@/lib/ipc'

interface SyncSummary {
  key: string
  params?: Record<string, string | number>
  problem: boolean
}

const SIGNED_OUT: AccountStatus = { serverUrl: null, signedIn: false, user: null }

/** The Zeima account: server, sign-in and who is signed in. */
export const useAccountStore = defineStore('account', () => {
  const status = ref<AccountStatus>(SIGNED_OUT)
  const busy = ref(false)
  const sync = ref<SyncStatus | null>(null)
  const conflicts = ref<ConflictView[]>([])
  const plan = ref<PlanInfo | null>(null)
  /** What can be bought here; empty on servers without billing. */
  const offers = ref<PlanOffer[]>([])
  const subscription = ref<Subscription | null>(null)

  async function load() {
    try {
      status.value = await call(commands.accountStatus())
    } catch {
      // Keychain or server trouble must not break the app.
      status.value = SIGNED_OUT
    }
  }

  async function loadSync() {
    try {
      sync.value = await call(commands.syncStatus())
      plan.value = await call(commands.planStatus())
      if (status.value.signedIn) {
        offers.value = await call(commands.billingPlans()).catch(() => [])
        subscription.value = offers.value.length ? await call(commands.billingSubscription()).catch(() => null) : null
      } else {
        offers.value = []
        subscription.value = null
      }
      conflicts.value = sync.value.conflicts > 0 ? await call(commands.syncConflicts()) : []
    } catch {
      sync.value = null
      conflicts.value = []
    }
  }

  /** Tries the server's version again; true if it was written. */
  async function retryConflict(c: ConflictView) {
    const written = await call(commands.syncRetryConflict(c.table, c.rowId))
    await loadSync()
    return written
  }

  async function dismissConflict(c: ConflictView) {
    await call(commands.syncDismissConflict(c.table, c.rowId))
    await loadSync()
  }

  /** Syncs now; throws with the server's message on failure. */
  async function syncNow() {
    try {
      sync.value = await call(commands.syncNow())
    } finally {
      await loadSync()
    }
  }

  /** How this device's data and the account's go together (first sign-in on a device that has data). */
  async function join(strategy: JoinStrategy) {
    try {
      sync.value = await call(commands.syncJoin(strategy))
    } finally {
      await loadSync()
    }
  }

  async function setServer(url: string | null) {
    status.value = await call(commands.accountSetServer(url))
  }

  /** Resolves when the browser sign-in is done; throws on failure or cancel. */
  async function signIn() {
    busy.value = true
    try {
      status.value = await call(commands.accountSignIn())
    } finally {
      busy.value = false
    }
  }

  const cancel = () => call(commands.accountCancel()).catch(() => {})

  async function signOut() {
    status.value = await call(commands.accountSignOut())
  }

  /** One line for the sidebar: what the sync is doing or what needs attention. */
  const syncSummary = computed<SyncSummary | null>(() => {
    const st = sync.value
    if (!status.value.signedIn || !st) return null
    const line = (key: string, problem: boolean, params?: Record<string, string | number>): SyncSummary => ({ key, problem, params })
    if (st.joinChoice) return line('sync.chooseJoin', true)
    if (st.syncing) return line('sync.syncing', false)
    if (st.lastErrorKind) return line(`sync.error.${st.lastErrorKind}`, true)
    if (st.conflicts > 0) return line('sync.conflicts', true, { n: st.conflicts })
    if (st.pending > 0) return line('sync.pending', false, { n: st.pending })
    if (st.lastSyncedAt) {
      const at = new Date(st.lastSyncedAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
      return line('sync.synced', false, { at })
    }
    return line('sync.waiting', false)
  })

  // The backend says when a sync starts or ends; keep the status current.
  void events.syncChanged.listen(() => void loadSync())

  return { status, busy, syncSummary, sync, conflicts, plan, offers, subscription, join, retryConflict, dismissConflict, loadSync, syncNow, load, setServer, signIn, cancel, signOut }
})
