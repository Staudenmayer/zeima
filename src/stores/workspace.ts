import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { commands, type WorkspaceView } from '@/bindings'
import { call } from '@/lib/ipc'
import { useOrgStore } from '@/stores/org'

/** The workspace the person works in: their own, or an organization's shared one. */
export const useWorkspaceStore = defineStore('workspace', () => {
  const all = ref<WorkspaceView[]>([])
  const org = useOrgStore()

  const active = computed(() => all.value.find((w) => w.active) ?? null)
  const inOrg = computed(() => active.value?.kind === 'org')
  /** More than one workspace: show the switcher. */
  const switchable = computed(() => all.value.length > 1)
  /** Role in the active organization, if the workspace is an organization's. */
  const role = computed(() => (active.value?.orgId ? (org.orgs.find((o) => o.id === active.value!.orgId)?.role ?? 'member') : null))
  /** The organization the active workspace belongs to. */
  const orgSummary = computed(() => (active.value?.orgId ? org.orgs.find((o) => o.id === active.value!.orgId) : undefined))
  /**
   * Whether the person may do `action` here: always in their own workspace; in
   * an organization's, what their role allows (the server enforces it).
   */
  const can = (action: string) => !inOrg.value || orgSummary.value?.permissions[action] !== undefined
  /** How wide in an organization (own, department, everyone); `org` in the person's own workspace. */
  const scopeOf = (action: string) => (!inOrg.value ? 'org' : (orgSummary.value?.permissions[action] ?? null))


  async function load() {
    all.value = await call(commands.workspaceList())
    if (all.value.some((w) => w.kind === 'org')) await org.loadOrgs().catch(() => {})
  }

  async function switchTo(id: string) {
    await call(commands.workspaceSwitch(id))
    await load()
  }

  return { all, active, inOrg, switchable, role, can, scopeOf, load, switchTo }
})
