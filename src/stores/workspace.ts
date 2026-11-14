import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { commands, type WorkspaceView } from '@/bindings'
import { call } from '@/lib/ipc'
import { CAN, useOrgStore } from '@/stores/org'

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
  /**
   * Whether the person may change shared things (projects, tags, cost
   * centers, activities, policies) here. Always in their own workspace; in an
   * organization's, owners and admins (the server enforces it).
   */
  const canEditShared = computed(() => !inOrg.value || (role.value !== null && CAN.manage(role.value)))

  async function load() {
    all.value = await call(commands.workspaceList())
    if (all.value.some((w) => w.kind === 'org')) await org.loadOrgs().catch(() => {})
  }

  async function switchTo(id: string) {
    await call(commands.workspaceSwitch(id))
    await load()
  }

  return { all, active, inOrg, switchable, role, canEditShared, load, switchTo }
})
