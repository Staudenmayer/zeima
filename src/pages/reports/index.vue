<script setup lang="ts">
import { computed, onScopeDispose, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useIntervalFn, useStorage } from '@vueuse/core'
import { toast } from 'vue-sonner'
import { BarChart3, ChevronRight, Download, FileText, Puzzle } from '@lucide/vue'
import { commands, events, type AddonExportFormat, type ExportFormat, type Report, type ReportGroup, type ReportQuery, type ReportRow } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import DateInput from '@/components/time/DateInput.vue'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import EmptyState from '@/components/time/EmptyState.vue'
import MultiFilter, { type FilterOption } from '@/components/time/MultiFilter.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import ActivityLabel from '@/components/time/ActivityLabel.vue'
import ProjectDot from '@/components/time/ProjectDot.vue'
import TagList from '@/components/time/TagList.vue'
import { useFormat } from '@/composables/useFormat'
import { useReportLabels } from '@/composables/useReportLabels'
import { call, errorMessage } from '@/lib/ipc'
import { addDays, parseDate, presetRange, toDateInput, type PeriodPreset } from '@/lib/time'
import { costCenterLabel, useCostCentersStore } from '@/stores/costCenters'
import { useActivitiesStore } from '@/stores/activities'
import { useProjectsStore } from '@/stores/projects'
import { useSettingsStore } from '@/stores/settings'
import { useTagsStore } from '@/stores/tags'
import { useTimerStore } from '@/stores/timer'

definePage({ meta: { titleKey: 'nav.reports' } })

const { t } = useI18n()
const format = useFormat()
const settings = useSettingsStore()
const activities = useActivitiesStore()
const projects = useProjectsStore()
const tags = useTagsStore()
const costCenters = useCostCentersStore()
const timer = useTimerStore()
const labels = useReportLabels()

const presets: PeriodPreset[] = ['thisWeek', 'lastWeek', 'thisMonth', 'lastMonth', 'thisYear', 'lastYear']
const groups: ReportGroup[] = ['activity', 'project', 'tag', 'costCenter', 'billable', 'day', 'week', 'month']
const NONE = 'none'

// Remembered between visits (per device).
const state = useStorage('reports.state', {
  period: 'thisMonth' as PeriodPreset | 'custom',
  customFrom: toDateInput(Date.now()),
  customTo: toDateInput(Date.now()),
  activityIds: [] as (string | null)[],
  projectIds: [] as (string | null)[],
  tagIds: [] as (string | null)[],
  costCenterIds: [] as (string | null)[],
  billable: 'all' as 'all' | 'yes' | 'no',
  group1: 'project' as ReportGroup,
  group2: NONE as ReportGroup | typeof NONE,
  view: 'summary' as 'summary' | 'entries',
}, undefined, { mergeDefaults: true })

const range = computed(() => {
  if (state.value.period !== 'custom') return presetRange(state.value.period, Date.now())
  // Half-typed date inputs fall back to today.
  const today = parseDate(toDateInput(Date.now()))
  const parse = (d: string) => (Number.isFinite(parseDate(d)) ? parseDate(d) : today)
  const from = parse(state.value.customFrom)
  const to = addDays(parse(state.value.customTo), 1)
  return { from, to: Math.max(from, to) }
})

const groupBy = computed<ReportGroup[]>(() =>
  state.value.group2 === NONE || state.value.group2 === state.value.group1
    ? [state.value.group1]
    : [state.value.group1, state.value.group2],
)

const query = computed<ReportQuery>(() => ({
  range: range.value,
  filter: {
    activityIds: state.value.activityIds,
    projectIds: state.value.projectIds,
    tagIds: state.value.tagIds.filter((id): id is string => id !== null),
    costCenterIds: state.value.costCenterIds,
    billable: state.value.billable === 'all' ? null : state.value.billable === 'yes',
  },
  groupBy: groupBy.value,
}))

const report = ref<Report | null>(null)
/** Grouping of the loaded report; the selects may already show the next one. */
const shownGroups = ref<ReportGroup[]>([])
async function reload() {
  const q = query.value
  try {
    const result = await call(commands.report(q))
    // A newer query may have finished first.
    if (q !== query.value) return
    report.value = result
    shownGroups.value = q.groupBy ?? []
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
watch(query, reload, { immediate: true, deep: true })
useIntervalFn(() => timer.running && void reload(), 60_000)
const unlisten = events.dataChanged.listen((e) => {
  if (e.payload.kinds.some((k) => k === 'entry' || k === 'activity' || k === 'project' || k === 'tag' || k === 'costCenter')) void reload()
})
onScopeDispose(() => void unlisten.then((fn) => fn()))

// Filter options
const activityOptions = computed<FilterOption[]>(() => [
  ...activities.all.map((a) => ({ value: a.id, label: a.name, color: a.color })),
  { value: null, label: t('activities.none') },
])
const projectOptions = computed<FilterOption[]>(() => [
  ...projects.all.map((p) => ({ value: p.id, label: p.alias ? `${p.name} (${p.alias})` : p.name, color: p.color })),
  { value: null, label: t('common.noProject') },
])
const tagOptions = computed<FilterOption[]>(() => tags.all.map((tag) => ({ value: tag.id, label: tag.name, color: tag.color })))
const costCenterOptions = computed<FilterOption[]>(() => [
  ...costCenters.all.map((c) => ({ value: c.id, label: costCenterLabel(c) })),
  { value: null, label: t('reports.noCostCenter') },
])
const hasFilter = computed(
  () =>
    state.value.activityIds.length + state.value.projectIds.length + state.value.tagIds.length + state.value.costCenterIds.length > 0 ||
    state.value.billable !== 'all',
)
function clearFilters() {
  state.value.activityIds = []
  state.value.projectIds = []
  state.value.tagIds = []
  state.value.costCenterIds = []
  state.value.billable = 'all'
}

// Result tree
const expanded = ref(new Set<string>())
const rowId = (row: ReportRow) => row.key ?? '__none__'
function toggle(row: ReportRow) {
  const id = rowId(row)
  const next = new Set(expanded.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  expanded.value = next
}
const maxMs = computed(() => Math.max(1, ...(report.value?.rows ?? []).map((r) => r.ms)))
const hours = (ms: number) => `${settings.formatDuration(ms)} h`

const periodLabel = computed(() => {
  const { from, to } = range.value
  return `${format.shortDate(from)} – ${format.longDate(addDays(to, -1))}`
})

// Export
const exporting = ref(false)
async function exportAs(fmt: ExportFormat) {
  exporting.value = true
  try {
    const stem = `${t('nav.reports').toLowerCase()}-${toDateInput(range.value.from)}-${toDateInput(addDays(range.value.to, -1))}`
    const path = await call(commands.reportExport(query.value, fmt, stem))
    if (path) toast.success(t('reports.exported', { path }))
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    exporting.value = false
  }
}

// Formats contributed by enabled addons (report.export hook).
const addonFormats = ref<AddonExportFormat[]>([])
void call(commands.addonExportFormats()).then((f) => (addonFormats.value = f)).catch(() => {})

async function exportWithAddon(f: AddonExportFormat) {
  exporting.value = true
  try {
    const stem = `${t('nav.reports').toLowerCase()}-${toDateInput(range.value.from)}-${toDateInput(addDays(range.value.to, -1))}`
    const path = await call(commands.reportExportAddon(query.value, f.addonId, f.formatId, stem))
    if (path) toast.success(t('reports.exported', { path }))
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    exporting.value = false
  }
}

/** Month for the printable timesheet: the selected period's first month. */
const timesheetMonth = computed(() => toDateInput(range.value.from).slice(0, 7))

function entryDuration(e: { startAt: number; endAt: number | null }) {
  return (e.endAt ?? timer.now) - e.startAt
}
</script>

<template>
  <PageHeader :title="$t('nav.reports')">
    <DropdownMenu>
      <DropdownMenuTrigger as-child>
        <Button variant="outline" :disabled="exporting"><Download /> {{ $t('reports.export') }}</Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem @select="exportAs('xlsx')">{{ $t('reports.exportXlsx') }}</DropdownMenuItem>
        <DropdownMenuItem @select="exportAs('csvEntries')">{{ $t('reports.exportCsvEntries') }}</DropdownMenuItem>
        <DropdownMenuItem @select="exportAs('csvSummary')">{{ $t('reports.exportCsvSummary') }}</DropdownMenuItem>
        <template v-if="addonFormats.length">
          <DropdownMenuSeparator />
          <DropdownMenuItem v-for="f in addonFormats" :key="`${f.addonId}/${f.formatId}`" @select="exportWithAddon(f)">
            <Puzzle /> {{ f.name }}
          </DropdownMenuItem>
        </template>
        <DropdownMenuSeparator />
        <DropdownMenuItem as-child>
          <RouterLink :to="{ path: '/reports/timesheet', query: { month: timesheetMonth } }">
            <FileText /> {{ $t('reports.timesheetPdf') }}
          </RouterLink>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  </PageHeader>

  <!-- Period and filters -->
  <div class="mb-4 flex flex-wrap items-center gap-2">
    <Select v-model="state.period">
      <SelectTrigger size="sm" class="w-40"><SelectValue /></SelectTrigger>
      <SelectContent>
        <SelectItem v-for="p in presets" :key="p" :value="p">{{ $t(`reports.periods.${p}`) }}</SelectItem>
        <SelectItem value="custom">{{ $t('reports.periods.custom') }}</SelectItem>
      </SelectContent>
    </Select>
    <template v-if="state.period === 'custom'">
      <DateInput v-model="state.customFrom" class="w-40 [&_input]:h-8" :aria-label="$t('reports.from')" />
      <span class="text-muted-foreground">–</span>
      <DateInput v-model="state.customTo" class="w-40 [&_input]:h-8" :aria-label="$t('reports.to')" />
    </template>
    <span v-else class="text-sm text-muted-foreground">{{ periodLabel }}</span>

    <span class="mx-1 h-5 w-px bg-border" />
    <MultiFilter v-model="state.activityIds" :label="$t('entry.activity')" :options="activityOptions" />
    <MultiFilter v-model="state.projectIds" :label="$t('entry.project')" :options="projectOptions" />
    <MultiFilter v-if="tagOptions.length" v-model="state.tagIds" :label="$t('entry.tags')" :options="tagOptions" />
    <MultiFilter
      v-if="costCenters.all.length"
      v-model="state.costCenterIds"
      :label="$t('costCenters.costCenter')"
      :options="costCenterOptions"
    />
    <Select v-model="state.billable">
      <SelectTrigger size="sm" class="w-40"><SelectValue /></SelectTrigger>
      <SelectContent>
        <SelectItem value="all">{{ $t('reports.billableAll') }}</SelectItem>
        <SelectItem value="yes">{{ $t('reports.billable') }}</SelectItem>
        <SelectItem value="no">{{ $t('reports.notBillable') }}</SelectItem>
      </SelectContent>
    </Select>
    <Button v-if="hasFilter" size="sm" variant="ghost" @click="clearFilters">{{ $t('reports.clearFilter') }}</Button>
  </div>

  <!-- Totals -->
  <div class="mb-6 grid gap-4 sm:grid-cols-3">
    <Card class="py-4">
      <CardHeader class="px-4">
        <CardDescription>{{ $t('reports.total') }}</CardDescription>
        <CardTitle class="text-2xl tabular-nums">{{ hours(report?.totalMs ?? 0) }}</CardTitle>
      </CardHeader>
    </Card>
    <Card class="py-4">
      <CardHeader class="px-4">
        <CardDescription>{{ $t('reports.billable') }}</CardDescription>
        <CardTitle class="text-2xl tabular-nums">
          {{ hours(report?.billableMs ?? 0) }}
          <span v-if="report && report.totalMs > 0" class="text-sm font-normal text-muted-foreground">
            · {{ Math.round((report.billableMs / report.totalMs) * 100) }} %
          </span>
        </CardTitle>
      </CardHeader>
    </Card>
    <Card class="py-4">
      <CardHeader class="px-4">
        <CardDescription>{{ $t('reports.entries') }}</CardDescription>
        <CardTitle class="text-2xl tabular-nums">{{ report?.entryCount ?? 0 }}</CardTitle>
      </CardHeader>
    </Card>
  </div>

  <!-- View and grouping -->
  <div class="mb-3 flex flex-wrap items-center gap-2">
    <Tabs v-model="state.view">
      <TabsList>
        <TabsTrigger value="summary">{{ $t('reports.summary') }}</TabsTrigger>
        <TabsTrigger value="entries">{{ $t('reports.entries') }}</TabsTrigger>
      </TabsList>
    </Tabs>
    <template v-if="state.view === 'summary'">
      <span class="ml-auto text-sm text-muted-foreground">{{ $t('reports.groupBy') }}</span>
      <Select v-model="state.group1">
        <SelectTrigger size="sm" class="w-36"><SelectValue /></SelectTrigger>
        <SelectContent>
          <SelectItem v-for="g in groups" :key="g" :value="g">{{ $t(`reports.groups.${g}`) }}</SelectItem>
        </SelectContent>
      </Select>
      <span class="text-sm text-muted-foreground">{{ $t('reports.then') }}</span>
      <Select v-model="state.group2">
        <SelectTrigger size="sm" class="w-36"><SelectValue /></SelectTrigger>
        <SelectContent>
          <SelectItem :value="NONE">{{ $t('common.none') }}</SelectItem>
          <SelectItem v-for="g in groups.filter((g) => g !== state.group1)" :key="g" :value="g">
            {{ $t(`reports.groups.${g}`) }}
          </SelectItem>
        </SelectContent>
      </Select>
    </template>
  </div>

  <EmptyState v-if="report && report.entryCount === 0" :icon="BarChart3" :text="$t('reports.empty')" />

  <!-- Summary -->
  <div v-else-if="report && state.view === 'summary'" class="overflow-x-auto rounded-lg border">
    <table class="w-full text-sm">
      <thead class="text-left text-xs text-muted-foreground">
        <tr class="border-b">
          <th class="px-3 py-2 font-medium">{{ $t(`reports.groups.${shownGroups[0]}`) }}</th>
          <th class="hidden w-1/4 px-3 py-2 sm:table-cell"><span class="sr-only">{{ $t('reports.share') }}</span></th>
          <th class="w-24 px-3 py-2 text-right font-medium">{{ $t('reports.hours') }}</th>
          <th class="w-28 px-3 py-2 text-right font-medium">{{ $t('reports.billable') }}</th>
          <th class="w-20 px-3 py-2 text-right font-medium">{{ $t('reports.entries') }}</th>
        </tr>
      </thead>
      <tbody>
        <template v-for="row in report.rows" :key="rowId(row)">
          <tr
            class="border-b last:border-b-0 hover:bg-muted/30"
            :class="row.children.length ? 'cursor-pointer' : ''"
            @click="row.children.length && toggle(row)"
          >
            <td class="px-3 py-2">
              <div class="flex min-w-0 items-center gap-2">
                <ChevronRight
                  v-if="row.children.length"
                  class="size-4 shrink-0 text-muted-foreground transition-transform"
                  :class="expanded.has(rowId(row)) ? 'rotate-90' : ''"
                />
                <ProjectDot v-if="shownGroups[0] === 'project'" :project="row.key ? projects.byId.get(row.key) : null" show-alias />
                <template v-else>
                  <span
                    v-if="labels.color(shownGroups[0], row.key)"
                    class="size-2.5 shrink-0 rounded-full"
                    :style="{ backgroundColor: labels.color(shownGroups[0], row.key) }"
                  />
                  <span class="truncate" :class="row.key === null ? 'text-muted-foreground' : ''">
                    {{ labels.label(shownGroups[0], row.key) }}
                  </span>
                </template>
              </div>
            </td>
            <td class="hidden px-3 py-2 sm:table-cell">
              <div class="h-1.5 overflow-hidden rounded-full bg-muted">
                <div
                  class="h-full rounded-full bg-primary/70"
                  :style="{
                    width: `${(row.ms / maxMs) * 100}%`,
                    backgroundColor: labels.color(shownGroups[0], row.key),
                  }"
                />
              </div>
            </td>
            <td class="px-3 py-2 text-right font-medium tabular-nums">{{ hours(row.ms) }}</td>
            <td class="px-3 py-2 text-right tabular-nums text-muted-foreground">{{ hours(row.billableMs) }}</td>
            <td class="px-3 py-2 text-right tabular-nums text-muted-foreground">{{ row.entryCount }}</td>
          </tr>
          <template v-if="expanded.has(rowId(row)) && shownGroups[1]">
            <tr v-for="child in row.children" :key="`${rowId(row)}/${rowId(child)}`" class="border-b bg-muted/20 text-[13px]">
              <td class="py-1.5 pr-3 pl-10">
                <span class="flex min-w-0 items-center gap-2">
                  <span
                    v-if="labels.color(shownGroups[1], child.key)"
                    class="size-2 shrink-0 rounded-full"
                    :style="{ backgroundColor: labels.color(shownGroups[1], child.key) }"
                  />
                  <span class="truncate" :class="child.key === null ? 'text-muted-foreground' : ''">
                    {{ labels.label(shownGroups[1], child.key) }}
                  </span>
                </span>
              </td>
              <td class="hidden px-3 py-1.5 sm:table-cell" />
              <td class="px-3 py-1.5 text-right tabular-nums">{{ hours(child.ms) }}</td>
              <td class="px-3 py-1.5 text-right tabular-nums text-muted-foreground">{{ hours(child.billableMs) }}</td>
              <td class="px-3 py-1.5 text-right tabular-nums text-muted-foreground">{{ child.entryCount }}</td>
            </tr>
          </template>
        </template>
      </tbody>
      <tfoot>
        <tr class="border-t bg-muted/40 font-medium">
          <td class="px-3 py-2">{{ $t('common.total') }}</td>
          <td class="hidden sm:table-cell" />
          <td class="px-3 py-2 text-right tabular-nums">{{ hours(report.totalMs) }}</td>
          <td class="px-3 py-2 text-right tabular-nums">{{ hours(report.billableMs) }}</td>
          <td class="px-3 py-2 text-right tabular-nums">{{ report.entryCount }}</td>
        </tr>
      </tfoot>
    </table>
    <p v-if="shownGroups.includes('tag')" class="border-t px-3 py-2 text-xs text-muted-foreground">{{ $t('reports.tagNote') }}</p>
  </div>

  <!-- Entries -->
  <div v-else-if="report" class="overflow-x-auto rounded-lg border">
    <table class="w-full text-sm">
      <thead class="text-left text-xs text-muted-foreground">
        <tr class="border-b">
          <th class="px-3 py-2 font-medium">{{ $t('entry.date') }}</th>
          <th class="px-3 py-2 font-medium">{{ $t('timesheet.time') }}</th>
          <th class="px-3 py-2 font-medium">{{ $t('entry.activity') }}</th>
          <th class="px-3 py-2 font-medium">{{ $t('entry.project') }}</th>
          <th class="px-3 py-2 font-medium">{{ $t('entry.tags') }}</th>
          <th v-if="costCenters.all.length" class="px-3 py-2 font-medium">{{ $t('costCenters.costCenter') }}</th>
          <th class="px-3 py-2 font-medium">{{ $t('entry.note') }}</th>
          <th class="px-3 py-2 text-right font-medium">{{ $t('timesheet.duration') }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="e in report.entries" :key="e.id" class="border-b last:border-b-0 hover:bg-muted/30">
          <td class="whitespace-nowrap px-3 py-1.5 tabular-nums">{{ format.shortDate(e.startAt) }}</td>
          <td class="whitespace-nowrap px-3 py-1.5 tabular-nums text-muted-foreground">
            {{ format.time(e.startAt) }}–{{ e.endAt === null ? '' : format.time(e.endAt) }}
          </td>
          <td class="max-w-44 px-3 py-1.5"><ActivityLabel :activity="e.activityId ? activities.byId.get(e.activityId) : null" /></td>
          <td class="max-w-56 px-3 py-1.5"><ProjectDot :project="e.projectId ? projects.byId.get(e.projectId) : null" /></td>
          <td class="px-3 py-1.5"><TagList :ids="e.tagIds" /></td>
          <td v-if="costCenters.all.length" class="whitespace-nowrap px-3 py-1.5 font-mono text-xs text-muted-foreground">
            {{ e.costCenterId ? costCenters.byId.get(e.costCenterId)?.code : '' }}
          </td>
          <td class="max-w-0 w-full truncate px-3 py-1.5 text-muted-foreground" :title="e.note">{{ e.note }}</td>
          <td class="whitespace-nowrap px-3 py-1.5 text-right tabular-nums">
            <span v-if="e.billable" class="mr-1 text-xs text-muted-foreground" :title="$t('entry.billable')">€</span>
            {{ settings.formatDuration(entryDuration(e)) }}
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
