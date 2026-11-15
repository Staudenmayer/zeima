<script setup lang="ts">
import { useStorage } from '@vueuse/core'
import { ChevronDown } from '@lucide/vue'
import { computed, onMounted, onUnmounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { events } from '@/bindings'
import PersonPopover from '@/components/time/PersonPopover.vue'
import UserAvatar from '@/components/time/UserAvatar.vue'
import { PRESENCE_COLORS } from '@/lib/presence'
import { type PresenceItem, useOrgStore } from '@/stores/org'
import { useProjectsStore } from '@/stores/projects'
import { useWorkspaceStore } from '@/stores/workspace'

// Who is online, like the member list of a chat app: a column on the right of every page in an
// organization's workspace, with a separator per department. Clicking a person shows their profile.
const { t } = useI18n()
const org = useOrgStore()
const projects = useProjectsStore()
const workspaces = useWorkspaceStore()

const name = (id: string) => org.nameOf(id) || t('team.someone')
const project = (id: string | null) => (id ? (projects.byId.get(id)?.name ?? '') : '')
const departmentOf = (id: string) => org.directory.find((e) => e.id === id)?.departments[0] ?? null

/** A member of the organization as the list shows them: online (with what they report) or offline. */
interface Row {
  userId: string
  state: PresenceItem['state'] | 'offline'
  projectId: string | null
}
/** Departments the person folded away (remembered). */
const collapsed = useStorage<string[]>('onlineSidebar.collapsed', [])
const NONE = '\u0000none'
const keyOf = (department: string | null) => department ?? NONE
const isCollapsed = (department: string | null) => collapsed.value.includes(keyOf(department))
const toggle = (department: string | null) => {
  const key = keyOf(department)
  collapsed.value = collapsed.value.includes(key) ? collapsed.value.filter((k) => k !== key) : [...collapsed.value, key]
}

/** Everyone in the organization: those who report presence, then the others as offline. */
const everyone = computed<Row[]>(() => {
  const online = new Map(org.presence.map((p) => [p.userId, p]))
  const rest = org.directory.filter((e) => !online.has(e.id)).map((e): Row => ({ userId: e.id, state: 'offline', projectId: null }))
  return [...org.presence.map((p): Row => ({ userId: p.userId, state: p.state, projectId: p.projectId })), ...rest]
})
const onlineCount = computed(() => org.presence.length)

/**
 * Own department first, then the departments by how many are working on a project, people without
 * a department last. Within one: the online people (those working on a project first, then by name),
 * then the offline ones by name.
 */
const groups = computed(() => {
  const by = new Map<string | null, Row[]>()
  for (const p of everyone.value) by.set(departmentOf(p.userId), [...(by.get(departmentOf(p.userId)) ?? []), p])
  const mine = org.myId ? departmentOf(org.myId) : null
  const active = (list: Row[]) => list.filter((p) => p.projectId).length
  const rank = (p: Row) => (p.state === 'offline' ? 0 : p.projectId ? 2 : 1)
  return [...by.entries()]
    .sort(([x, lx], [y, ly]) =>
      x === null ? 1 : y === null ? -1 : x === mine ? -1 : y === mine ? 1 : active(ly) - active(lx) || x.localeCompare(y),
    )
    .map(([department, list]) => ({
      department,
      online: list.filter((p) => p.state !== 'offline').length,
      list: [...list].sort((a, b) => rank(b) - rank(a) || name(a.userId).localeCompare(name(b.userId))),
    }))
})

async function load() {
  const orgId = workspaces.active?.orgId
  if (orgId) await org.loadGlance(orgId).catch(() => {})
}
let unlisten: (() => void) | undefined
let refresh: ReturnType<typeof setInterval> | undefined
onMounted(async () => {
  await load()
  unlisten = await events.presenceChanged.listen((e) => {
    if (e.payload.orgId === org.activeId) void org.loadPresence().catch(() => {})
  })
  // A quiet minute still ages out stale reports.
  refresh = setInterval(() => void org.loadPresence().catch(() => {}), 60_000)
})
onUnmounted(() => {
  unlisten?.()
  clearInterval(refresh)
})
watch(() => workspaces.active?.id, load)
</script>

<template>
  <aside :aria-label="$t('org.tabs.online')" class="sticky top-0 h-svh w-60 shrink-0 overflow-y-auto border-l bg-sidebar px-2 py-4">
    <p class="px-2 pb-2 text-xs font-semibold text-muted-foreground">{{ $t('org.onlineCount', onlineCount) }}</p>
    <section v-for="g in groups" :key="g.department ?? ''" class="mb-3">
      <h3 v-if="groups.length > 1 || g.department" class="pt-2 pb-1 text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">
        <button
          type="button"
          class="flex w-full items-center gap-1 rounded px-2 py-0.5 text-left uppercase hover:text-foreground"
          :aria-expanded="!isCollapsed(g.department)"
          :title="$t(isCollapsed(g.department) ? 'directory.expand' : 'directory.collapse')"
          @click="toggle(g.department)"
        >
          <ChevronDown class="size-3 shrink-0 transition-transform" :class="isCollapsed(g.department) ? '-rotate-90' : ''" />
          <span class="truncate">{{ g.department ?? $t('directory.noDepartment') }} - {{ g.online }}</span>
        </button>
      </h3>
      <ul v-show="!isCollapsed(g.department) || (groups.length === 1 && !g.department)" class="grid gap-0.5">
        <li v-for="p in g.list" :key="p.userId">
          <PersonPopover :user-id="p.userId">
            <button type="button" class="flex w-full min-w-0 items-center gap-2.5 rounded-lg px-2 py-1.5 text-left hover:bg-muted/60" :class="p.state === 'offline' ? 'opacity-60' : ''" :title="$t('directory.showProfile')">
              <span class="relative inline-flex shrink-0">
                <UserAvatar :user-id="p.userId" :name="name(p.userId)" size="md" />
                <span
                  class="absolute -right-0.5 -bottom-0.5 aspect-square size-3 rounded-full ring-2 ring-sidebar"
                  :style="{ backgroundColor: PRESENCE_COLORS[p.state] }"
                  :title="p.state === 'offline' ? $t('org.offline') : $t(`org.presenceStates.${p.state}`)"
                />
              </span>
              <span class="grid min-w-0 leading-4">
                <span class="truncate font-medium">{{ name(p.userId) }}</span>
                <span class="truncate text-xs text-muted-foreground">{{ project(p.projectId) || (p.state === 'offline' ? $t('org.offline') : $t(`org.presenceStates.${p.state}`)) }}</span>
              </span>
            </button>
          </PersonPopover>
        </li>
      </ul>
    </section>
    <p v-if="everyone.length === 0" class="px-2 text-sm text-muted-foreground">{{ $t('org.nobodyOnline') }}</p>
  </aside>
</template>
