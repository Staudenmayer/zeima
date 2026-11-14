<script setup lang="ts">
import UserAvatar from '@/components/time/UserAvatar.vue'
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { ChevronLeft, ChevronRight, Palmtree, Pencil, Plus, Trash2 } from '@lucide/vue'
import { commands, type Absence, type AbsenceType, type HolidayView, type VacationSummary } from '@/bindings'
import { Badge } from '@/components/ui/badge'
import MultiFilter, { type FilterOption } from '@/components/time/MultiFilter.vue'
import TableToolbar from '@/components/time/TableToolbar.vue'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import DateInput from '@/components/time/DateInput.vue'
import { Progress } from '@/components/ui/progress'
import AbsenceDialog from '@/components/time/AbsenceDialog.vue'
import AbsenceTypeDialog from '@/components/time/AbsenceTypeDialog.vue'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import EmptyState from '@/components/time/EmptyState.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import { useFormat } from '@/composables/useFormat'
import { call, errorMessage } from '@/lib/ipc'
import { parseDate, toDateInput } from '@/lib/time'
import { useAccountStore } from '@/stores/account'
import { useOrgStore } from '@/stores/org'
import { useWorkStore } from '@/stores/work'
import { useWorkspaceStore } from '@/stores/workspace'
import SortHead from '@/components/time/SortHead.vue'
import { useSort } from '@/composables/useSort'

definePage({ meta: { titleKey: 'nav.absences' } })

const { t, locale } = useI18n()
const work = useWorkStore()
const workspaces = useWorkspaceStore()
const account = useAccountStore()
const org = useOrgStore()
const format = useFormat()

const year = ref(Number(toDateInput(Date.now()).slice(0, 4)))
const absences = ref<Absence[]>([])
/** Approved absences of colleagues (what the person may see of them). */
const others = ref<Absence[]>([])
const holidays = ref<HolidayView[]>([])
const vacation = ref<VacationSummary | null>(null)

async function run<T>(promise: Promise<T>): Promise<T | undefined> {
  try {
    return await promise
  } catch (err) {
    toast.error(errorMessage(err))
    return undefined
  }
}

async function load() {
  const y = year.value
  const [a, o, h, v] = await Promise.all([
    run(call(commands.absenceList(`${y}-01-01`, `${y}-12-31`))),
    workspaces.inOrg ? run(call(commands.absenceListOthers(`${y}-01-01`, `${y}-12-31`))) : Promise.resolve([]),
    run(call(commands.holidayList(y, locale.value))),
    run(call(commands.vacationSummary(y))),
  ])
  absences.value = a ?? []
  others.value = o ?? []
  holidays.value = h ?? []
  vacation.value = v ?? null
}
// Reload when the year, the language or any work data changes.
watch([year, locale, () => workspaces.active?.id, () => work.schedules, () => work.absenceTypes, () => work.balance], load, { immediate: true })

const days = (n: number | null | undefined) => format.number(n ?? 0, 1)
const usedPct = computed(() => {
  const v = vacation.value
  if (!v || !v.entitlementDays) return 0
  return Math.min(100, (((v.usedDays ?? 0) + (v.plannedDays ?? 0)) / v.entitlementDays) * 100)
})

function range(a: Absence) {
  const from = format.shortDate(parseDate(a.startDate))
  return a.startDate === a.endDate ? from : `${from} – ${format.shortDate(parseDate(a.endDate))}`
}

// Dialogs
const dialogOpen = ref(false)
const editing = ref<Absence | null>(null)
function open(a: Absence | null) {
  editing.value = a
  dialogOpen.value = true
}
const deleting = ref<Absence | null>(null)
const confirmOpen = ref(false)
function askDelete(a: Absence) {
  deleting.value = a
  confirmOpen.value = true
}
const confirmDelete = () => deleting.value && run(call(commands.absenceDelete(deleting.value.id)))

const typeDialogOpen = ref(false)
const editingType = ref<AbsenceType | null>(null)
function openType(type: AbsenceType | null) {
  editingType.value = type
  typeDialogOpen.value = true
}

const holidayDate = ref(toDateInput(Date.now()))
const holidayName = ref('')
async function addHoliday() {
  const ok = await run(call(commands.holidayCreate(holidayDate.value, holidayName.value)))
  if (ok) holidayName.value = ''
}

// ---------------------------------------------------------------- filters
const me = computed(() => account.status.user?.id ?? '')
const mine = (a: Absence) => a.userId === me.value
/** Colleagues' absences are read only; the person's own can be edited. */
const everyone = computed(() => [...absences.value, ...others.value])
const withPeople = computed(() => others.value.length > 0)
const personName = (a: Absence) => (mine(a) ? t('timesheet.me') : org.nameOf(a.userId) || t('approvals.someone'))
watch(
  () => [others.value.length, workspaces.active?.orgId] as const,
  ([n, orgId]) => {
    if (n > 0 && orgId && org.people.length === 0) void org.loadGlance(orgId).catch(() => {})
  },
  { immediate: true },
)

const filter = ref({ people: [] as (string | null)[], types: [] as (string | null)[], statuses: [] as (string | null)[], months: [] as (string | null)[] })
const search = ref('')
const hasFilter = computed(() => search.value.trim() !== '' || Object.values(filter.value).some((v) => v.length > 0))
const clearFilter = () => {
  search.value = ''
  filter.value = { people: [], types: [], statuses: [], months: [] }
}
const personOptions = computed<FilterOption[]>(() => [
  { value: me.value, label: t('timesheet.me'), person: true },
  ...[...new Set(others.value.map((a) => a.userId))].map((id) => ({ value: id, label: org.nameOf(id) || t('approvals.someone'), person: true })),
])
const typeOptions = computed<FilterOption[]>(() => work.absenceTypes.map((x) => ({ value: x.id, label: work.absenceTypeName(x), color: x.color })))
const statusOptions = computed<FilterOption[]>(() => (['requested', 'approved', 'rejected'] as const).map((v) => ({ value: v, label: t(`absences.status.${v}`) })))
const monthOptions = computed<FilterOption[]>(() =>
  Array.from({ length: 12 }, (_, i) => ({ value: String(i + 1), label: new Intl.DateTimeFormat(locale.value, { month: 'long' }).format(new Date(2000, i, 15)) })),
)
/** Whether the absence touches one of the months (it may span several). */
function inMonths(a: Absence, months: string[]) {
  const y = String(year.value)
  const from = a.startDate.startsWith(y) ? Number(a.startDate.slice(5, 7)) : 1
  const to = a.endDate.startsWith(y) ? Number(a.endDate.slice(5, 7)) : 12
  return months.some((m) => Number(m) >= from && Number(m) <= to)
}
const filtered = computed(() => {
  const f = filter.value
  const words = search.value.trim().toLowerCase().split(/\s+/).filter(Boolean)
  const text = (a: Absence) => {
    const type = work.absenceTypesById.get(a.typeId)
    return [personName(a), type ? work.absenceTypeName(type) : '', a.note, range(a), t(`absences.status.${a.status}`)].join(' ').toLowerCase()
  }
  return everyone.value.filter(
    (a) =>
      (words.length === 0 || words.every((w) => text(a).includes(w))) &&
      (!f.people.length || f.people.includes(a.userId)) &&
      (!f.types.length || f.types.includes(a.typeId)) &&
      (!f.statuses.length || f.statuses.includes(a.status)) &&
      (!f.months.length || inMonths(a, f.months as string[])),
  )
})

const { sort, sorted: sortedAbsences } = useSort(filtered, {
  person: (a) => personName(a),
  period: (a) => a.startDate,
  type: (a) => {
    const t = work.absenceTypesById.get(a.typeId)
    return t ? work.absenceTypeName(t) : ''
  },
  status: (a) => a.status,
  note: (a) => a.note,
})
</script>

<template>
  <PageHeader :title="$t('absences.title')">
    <div class="flex items-center">
      <Button size="icon" variant="ghost" :title="$t('timesheet.previous')" @click="year--"><ChevronLeft /></Button>
      <span class="w-14 text-center font-medium tabular-nums">{{ year }}</span>
      <Button size="icon" variant="ghost" :title="$t('timesheet.next')" @click="year++"><ChevronRight /></Button>
    </div>
    <Button @click="open(null)"><Plus /> {{ $t('absences.add') }}</Button>
  </PageHeader>

  <div class="grid gap-6 lg:grid-cols-3">
    <div class="grid content-start gap-6 lg:col-span-2">
      <Card v-if="vacation">
        <CardHeader>
          <CardTitle>{{ $t('absences.vacation', { year }) }}</CardTitle>
        </CardHeader>
        <CardContent class="grid gap-4">
          <Progress :model-value="usedPct" />
          <dl class="grid grid-cols-4 gap-2 text-center text-sm">
            <div>
              <dt class="text-muted-foreground">{{ $t('absences.entitlement') }}</dt>
              <dd class="text-lg font-semibold tabular-nums">{{ days(vacation.entitlementDays) }}</dd>
            </div>
            <div>
              <dt class="text-muted-foreground">{{ $t('absences.used') }}</dt>
              <dd class="text-lg font-semibold tabular-nums">{{ days(vacation.usedDays) }}</dd>
            </div>
            <div>
              <dt class="text-muted-foreground">{{ $t('absences.planned') }}</dt>
              <dd class="text-lg font-semibold tabular-nums">{{ days(vacation.plannedDays) }}</dd>
            </div>
            <div>
              <dt class="text-muted-foreground">{{ $t('absences.remaining') }}</dt>
              <dd class="text-lg font-semibold tabular-nums">{{ days(vacation.remainingDays) }}</dd>
            </div>
          </dl>
        </CardContent>
      </Card>

      <EmptyState v-if="everyone.length === 0" :icon="Palmtree" :text="$t('absences.empty')">
        <Button variant="outline" @click="open(null)"><Plus /> {{ $t('absences.add') }}</Button>
      </EmptyState>
      <template v-else>
      <TableToolbar v-model="search" class="-mb-3" :active="hasFilter" @clear="clearFilter">
        <MultiFilter v-if="withPeople" v-model="filter.people" :label="$t('timesheet.person')" :options="personOptions" />
        <MultiFilter v-model="filter.types" :label="$t('absences.type')" :options="typeOptions" />
        <MultiFilter v-model="filter.statuses" :label="$t('absences.statusColumn')" :options="statusOptions" />
        <MultiFilter v-model="filter.months" :label="$t('absences.month')" :options="monthOptions" />
      </TableToolbar>
      <p v-if="sortedAbsences.length === 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('table.noMatch') }}</p>
      <div v-else class="overflow-x-auto rounded-lg border">
        <table class="w-full text-sm">
          <thead class="text-left text-xs text-muted-foreground">
            <tr class="border-b">
              <SortHead v-if="withPeople" :sort="sort" column="person" class="px-3 py-2 font-medium">{{ $t('timesheet.person') }}</SortHead>
              <SortHead :sort="sort" column="period" class="px-3 py-2 font-medium">{{ $t('absences.period') }}</SortHead>
              <SortHead :sort="sort" column="type" class="px-3 py-2 font-medium">{{ $t('absences.type') }}</SortHead>
              <SortHead :sort="sort" column="status" class="px-3 py-2 font-medium">{{ $t('absences.statusColumn') }}</SortHead>
              <SortHead :sort="sort" column="note" class="px-3 py-2 font-medium">{{ $t('entry.note') }}</SortHead>
              <th class="w-20 px-1 py-2"><span class="sr-only">{{ $t('common.edit') }}</span></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="a in sortedAbsences" :key="a.id" class="group border-b last:border-b-0 hover:bg-muted/30" :class="mine(a) ? 'cursor-pointer' : ''" @click="mine(a) && open(a)">
              <td v-if="withPeople" class="whitespace-nowrap px-3 py-1.5 font-medium"><span class="flex items-center gap-2"><UserAvatar :user-id="mine(a) ? org.myId : a.userId" :name="personName(a)" size="sm" />{{ personName(a) }}</span></td>
              <td class="whitespace-nowrap px-3 py-1.5 font-medium tabular-nums">{{ range(a) }}</td>
              <td class="whitespace-nowrap px-3 py-1.5">
                <span class="flex items-center gap-2">
                  <span class="size-3 shrink-0 rounded-full" :style="{ backgroundColor: work.absenceTypesById.get(a.typeId)?.color }" />
                  {{ work.absenceTypesById.get(a.typeId) ? work.absenceTypeName(work.absenceTypesById.get(a.typeId)!) : '' }}
                  <span v-if="a.halfDay" class="text-xs text-muted-foreground">{{ $t(`absences.halfDays.${a.halfDay}`) }}</span>
                </span>
              </td>
              <td class="whitespace-nowrap px-3 py-1.5">
                <Badge v-if="a.status !== 'approved'" :variant="a.status === 'rejected' ? 'destructive' : 'secondary'">{{ $t(`absences.status.${a.status}`) }}</Badge>
                <span v-else class="text-muted-foreground">{{ $t('absences.status.approved') }}</span>
              </td>
              <td class="max-w-0 w-full truncate px-3 py-1.5 text-muted-foreground" :title="a.note">
                <span v-if="a.status === 'rejected' && a.decisionNote" class="mr-2 text-xs">{{ a.decisionNote }}</span>{{ a.note }}
              </td>
              <td class="px-1 py-0.5" @click.stop>
                <div v-if="mine(a)" class="flex justify-end opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                  <Button size="icon" variant="ghost" class="size-7" :title="$t('common.edit')" @click="open(a)"><Pencil /></Button>
                  <Button size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="askDelete(a)"><Trash2 /></Button>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      </template>
    </div>

    <div class="grid content-start gap-6">
      <Card>
        <CardHeader class="flex flex-row items-center justify-between">
          <CardTitle>{{ $t('absences.types') }}</CardTitle>
          <Button size="sm" variant="outline" @click="openType(null)"><Plus /> {{ $t('absences.addType') }}</Button>
        </CardHeader>
        <CardContent>
          <ul class="grid gap-1 text-sm">
            <li v-for="type in work.absenceTypes" :key="type.id" class="group flex items-center gap-2">
              <span class="size-2.5 rounded-full" :style="{ backgroundColor: type.color }" />
              <span class="flex-1">{{ work.absenceTypeName(type) }}</span>
              <Button size="icon" variant="ghost" class="size-7 opacity-0 group-hover:opacity-100" @click="openType(type)"><Pencil /></Button>
              <Button
                v-if="type.key === null"
                size="icon"
                variant="ghost"
                class="size-7 opacity-0 group-hover:opacity-100"
                @click="run(call(commands.absenceTypeDelete(type.id)))"
              ><Trash2 /></Button>
            </li>
          </ul>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{{ $t('absences.holidays', { year }) }}</CardTitle>
        </CardHeader>
        <CardContent class="grid gap-3">
          <ul class="grid gap-1 text-sm">
            <li v-for="h in holidays" :key="`${h.date}-${h.name}`" class="group flex items-center gap-2" :class="h.dayOff ? '' : 'text-muted-foreground'">
              <span class="w-20 tabular-nums">{{ format.shortDate(parseDate(h.date)) }}</span>
              <span class="flex-1">{{ h.name }}</span>
              <span v-if="!h.dayOff" class="text-xs">{{ $t('absences.informational') }}</span>
              <template v-if="h.customId">
                <span class="text-xs text-muted-foreground">{{ $t('absences.custom') }}</span>
                <Button size="icon" variant="ghost" class="size-7 opacity-0 group-hover:opacity-100" @click="run(call(commands.holidayDelete(h.customId)))"><Trash2 /></Button>
              </template>
            </li>
          </ul>
          <form class="grid gap-2" @submit.prevent="addHoliday">
            <div class="flex gap-2">
              <DateInput v-model="holidayDate" class="w-40" required />
              <Input v-model="holidayName" :placeholder="t('absences.holidayName')" required />
            </div>
            <Button type="submit" size="sm" variant="outline"><Plus /> {{ $t('absences.addHoliday') }}</Button>
          </form>
        </CardContent>
      </Card>
    </div>
  </div>

  <AbsenceDialog v-model:open="dialogOpen" :absence="editing" />
  <AbsenceTypeDialog v-model:open="typeDialogOpen" :type="editingType" />
  <ConfirmDialog v-model:open="confirmOpen" :title="$t('absences.deleteConfirm')" @confirm="confirmDelete" />
</template>
