<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { toast } from 'vue-sonner'
import { ArrowLeft, Printer } from '@lucide/vue'
import { commands, type Report, type WorkDay } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useFormat } from '@/composables/useFormat'
import { useWorkDays } from '@/composables/useWorkDays'
import { call, errorMessage } from '@/lib/ipc'
import { addMonths, parseDate, toDateInput, weekday } from '@/lib/time'
import { useProjectsStore } from '@/stores/projects'
import { useSettingsStore } from '@/stores/settings'
import { useWorkStore } from '@/stores/work'

// Printable monthly timesheet (SPEC §5.5): print or save as PDF, then sign.
definePage({ meta: { layout: 'print' } })

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const format = useFormat()
const settings = useSettingsStore()
const projects = useProjectsStore()
const work = useWorkStore()

const month = ref(typeof route.query.month === 'string' ? route.query.month : toDateInput(Date.now()).slice(0, 7))
watch(month, (m) => void router.replace({ query: { month: m } }))

const range = computed(() => {
  // A cleared month input falls back to the current month.
  const parsed = parseDate(`${month.value}-01`)
  const from = Number.isFinite(parsed) ? parsed : parseDate(`${toDateInput(Date.now()).slice(0, 7)}-01`)
  return { from, to: addMonths(from, 1) }
})
const { days, holidayName } = useWorkDays(range)

const title = computed(() =>
  format.monthYear(range.value.from),
)

const totals = computed(() =>
  days.value.reduce(
    (sum, d) => ({
      net: sum.net + d.netMs,
      target: sum.target + d.targetMs,
      delta: sum.delta + d.deltaMs,
      breaks: sum.breaks + d.breakTakenMs + d.breakDeductedMs,
    }),
    { net: 0, target: 0, delta: 0, breaks: 0 },
  ),
)

const report = ref<Report | null>(null)
watch(
  range,
  async (r) => {
    try {
      report.value = await call(commands.report({ range: r, filter: { projectIds: [], tagIds: [], costCenterIds: [], billable: null }, groupBy: ['project'] }))
    } catch (err) {
      toast.error(errorMessage(err))
    }
  },
  { immediate: true },
)

const h = (ms: number) => (ms ? settings.formatDuration(ms) : '')
const isWeekend = (d: WorkDay) => [0, 6].includes(weekday(parseDate(d.date)))

function remark(d: WorkDay) {
  const parts: string[] = []
  const holiday = d.holiday ?? holidayName(d.date)
  if (holiday) parts.push(holiday)
  for (const id of d.absenceTypeIds) {
    const type = work.absenceTypesById.get(id)
    if (type) parts.push(work.absenceTypeName(type))
  }
  if (d.warnings.length) parts.push(t('reports.sheet.warning'))
  return parts.join(', ')
}

async function print() {
  try {
    await call(commands.windowPrint())
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
</script>

<template>
  <div class="mx-auto flex max-w-[210mm] flex-wrap items-center gap-2 px-4 py-4 print:hidden">
    <Button variant="ghost" @click="router.back()"><ArrowLeft /> {{ $t('reports.sheet.back') }}</Button>
    <Input v-model="month" type="month" class="w-44" :aria-label="$t('reports.groups.month')" />
    <Button class="ml-auto" @click="print"><Printer /> {{ $t('reports.sheet.print') }}</Button>
  </div>

  <!-- Paper: always light, regardless of the app theme. -->
  <article
    class="sheet mx-auto mb-10 max-w-[210mm] bg-white p-[14mm] text-[10.5px] leading-snug text-neutral-900 shadow-sm print:m-0 print:max-w-none print:p-0 print:shadow-none"
  >
    <header class="mb-5 flex items-end justify-between gap-4 border-b border-neutral-300 pb-3">
      <div>
        <h1 class="text-lg font-semibold">{{ $t('reports.sheet.title') }}</h1>
        <p class="text-sm text-neutral-600">{{ title }}</p>
      </div>
      <div class="grid gap-1 text-right">
        <span>{{ $t('reports.sheet.name') }}: <span class="inline-block w-48 border-b border-neutral-400" /></span>
        <span v-if="work.summary" class="text-neutral-600">
          {{ work.summary.name }}<template v-if="work.summary.regionName"> · {{ work.summary.regionName }}</template>
        </span>
      </div>
    </header>

    <table class="w-full border-collapse tabular-nums">
      <thead>
        <tr class="border-b border-neutral-400 text-left text-[9.5px] uppercase tracking-wide text-neutral-600">
          <th class="py-1 pr-2 font-medium">{{ $t('entry.date') }}</th>
          <th class="py-1 pr-2 font-medium">{{ $t('entry.start') }}</th>
          <th class="py-1 pr-2 font-medium">{{ $t('entry.end') }}</th>
          <th class="py-1 pr-2 text-right font-medium">{{ $t('reports.sheet.break') }}</th>
          <th class="py-1 pr-2 text-right font-medium">{{ $t('reports.sheet.worked') }}</th>
          <th class="py-1 pr-2 text-right font-medium">{{ $t('dashboard.target') }}</th>
          <th class="py-1 pr-2 text-right font-medium">{{ $t('reports.sheet.difference') }}</th>
          <th class="py-1 font-medium">{{ $t('reports.sheet.remark') }}</th>
        </tr>
      </thead>
      <tbody>
        <tr
          v-for="d in days"
          :key="d.date"
          class="border-b border-neutral-200"
          :class="isWeekend(d) || d.holiday ? 'bg-neutral-50 text-neutral-500' : ''"
        >
          <td class="py-[3px] pr-2 whitespace-nowrap">
            {{ format.weekday(parseDate(d.date)) }} {{ format.shortDate(parseDate(d.date)) }}
          </td>
          <td class="py-[3px] pr-2">{{ d.firstStart !== null ? format.time(d.firstStart) : '' }}</td>
          <td class="py-[3px] pr-2">{{ d.lastEnd !== null ? format.time(d.lastEnd) : '' }}</td>
          <td class="py-[3px] pr-2 text-right">{{ h(d.breakTakenMs + d.breakDeductedMs) }}</td>
          <td class="py-[3px] pr-2 text-right font-medium">{{ h(d.netMs) }}</td>
          <td class="py-[3px] pr-2 text-right">{{ h(d.targetMs) }}</td>
          <td class="py-[3px] pr-2 text-right">{{ d.deltaMs ? settings.formatSigned(d.deltaMs) : '' }}</td>
          <td class="py-[3px]">{{ remark(d) }}</td>
        </tr>
      </tbody>
      <tfoot>
        <tr class="border-t-2 border-neutral-400 font-semibold">
          <td class="py-1.5 pr-2" colspan="3">{{ $t('common.total') }}</td>
          <td class="py-1.5 pr-2 text-right">{{ h(totals.breaks) }}</td>
          <td class="py-1.5 pr-2 text-right">{{ settings.formatDuration(totals.net) }}</td>
          <td class="py-1.5 pr-2 text-right">{{ settings.formatDuration(totals.target) }}</td>
          <td class="py-1.5 pr-2 text-right">{{ settings.formatSigned(totals.delta) }}</td>
          <td />
        </tr>
      </tfoot>
    </table>

    <section v-if="report && report.rows.length" class="mt-6 break-inside-avoid">
      <h2 class="mb-1 font-semibold">{{ $t('reports.sheet.projects') }}</h2>
      <table class="w-full max-w-md border-collapse tabular-nums">
        <tbody>
          <tr v-for="row in report.rows" :key="row.key ?? 'none'" class="border-b border-neutral-200">
            <td class="py-[3px] pr-2">
              {{ row.key ? (projects.byId.get(row.key)?.name ?? '') : $t('common.noProject') }}
              <span v-if="row.key && projects.byId.get(row.key)?.alias" class="text-neutral-500">
                ({{ projects.byId.get(row.key)?.alias }})
              </span>
            </td>
            <td class="py-[3px] text-right">{{ settings.formatDuration(row.ms) }} h</td>
          </tr>
        </tbody>
      </table>
    </section>

    <footer class="mt-14 grid grid-cols-2 gap-12 break-inside-avoid">
      <div class="border-t border-neutral-500 pt-1 text-neutral-600">{{ $t('reports.sheet.signEmployee') }}</div>
      <div class="border-t border-neutral-500 pt-1 text-neutral-600">{{ $t('reports.sheet.signSupervisor') }}</div>
    </footer>
  </article>
</template>

<style>
@media print {
  @page {
    size: A4;
    margin: 12mm;
  }
  html,
  body {
    background: white !important;
  }
}
</style>
