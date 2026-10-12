<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { ChevronLeft, ChevronRight, Palmtree, Pencil, Plus, Trash2 } from '@lucide/vue'
import { commands, type Absence, type AbsenceType, type HolidayView, type VacationSummary } from '@/bindings'
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
import { useWorkStore } from '@/stores/work'

definePage({ meta: { titleKey: 'nav.absences' } })

const { t, locale } = useI18n()
const work = useWorkStore()
const format = useFormat()

const year = ref(Number(toDateInput(Date.now()).slice(0, 4)))
const absences = ref<Absence[]>([])
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
  const [a, h, v] = await Promise.all([
    run(call(commands.absenceList(`${y}-01-01`, `${y}-12-31`))),
    run(call(commands.holidayList(y, locale.value))),
    run(call(commands.vacationSummary(y))),
  ])
  absences.value = a ?? []
  holidays.value = h ?? []
  vacation.value = v ?? null
}
// Reload when the year, the language or any work data changes.
watch([year, locale, () => work.schedules, () => work.absenceTypes, () => work.balance], load, { immediate: true })

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

      <EmptyState v-if="absences.length === 0" :icon="Palmtree" :text="$t('absences.empty')">
        <Button variant="outline" @click="open(null)"><Plus /> {{ $t('absences.add') }}</Button>
      </EmptyState>
      <ul v-else class="divide-y rounded-lg border">
        <li v-for="a in absences" :key="a.id" class="group flex items-center gap-3 px-3 py-2 text-sm hover:bg-muted/40">
          <span class="size-3 rounded-full" :style="{ backgroundColor: work.absenceTypesById.get(a.typeId)?.color }" />
          <span class="w-40 font-medium">{{ range(a) }}</span>
          <span>{{ work.absenceTypesById.get(a.typeId) ? work.absenceTypeName(work.absenceTypesById.get(a.typeId)!) : '' }}</span>
          <span v-if="a.halfDay" class="text-xs text-muted-foreground">{{ $t(`absences.halfDays.${a.halfDay}`) }}</span>
          <span class="flex-1 truncate text-muted-foreground">{{ a.note }}</span>
          <div class="flex opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
            <Button size="icon" variant="ghost" class="size-7" :title="$t('common.edit')" @click="open(a)"><Pencil /></Button>
            <Button size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="askDelete(a)"><Trash2 /></Button>
          </div>
        </li>
      </ul>
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
