<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { ChevronDown } from '@lucide/vue'
import { events } from '@/bindings'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { useOrgStore } from '@/stores/org'
import { useProjectsStore } from '@/stores/projects'
import { useWorkspaceStore } from '@/stores/workspace'

const { t } = useI18n()
const org = useOrgStore()
const projects = useProjectsStore()
const workspaces = useWorkspaceStore()
const COLORS: Record<string, string> = { online: '#22c55e', tracking: '#3b82f6', in_meeting: '#f59e0b', away: '#9ca3af' }

/** Departments to show (empty = everyone); a department includes the ones inside it. */
const selected = ref<string[]>([])
const topLevel = computed(() => org.departments.filter((g) => !g.path.includes('/')))
const inDepartment = (path: string, userId: string) =>
  org.departments.some((g) => (g.path === path || g.path.startsWith(`${path}/`)) && g.memberIds.includes(userId))
const shown = () =>
  org.presence.filter((p) => selected.value.length === 0 || selected.value.some((path) => inDepartment(path, p.userId)))
const toggleDepartment = (path: string) =>
  (selected.value = selected.value.includes(path) ? selected.value.filter((x) => x !== path) : [...selected.value, path])
const filterLabel = computed(() => {
  const picked = topLevel.value.filter((g) => selected.value.includes(g.path))
  return picked.length === 0 ? t('org.allDepartments') : picked.length === 1 ? picked[0]!.name : t('org.nDepartments', { n: picked.length })
})

async function load() {
  const orgId = workspaces.active?.orgId
  if (orgId) await org.loadGlance(orgId).catch(() => {})
}
let unlisten: (() => void) | undefined
onMounted(async () => {
  await load()
  unlisten = await events.presenceChanged.listen((e) => {
    if (e.payload.orgId === org.activeId) void org.loadPresence().catch(() => {})
  })
})
onUnmounted(() => unlisten?.())
watch(() => workspaces.active?.id, load)
</script>

<template>
  <Card v-if="workspaces.inOrg">
    <CardHeader>
      <CardDescription>{{ $t('org.tabs.online') }}</CardDescription>
      <CardTitle class="flex items-center justify-between gap-3 text-base">
        <span>{{ $t('org.onlineCount', shown().length) }}</span>
        <DropdownMenu v-if="topLevel.length">
          <DropdownMenuTrigger as-child>
            <Button variant="outline" size="sm" class="h-7 gap-1.5 text-xs font-normal" :title="$t('org.filterDepartments')">
              {{ filterLabel }} <ChevronDown class="size-3.5" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" class="min-w-52">
            <DropdownMenuCheckboxItem :model-value="selected.length === 0" @select.prevent="selected = []">{{ $t('org.allDepartments') }}</DropdownMenuCheckboxItem>
            <DropdownMenuCheckboxItem
              v-for="g in topLevel"
              :key="g.id"
              :model-value="selected.includes(g.path)"
              @select.prevent="toggleDepartment(g.path)"
            >{{ g.name }}</DropdownMenuCheckboxItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </CardTitle>
    </CardHeader>
    <CardContent>
      <p v-if="shown().length === 0" class="text-sm text-muted-foreground">{{ $t('org.nobodyOnline') }}</p>
      <ul v-else class="grid gap-1.5 text-sm sm:grid-cols-2">
        <li v-for="p in shown()" :key="p.userId" class="flex items-center gap-2">
          <span class="size-2.5 shrink-0 rounded-full" :style="{ backgroundColor: COLORS[p.state] }" />
          <span class="truncate">{{ org.nameOf(p.userId) || $t('team.someone') }}</span>
          <span v-if="p.projectId" class="truncate text-xs text-muted-foreground">{{ projects.byId.get(p.projectId)?.name }}</span>
        </li>
      </ul>
    </CardContent>
  </Card>
</template>
