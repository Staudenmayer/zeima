<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Users } from '@lucide/vue'
import { commands, type TeamMember, type TimeEntry } from '@/bindings'
import { Button } from '@/components/ui/button'
import EmptyState from '@/components/time/EmptyState.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import { addDays, formatDuration, startOfDay, startOfWeek } from '@/lib/time'
import { call, errorMessage } from '@/lib/ipc'
import { useCostCentersStore } from '@/stores/costCenters'
import { useOrgStore } from '@/stores/org'
import { useTagsStore } from '@/stores/tags'
import { useProjectsStore } from '@/stores/projects'
import { useSettingsStore } from '@/stores/settings'
import { useWorkspaceStore } from '@/stores/workspace'

definePage({ meta: { titleKey: 'nav.team' } })

const org = useOrgStore()
const tags = useTagsStore()
const costCenters = useCostCentersStore()
const projects = useProjectsStore()
const settings = useSettingsStore()
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

const DAY = 86_400_000
const currentRange = computed(() => range(preset.value))
const days = computed(() => Math.round((currentRange.value.to - currentRange.value.from) / DAY))
/** The departments a person is in, as the departments list names them. */
const departmentsOf = (userId: string) => org.departments.filter((g) => g.memberIds.includes(userId)).map((g) => g.path)
/** The smallest weekly target of the organization and the person's departments, in minutes. */
function weeklyTarget(userId: string): number | null {
  const mine = departmentsOf(userId)
  const targets = org.policies
    .filter((p) => p.groupPath === null || mine.includes(p.groupPath))
    .map((p) => p.weeklyTargetMinutes)
    .filter((m): m is number => typeof m === 'number' && m > 0)
  return targets.length ? Math.min(...targets) : null
}
/** Tracked minus target for the shown period, in ms; `null` without a target. */
function overtime(m: TeamMember): number | null {
  const target = weeklyTarget(m.userId)
  return target === null ? null : m.totalMs - ((target * 60_000 * days.value) / 7)
}
/** A difference of more than a fifth of the target is worth a look. */
const outlier = (m: TeamMember) => {
  const delta = overtime(m)
  const target = weeklyTarget(m.userId)
  return delta !== null && target !== null && Math.abs(delta) > ((target * 60_000 * days.value) / 7) * 0.2
}
const signed = (ms: number) => `${ms < 0 ? '−' : '+'}${hours(Math.abs(ms))}`
const outliers = computed(() => team.value.filter(outlier))

/** Time per department: the people of each, as far as this device knows them. */
const departmentTotals = computed(() =>
  org.departments
    .map((g) => {
      const people = team.value.filter((m) => g.memberIds.includes(m.userId))
      return { path: g.path, total: people.reduce((n, m) => n + m.totalMs, 0), billable: people.reduce((n, m) => n + m.billableMs, 0), count: people.length }
    })
    .filter((g) => g.count > 0),
)

const tagName = (id: string) => tags.all.find((t) => t.id === id)?.name ?? ''
const costCenterCode = (id: string) => costCenters.all.find((c) => c.id === id)?.code ?? ''

// One person's entries, opened on demand.
const opened = ref<string | null>(null)
const entries = ref<TimeEntry[]>([])
async function toggle(userId: string) {
  if (opened.value === userId) {
    opened.value = null
    return
  }
  opened.value = userId
  entries.value = []
  try {
    entries.value = await call(commands.teamEntries(userId, currentRange.value))
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
const entryTime = (e: TimeEntry) => `${new Date(e.startAt).toLocaleString([], { weekday: 'short', day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' })} · ${hours((e.endAt ?? e.startAt) - e.startAt)}`

const widest = computed(() => Math.max(1, ...team.value.map((m) => m.totalMs)))

async function load() {
  if (!workspaces.inOrg) return
  loading.value = true
  try {
    if (org.members.length === 0) await org.load().catch(() => {})
    await Promise.all([org.loadPolicies().catch(() => {}), tags.load().catch(() => {}), costCenters.load().catch(() => {})])
    team.value = await call(commands.teamSummary(range(preset.value)))
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    loading.value = false
  }
}
onMounted(load)
watch([preset, () => workspaces.active?.id], () => {
  opened.value = null
  void load()
})
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
    </div>

    <section v-if="departmentTotals.length" class="grid gap-1">
      <h2 class="text-sm font-medium">{{ $t('team.departments') }}</h2>
      <ul class="divide-y rounded-lg border text-sm">
        <li v-for="g in departmentTotals" :key="g.path" class="flex items-center gap-3 px-3 py-1.5">
          <span class="flex-1">{{ g.path }} <span class="text-xs text-muted-foreground">{{ $t('team.people', g.count) }}</span></span>
          <span class="tabular-nums">{{ hours(g.total) }}</span>
          <span class="w-24 text-right text-xs tabular-nums text-muted-foreground">{{ $t('team.billableShort', { h: hours(g.billable) }) }}</span>
        </li>
      </ul>
    </section>

    <section v-if="outliers.length" class="rounded-lg border border-amber-500/40 bg-amber-500/5 p-3 text-sm">
      <h2 class="font-medium">{{ $t('team.outliers') }}</h2>
      <p class="mt-1 text-muted-foreground">
        <span v-for="(m, i) in outliers" :key="m.userId">{{ i ? ', ' : '' }}{{ nameOf(m.userId) || $t('team.someone') }} ({{ signed(overtime(m)!) }})</span>
      </p>
    </section>

    <p v-if="!loading && team.length === 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('team.empty') }}</p>
    <ul v-else class="grid gap-3">
      <li v-for="m in team" :key="m.userId" class="rounded-lg border p-3">
        <div class="flex items-baseline gap-3">
          <span class="flex-1 font-medium">{{ nameOf(m.userId) || $t('team.someone') }}</span>
          <span class="text-sm tabular-nums">{{ hours(m.totalMs) }}</span>
          <span v-if="overtime(m) !== null" class="text-xs tabular-nums" :class="outlier(m) ? 'font-medium text-amber-600' : 'text-muted-foreground'">{{ signed(overtime(m)!) }}</span>
          <span class="w-24 text-right text-xs tabular-nums text-muted-foreground">{{ $t('team.billableShort', { h: hours(m.billableMs) }) }}</span>
          <Button size="sm" variant="ghost" @click="toggle(m.userId)">{{ opened === m.userId ? $t('team.hideEntries') : $t('team.entries') }}</Button>
        </div>
        <div class="mt-2 h-1.5 rounded-full bg-muted"><div class="h-full rounded-full bg-primary" :style="{ width: `${(m.totalMs / widest) * 100}%` }" /></div>
        <ul class="mt-3 grid gap-1 text-sm">
          <li v-for="p in m.projects" :key="p.projectId ?? 'none'" class="flex items-center gap-2">
            <span class="size-2.5 rounded-full" :style="{ backgroundColor: p.projectId ? projects.byId.get(p.projectId)?.color : 'var(--muted-foreground)' }" />
            <span class="flex-1 truncate">{{ projectName(p.projectId) || $t('team.noProject') }}</span>
            <span class="tabular-nums text-muted-foreground">{{ hours(p.ms) }}</span>
          </li>
        </ul>
        <p v-if="m.tags.length" class="mt-2 text-xs text-muted-foreground">
          {{ $t('team.tags') }}: <span v-for="(t, i) in m.tags" :key="t.id">{{ i ? ', ' : '' }}{{ tagName(t.id) || '·' }} {{ hours(t.ms) }}</span>
        </p>
        <p v-if="m.costCenters.length" class="mt-1 text-xs text-muted-foreground">
          {{ $t('team.costCenters') }}: <span v-for="(c, i) in m.costCenters" :key="c.id">{{ i ? ', ' : '' }}{{ costCenterCode(c.id) || '·' }} {{ hours(c.ms) }}</span>
        </p>
        <ul v-if="opened === m.userId" class="mt-3 divide-y rounded-md border text-xs">
          <li v-for="e in entries" :key="e.id" class="flex items-center gap-3 px-2 py-1.5">
            <span class="w-52 tabular-nums text-muted-foreground">{{ entryTime(e) }}</span>
            <span class="w-32 truncate">{{ projectName(e.projectId) }}</span>
            <span class="flex-1 truncate text-muted-foreground">{{ e.note }}</span>
          </li>
          <li v-if="entries.length === 0" class="px-2 py-2 text-muted-foreground">{{ $t('team.noEntries') }}</li>
        </ul>
      </li>
    </ul>
    <p class="text-xs text-muted-foreground">{{ $t('team.hint') }}</p>
  </div>
</template>
