<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue'
import { events } from '@/bindings'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { useOrgStore } from '@/stores/org'
import { useProjectsStore } from '@/stores/projects'
import { useWorkspaceStore } from '@/stores/workspace'

const org = useOrgStore()
const projects = useProjectsStore()
const workspaces = useWorkspaceStore()
const COLORS: Record<string, string> = { online: '#22c55e', tracking: '#3b82f6', in_meeting: '#f59e0b', away: '#9ca3af' }

/** Filter by department (`''` = everyone). */
const department = ref('')
const shown = () => {
  const group = org.departments.find((g) => g.path === department.value)
  return org.presence.filter((p) => !group || group.memberIds.includes(p.userId))
}

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
        <select v-if="org.departments.length" v-model="department" class="rounded-md border bg-background px-2 py-1 text-xs font-normal">
          <option value="">{{ $t('org.allDepartments') }}</option>
          <option v-for="g in org.departments" :key="g.id" :value="g.path">{{ g.path }}</option>
        </select>
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
