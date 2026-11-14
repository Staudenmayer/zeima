<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Users } from '@lucide/vue'
import { commands, type TeamMember } from '@/bindings'
import { Button } from '@/components/ui/button'
import EmptyState from '@/components/time/EmptyState.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import { addDays, formatDuration, startOfDay, startOfWeek } from '@/lib/time'
import { call, errorMessage } from '@/lib/ipc'
import { useOrgStore } from '@/stores/org'
import { useProjectsStore } from '@/stores/projects'
import { useSettingsStore } from '@/stores/settings'
import { useSubmissionsStore } from '@/stores/submissions'
import { useWorkspaceStore } from '@/stores/workspace'

definePage({ meta: { titleKey: 'nav.team' } })

const org = useOrgStore()
const projects = useProjectsStore()
const settings = useSettingsStore()
const submissions = useSubmissionsStore()
const workspaces = useWorkspaceStore()

type Preset = 'thisWeek' | 'lastWeek' | 'thisMonth' | 'lastMonth'
const PRESETS: Preset[] = ['thisWeek', 'lastWeek', 'thisMonth', 'lastMonth']
const preset = ref<Preset>('thisWeek')

function range(p: Preset) {
  const now = Date.now()
  const week = startOfWeek(now)
  const d = new Date(startOfDay(now))
  const month = (offset: number) => ({
    from: startOfDay(new Date(d.getFullYear(), d.getMonth() + offset, 1, 12).getTime()),
    to: startOfDay(new Date(d.getFullYear(), d.getMonth() + offset + 1, 1, 12).getTime()),
  })
  switch (p) {
    case 'thisWeek':
      return { from: week, to: addDays(week, 7) }
    case 'lastWeek':
      return { from: addDays(week, -7), to: week }
    case 'thisMonth':
      return month(0)
    default:
      return month(-1)
  }
}

const team = ref<TeamMember[]>([])
const loading = ref(false)
const hours = (ms: number) => formatDuration(ms, settings.settings?.durationFormat ?? 'hm')
const total = computed(() => team.value.reduce((n, m) => n + m.totalMs, 0))
const billable = computed(() => team.value.reduce((n, m) => n + m.billableMs, 0))
const nameOf = (id: string) => org.nameOf(id)
const projectName = (id: string | null) => (id ? (projects.byId.get(id)?.name ?? '') : '')
const waiting = computed(() => submissions.all.filter((s) => s.status === 'submitted').length)
const widest = computed(() => Math.max(1, ...team.value.map((m) => m.totalMs)))

async function load() {
  if (!workspaces.inOrg) return
  loading.value = true
  try {
    if (org.members.length === 0) await org.load().catch(() => {})
    await submissions.load().catch(() => {})
    team.value = await call(commands.teamSummary(range(preset.value)))
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    loading.value = false
  }
}
onMounted(load)
watch([preset, () => workspaces.active?.id], load)
</script>

<template>
  <PageHeader :title="$t('nav.team')">
    <div class="flex gap-1">
      <Button v-for="p in PRESETS" :key="p" size="sm" :variant="preset === p ? 'default' : 'outline'" @click="preset = p">{{ $t(`team.${p}`) }}</Button>
    </div>
  </PageHeader>

  <EmptyState v-if="!workspaces.inOrg" :icon="Users" :text="$t('team.personal')" />

  <div v-else class="grid gap-4">
    <div class="flex flex-wrap gap-6 text-sm">
      <div><p class="text-xs text-muted-foreground">{{ $t('team.total') }}</p><p class="text-2xl font-semibold tabular-nums">{{ hours(total) }}</p></div>
      <div><p class="text-xs text-muted-foreground">{{ $t('team.billable') }}</p><p class="text-2xl font-semibold tabular-nums">{{ hours(billable) }}</p></div>
      <div v-if="waiting"><p class="text-xs text-muted-foreground">{{ $t('team.waiting') }}</p>
        <RouterLink to="/approvals" class="text-2xl font-semibold tabular-nums hover:underline">{{ waiting }}</RouterLink>
      </div>
    </div>

    <p v-if="!loading && team.length === 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('team.empty') }}</p>
    <ul v-else class="grid gap-3">
      <li v-for="m in team" :key="m.userId" class="rounded-lg border p-3">
        <div class="flex items-baseline gap-3">
          <span class="flex-1 font-medium">{{ nameOf(m.userId) || $t('team.someone') }}</span>
          <span class="text-sm tabular-nums">{{ hours(m.totalMs) }}</span>
          <span class="w-24 text-right text-xs tabular-nums text-muted-foreground">{{ $t('team.billableShort', { h: hours(m.billableMs) }) }}</span>
        </div>
        <div class="mt-2 h-1.5 rounded-full bg-muted"><div class="h-full rounded-full bg-primary" :style="{ width: `${(m.totalMs / widest) * 100}%` }" /></div>
        <ul class="mt-3 grid gap-1 text-sm">
          <li v-for="p in m.projects" :key="p.projectId ?? 'none'" class="flex items-center gap-2">
            <span class="size-2.5 rounded-full" :style="{ backgroundColor: p.projectId ? projects.byId.get(p.projectId)?.color : 'var(--muted-foreground)' }" />
            <span class="flex-1 truncate">{{ projectName(p.projectId) || $t('team.noProject') }}</span>
            <span class="tabular-nums text-muted-foreground">{{ hours(p.ms) }}</span>
          </li>
        </ul>
      </li>
    </ul>
    <p class="text-xs text-muted-foreground">{{ $t('team.hint') }}</p>
  </div>
</template>
