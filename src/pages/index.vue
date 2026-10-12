<script setup lang="ts">
import { computed, onScopeDispose, ref } from 'vue'
import { useIntervalFn, useStorage } from '@vueuse/core'
import { ArrowDown, ArrowUp, Check, Clock, Eye, EyeOff, Landmark, Play, Settings2, Target } from '@lucide/vue'
import { commands, events, type BudgetStatus, type Report } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import EmptyState from '@/components/time/EmptyState.vue'
import ProjectDot from '@/components/time/ProjectDot.vue'
import WeekChart from '@/components/time/WeekChart.vue'
import { useEntries } from '@/composables/useEntries'
import { useWorkDays } from '@/composables/useWorkDays'
import { useFormat } from '@/composables/useFormat'
import { call } from '@/lib/ipc'
import { HOUR, dayRange, parseDate, presetRange, startOfDay, toDateInput, weekDays } from '@/lib/time'
import { costCenterLabel, useCostCentersStore } from '@/stores/costCenters'
import { useProjectsStore } from '@/stores/projects'
import { useSettingsStore } from '@/stores/settings'
import { useTimerStore } from '@/stores/timer'
import { useWorkStore } from '@/stores/work'

definePage({ meta: { titleKey: 'nav.dashboard' } })

const projects = useProjectsStore()
const settings = useSettingsStore()
const timer = useTimerStore()
const work = useWorkStore()
const costCenters = useCostCentersStore()
const format = useFormat()

// Widget order and visibility, remembered per device.
type Widget = 'stats' | 'perDay' | 'projects' | 'budgets' | 'costCenters'
const allWidgets: Widget[] = ['stats', 'perDay', 'projects', 'budgets', 'costCenters']
const layout = useStorage('dashboard.layout', { order: allWidgets, hidden: [] as Widget[] }, undefined, {
  mergeDefaults: true,
})
// Widgets added in later versions are appended.
const order = computed(() => [
  ...layout.value.order.filter((w) => allWidgets.includes(w)),
  ...allWidgets.filter((w) => !layout.value.order.includes(w)),
])
const customizing = ref(false)
const visible = computed(() => order.value.filter((w) => customizing.value || !layout.value.hidden.includes(w)))
function move(w: Widget, by: -1 | 1) {
  const list = [...order.value]
  const i = list.indexOf(w)
  const j = i + by
  if (j < 0 || j >= list.length) return
  ;[list[i], list[j]] = [list[j], list[i]]
  layout.value.order = list
}
function toggleHidden(w: Widget) {
  const hidden = layout.value.hidden
  layout.value.hidden = hidden.includes(w) ? hidden.filter((x) => x !== w) : [...hidden, w]
}

// Recomputed at midnight via the minute tick below.
const today = ref(startOfDay(Date.now()))
const days = computed(() => weekDays(today.value))
const week = computed(() => ({ from: days.value[0].from, to: days.value[6].to }))
const ranges = computed(() => [...days.value, week.value])

const { summaries, reload } = useEntries(week, ranges)
const { byDate } = useWorkDays(week)
const todayWork = computed(() => byDate(toDateInput(today.value)))
const balanceMs = computed(() => (work.balance ? work.balance.untilYesterdayMs + work.balance.todayMs : 0))

// Budgets and this month's cost centers.
const budgets = ref<BudgetStatus[]>([])
const monthByCostCenter = ref<Report | null>(null)
async function loadExtras() {
  const month = presetRange('thisMonth', Date.now())
  const [b, r] = await Promise.all([
    call(commands.budgetList()).catch(() => []),
    call(
      commands.report({
        range: month,
        filter: { projectIds: [], tagIds: [], costCenterIds: [], billable: null },
        groupBy: ['costCenter'],
      }),
    ).catch(() => null),
  ])
  budgets.value = b
  monthByCostCenter.value = r
}
void loadExtras()
const unlisten = events.dataChanged.listen((e) => {
  if (e.payload.kinds.some((k) => k === 'entry' || k === 'project' || k === 'costCenter')) void loadExtras()
})
onScopeDispose(() => void unlisten.then((fn) => fn()))

// Running time grows; refresh totals every minute while tracking.
useIntervalFn(() => {
  today.value = startOfDay(Date.now())
  if (timer.running) {
    void reload()
    void loadExtras()
  }
}, 60_000)

const budgetRows = computed(() =>
  budgets.value
    .map((b) => ({
      ...b,
      project: projects.byId.get(b.projectId),
      pct: b.budgetMs > 0 ? (b.usedMs / b.budgetMs) * 100 : 0,
    }))
    .sort((a, b) => b.pct - a.pct),
)

const costCenterRows = computed(() => {
  const rows = monthByCostCenter.value?.rows ?? []
  const total = Math.max(1, monthByCostCenter.value?.totalMs ?? 0)
  return rows.map((r) => ({
    key: r.key,
    label: r.key ? costCenterLabel(costCenters.byId.get(r.key)) || r.key : null,
    ms: r.ms,
    pct: (r.ms / total) * 100,
  }))
})

const todayIndex = computed(() => days.value.findIndex((d) => d.from === dayRange(today.value).from))
const todaySummary = computed(() => summaries.value[todayIndex.value])
const weekSummary = computed(() => summaries.value[7])

const chartDays = computed(() =>
  days.value.map((d) => {
    const day = byDate(toDateInput(d.from))
    return { label: format.weekday(d.from), hours: (day?.netMs ?? 0) / HOUR, target: (day?.targetMs ?? 0) / HOUR }
  }),
)

const byProject = computed(() => {
  const list = weekSummary.value?.byProject ?? []
  const max = Math.max(1, ...list.map((p) => p.ms))
  return list.map((p) => ({
    project: p.projectId ? projects.byId.get(p.projectId) : null,
    ms: p.ms,
    pct: (p.ms / max) * 100,
  }))
})
</script>

<template>
  <div class="mb-4 flex justify-end">
    <Button v-if="!customizing" size="sm" variant="ghost" @click="customizing = true">
      <Settings2 /> {{ $t('dashboard.customize') }}
    </Button>
    <Button v-else size="sm" @click="customizing = false"><Check /> {{ $t('dashboard.done') }}</Button>
  </div>

  <div class="grid gap-4 lg:grid-cols-2">
    <section
      v-for="(w, i) in visible"
      :key="w"
      class="relative min-w-0"
      :class="[w === 'stats' ? 'lg:col-span-2' : '', customizing && layout.hidden.includes(w) ? 'opacity-40' : '']"
    >
      <div
        v-if="customizing"
        class="absolute -top-3 right-3 z-10 flex items-center gap-0.5 rounded-md border bg-background p-0.5 shadow-sm"
      >
        <span class="px-2 text-xs text-muted-foreground">{{ $t(`dashboard.widgets.${w}`) }}</span>
        <Button size="icon" variant="ghost" class="size-7" :title="$t('dashboard.moveUp')" :disabled="i === 0" @click="move(w, -1)"><ArrowUp /></Button>
        <Button size="icon" variant="ghost" class="size-7" :title="$t('dashboard.moveDown')" :disabled="i === visible.length - 1" @click="move(w, 1)"><ArrowDown /></Button>
        <Button size="icon" variant="ghost" class="size-7" :title="layout.hidden.includes(w) ? $t('dashboard.show') : $t('dashboard.hide')" @click="toggleHidden(w)">
          <Eye v-if="layout.hidden.includes(w)" /><EyeOff v-else />
        </Button>
      </div>

      <template v-if="w === 'stats'">
        <div class="grid gap-4 md:grid-cols-1 xl:grid-cols-3">
          <Card>
            <CardHeader>
              <CardDescription>{{ $t('dashboard.today') }} · {{ format.shortDate(today) }}</CardDescription>
              <CardTitle class="text-3xl tabular-nums">{{ settings.formatDuration(todayWork?.netMs ?? todaySummary?.totalMs ?? 0) }} h</CardTitle>
            </CardHeader>
            <CardContent class="text-sm text-muted-foreground">
              <template v-if="todayWork && todayWork.targetMs > 0">
                {{ $t('dashboard.ofTarget', { target: settings.formatDuration(todayWork.targetMs) }) }} ·
              </template>
              {{ $t('dashboard.entries', todaySummary?.entryCount ?? 0) }}
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardDescription>{{ $t('work.balance') }}</CardDescription>
              <CardTitle
                class="text-3xl tabular-nums"
                :class="balanceMs < 0 ? 'text-amber-600 dark:text-amber-400' : ''"
              >{{ settings.formatSigned(balanceMs) }} h</CardTitle>
            </CardHeader>
            <CardContent class="text-sm text-muted-foreground">
              <template v-if="work.balance?.since">{{ $t('work.balanceSince', { date: format.longDate(parseDate(work.balance.since)) }) }}</template>
              <template v-else>–</template>
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardDescription>{{ $t('dashboard.thisWeek') }}</CardDescription>
              <CardTitle class="text-3xl tabular-nums">{{ settings.formatDuration(weekSummary?.totalMs ?? 0) }} h</CardTitle>
            </CardHeader>
            <CardContent class="text-sm text-muted-foreground">
              {{ format.shortDate(week.from) }} – {{ format.shortDate(days[6].from) }}
            </CardContent>
          </Card>
        </div>
      </template>

      <template v-else-if="w === 'perDay'">
        <Card class="h-full">
          <CardHeader>
            <CardTitle>{{ $t('dashboard.perDay') }}</CardTitle>
          </CardHeader>
          <CardContent>
            <WeekChart :days="chartDays" />
          </CardContent>
        </Card>
      </template>

      <template v-else-if="w === 'projects'">
        <Card class="h-full">
          <CardHeader>
            <CardTitle>{{ $t('dashboard.byProject') }}</CardTitle>
          </CardHeader>
          <CardContent>
            <EmptyState v-if="byProject.length === 0" :icon="Clock" :text="$t('dashboard.noData')" />
            <ul v-else class="grid gap-3">
              <li v-for="row in byProject" :key="row.project?.id ?? 'none'" class="grid gap-1">
                <div class="flex items-center justify-between gap-2 text-sm">
                  <ProjectDot :project="row.project" />
                  <span class="tabular-nums text-muted-foreground">{{ settings.formatDuration(row.ms) }} h</span>
                </div>
                <div class="h-1.5 overflow-hidden rounded-full bg-muted">
                  <div
                    class="h-full rounded-full"
                    :style="{ width: `${row.pct}%`, backgroundColor: row.project?.color ?? 'var(--muted-foreground)' }"
                  />
                </div>
              </li>
            </ul>
          </CardContent>
        </Card>
      </template>

      <Card v-else-if="w === 'budgets'" class="h-full">
        <CardHeader>
          <CardTitle>{{ $t('dashboard.budgets') }}</CardTitle>
        </CardHeader>
        <CardContent>
          <EmptyState v-if="budgetRows.length === 0" :icon="Target" :text="$t('dashboard.noBudgets')" />
          <ul v-else class="grid gap-3">
            <li v-for="b in budgetRows" :key="b.projectId" class="grid gap-1">
              <div class="flex items-center justify-between gap-2 text-sm">
                <ProjectDot :project="b.project" />
                <span class="shrink-0 tabular-nums text-muted-foreground">
                  {{ settings.formatDuration(b.usedMs) }} / {{ settings.formatDuration(b.budgetMs) }} h
                  · {{ $t(`projects.periods.${b.period}`) }}
                </span>
              </div>
              <div class="h-1.5 overflow-hidden rounded-full bg-muted">
                <div
                  class="h-full rounded-full"
                  :class="b.pct >= 100 ? 'bg-red-500' : b.pct >= b.warnPct ? 'bg-amber-500' : 'bg-primary/70'"
                  :style="{ width: `${Math.min(100, b.pct)}%` }"
                />
              </div>
            </li>
          </ul>
        </CardContent>
      </Card>

      <Card v-else-if="w === 'costCenters'" class="h-full">
        <CardHeader>
          <CardTitle>{{ $t('dashboard.costCentersMonth') }}</CardTitle>
        </CardHeader>
        <CardContent>
          <EmptyState v-if="costCenters.all.length === 0" :icon="Landmark" :text="$t('dashboard.noCostCenters')"></EmptyState>
          <EmptyState v-else-if="costCenterRows.length === 0" :icon="Clock" :text="$t('dashboard.noData')" />
          <ul v-else class="grid gap-3">
            <li v-for="row in costCenterRows" :key="row.key ?? 'none'" class="grid gap-1">
              <div class="flex items-center justify-between gap-2 text-sm">
                <span class="truncate" :class="row.label ? '' : 'text-muted-foreground'">{{ row.label ?? $t('reports.noCostCenter') }}</span>
                <span class="shrink-0 tabular-nums text-muted-foreground">
                  {{ settings.formatDuration(row.ms) }} h · {{ Math.round(row.pct) }} %
                </span>
              </div>
              <div class="h-1.5 overflow-hidden rounded-full bg-muted">
                <div class="h-full rounded-full bg-primary/70" :style="{ width: `${row.pct}%` }" />
              </div>
            </li>
          </ul>
        </CardContent>
      </Card>
    </section>
  </div>
</template>
