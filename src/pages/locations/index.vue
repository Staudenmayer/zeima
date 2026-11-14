<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Building, CalendarCheck, ChevronLeft, ChevronRight, Home, MapPinned, Plane, Plus, Settings2, Trash2, User as UserIcon } from '@lucide/vue'
import { commands, type LocationKind, type LocationPattern, type Office, type WorkLocation } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import EmptyState from '@/components/time/EmptyState.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import { useFormat } from '@/composables/useFormat'
import { call, errorMessage } from '@/lib/ipc'
import { addDays, startOfWeek, toDateInput } from '@/lib/time'
import { useAccountStore } from '@/stores/account'
import { useOrgStore } from '@/stores/org'
import { useWorkspaceStore } from '@/stores/workspace'
import { useI18n } from 'vue-i18n'

definePage({ meta: { titleKey: 'nav.locations' } })

const { t } = useI18n()
const format = useFormat()
const account = useAccountStore()
const org = useOrgStore()
const workspaces = useWorkspaceStore()

const KINDS: LocationKind[] = ['office', 'home', 'travel', 'other']
const ICONS = { office: Building, home: Home, travel: Plane, other: MapPinned } as const
const COLORS: Record<LocationKind, string> = { office: 'text-blue-600', home: 'text-green-600', travel: 'text-amber-600', other: 'text-muted-foreground' }

const view = ref<'week' | 'month'>('week')
const weekStart = ref(startOfWeek(Date.now()))
/** The shown days: a week, or the whole month the week starts in. */
const days = computed(() => {
  if (view.value === 'week') return Array.from({ length: 7 }, (_, i) => addDays(weekStart.value, i))
  const d = new Date(weekStart.value)
  const first = new Date(d.getFullYear(), d.getMonth(), 1, 12).getTime()
  const count = new Date(d.getFullYear(), d.getMonth() + 1, 0).getDate()
  return Array.from({ length: count }, (_, i) => addDays(first, i))
})
const monthName = computed(() => new Date(weekStart.value).toLocaleDateString([], { month: 'long', year: 'numeric' }))
const iso = (ms: number) => toDateInput(ms)
const locations = ref<WorkLocation[]>([])
const offices = ref<Office[]>([])
const pattern = ref<LocationPattern>({ days: [] })
const me = computed(() => account.status.user?.id ?? '')

/** One row per person: you first, then the others who have a day in this week, then anyone else. */
const people = computed(() => {
  const ids = new Set<string>([me.value, ...locations.value.map((l) => l.userId)])
  const rows = [...ids].filter(Boolean).map((id) => ({ id, name: id === me.value ? '' : org.nameOf(id) }))
  return rows.sort((a, b) => (a.id === me.value ? -1 : b.id === me.value ? 1 : a.name.localeCompare(b.name)))
})
const at = (userId: string, day: number): WorkLocation | undefined => locations.value.find((l) => l.userId === userId && l.date === iso(day))
const kindAt = (userId: string, day: number): LocationKind | null => at(userId, day)?.kind ?? null
const officeName = (id: string | null) => offices.value.find((o) => o.id === id)?.name ?? ''
/** How many are in the office on a day. */
const inOffice = (day: number) => locations.value.filter((l) => l.date === iso(day) && l.kind === 'office').length
/** Per office and day: how many, and whether it is over the capacity. */
const officeLoad = (office: Office, day: number) => locations.value.filter((l) => l.date === iso(day) && l.officeId === office.id).length

async function load() {
  if (!workspaces.inOrg) return
  try {
    if (org.people.length === 0) await org.load().catch(() => {})
    const from = days.value[0]!
    const to = days.value[days.value.length - 1]!
    ;[locations.value, offices.value, pattern.value] = await Promise.all([
      call(commands.locationsList(iso(from), iso(to))),
      call(commands.officesList()),
      call(commands.locationPatternGet()),
    ])
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
onMounted(load)
watch([weekStart, view, () => workspaces.active?.id], load)

async function set(day: number, kind: LocationKind | null, officeId: string | null = null) {
  try {
    await call(commands.locationSet(iso(day), kind, officeId))
    await load()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
const move = (steps: number) => {
  if (view.value === 'week') weekStart.value = addDays(weekStart.value, steps * 7)
  else {
    const d = new Date(weekStart.value)
    weekStart.value = startOfWeek(new Date(d.getFullYear(), d.getMonth() + steps, 1, 12).getTime())
  }
}

// ---------------------------------------------------------------- usual week
const patternOpen = ref(false)
const PATTERN_DAYS = [1, 2, 3, 4, 5]
const draft = ref<Record<number, string>>({})
function openPattern() {
  draft.value = Object.fromEntries(pattern.value.days.map((d) => [d.weekday, d.officeId ? `office:${d.officeId}` : d.kind]))
  patternOpen.value = true
}
async function savePattern() {
  const dayList = Object.entries(draft.value)
    .filter(([, v]) => v && v !== 'none')
    .map(([weekday, v]) => (v.startsWith('office:') ? { weekday: Number(weekday), kind: 'office' as const, officeId: v.slice(7) } : { weekday: Number(weekday), kind: v as LocationKind, officeId: null }))
  try {
    await call(commands.locationPatternSet({ days: dayList }))
    patternOpen.value = false
    await load()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
async function applyPattern() {
  try {
    const n = await call(commands.locationPatternApply(iso(days.value[0]!), iso(days.value[days.value.length - 1]!)))
    toast.success(t('locations.applied', { n }))
    await load()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

// ------------------------------------------------------------------- offices
const officesOpen = ref(false)
const newOffice = ref({ name: '', capacity: '' })
async function addOffice() {
  try {
    await call(commands.officeSave(null, newOffice.value.name, newOffice.value.capacity ? Number(newOffice.value.capacity) : null))
    newOffice.value = { name: '', capacity: '' }
    await load()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
async function removeOffice(id: string) {
  try {
    await call(commands.officeDelete(id))
    await load()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
const today = computed(() => iso(Date.now()))
</script>

<template>
  <PageHeader :title="$t('nav.locations')">
    <div class="flex flex-wrap items-center gap-1">
      <div class="mr-2 flex gap-1">
        <Button size="sm" :variant="view === 'week' ? 'default' : 'outline'" @click="view = 'week'">{{ $t('locations.week') }}</Button>
        <Button size="sm" :variant="view === 'month' ? 'default' : 'outline'" @click="view = 'month'">{{ $t('locations.month') }}</Button>
      </div>
      <Button size="icon" variant="outline" :title="$t('locations.previous')" @click="move(-1)"><ChevronLeft /></Button>
      <Button size="sm" variant="outline" @click="weekStart = startOfWeek(Date.now())">{{ $t('locations.today') }}</Button>
      <Button size="icon" variant="outline" :title="$t('locations.next')" @click="move(1)"><ChevronRight /></Button>
    </div>
  </PageHeader>

  <EmptyState v-if="!workspaces.inOrg" :icon="MapPinned" :text="$t('locations.personal')" />

  <div v-else class="grid gap-3">
    <div class="flex flex-wrap items-center gap-2">
      <h2 class="text-sm font-medium">{{ view === 'week' ? `${format.shortDate(days[0]!)} – ${format.shortDate(days[6]!)}` : monthName }}</h2>
      <span class="flex-1" />
      <Button size="sm" variant="outline" @click="applyPattern"><CalendarCheck /> {{ $t('locations.applyPattern') }}</Button>
      <Button size="sm" variant="ghost" @click="openPattern"><Settings2 /> {{ $t('locations.usualWeek') }}</Button>
      <Button v-if="workspaces.canEditShared" size="sm" variant="ghost" @click="officesOpen = true"><Building /> {{ $t('locations.offices') }}</Button>
    </div>
    <div class="overflow-x-auto rounded-lg border">
      <table class="w-full text-sm" :class="view === 'month' ? 'min-w-[70rem]' : 'min-w-[40rem]'">
        <thead>
          <tr class="border-b text-left text-xs text-muted-foreground">
            <th class="w-40 px-3 py-2 font-normal" />
            <th v-for="d in days" :key="d" class="px-1 py-2 text-center font-normal" :class="iso(d) === today ? 'font-medium text-foreground' : ''">
              {{ format.weekday(d) }} {{ new Date(d).getDate() }}
            </th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="p in people" :key="p.id" class="border-b last:border-0">
            <td class="truncate px-3 py-2 font-medium">
              <span v-if="p.id === me" class="flex items-center gap-1.5"><UserIcon class="size-3.5" /> {{ $t('locations.you') }}</span>
              <span v-else>{{ p.name || $t('locations.someone') }}</span>
            </td>
            <td v-for="d in days" :key="d" class="px-0.5 py-1 text-center">
              <DropdownMenu v-if="p.id === me">
                <DropdownMenuTrigger as-child>
                  <Button variant="ghost" size="sm" class="h-9 w-full px-1" :aria-label="$t('locations.set')" :title="officeName(at(p.id, d)?.officeId ?? null)">
                    <component :is="ICONS[kindAt(p.id, d)!]" v-if="kindAt(p.id, d)" class="size-4" :class="COLORS[kindAt(p.id, d)!]" />
                    <Plus v-else class="size-3.5 text-muted-foreground opacity-40" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="center">
                  <template v-for="k in KINDS" :key="k">
                    <template v-if="k === 'office' && offices.length">
                      <DropdownMenuItem v-for="o in offices" :key="o.id" class="gap-2" @click="set(d, 'office', o.id)">
                        <Building class="size-4" :class="COLORS.office" /> {{ o.name }}
                      </DropdownMenuItem>
                    </template>
                    <DropdownMenuItem v-else class="gap-2" @click="set(d, k)">
                      <component :is="ICONS[k]" class="size-4" :class="COLORS[k]" /> {{ $t(`locations.kinds.${k}`) }}
                    </DropdownMenuItem>
                  </template>
                  <DropdownMenuItem v-if="kindAt(p.id, d)" class="text-muted-foreground" @click="set(d, null)">{{ $t('locations.clear') }}</DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
              <component :is="ICONS[kindAt(p.id, d)!]" v-else-if="kindAt(p.id, d)" class="mx-auto size-4" :class="COLORS[kindAt(p.id, d)!]" :title="[$t(`locations.kinds.${kindAt(p.id, d)!}`), officeName(at(p.id, d)?.officeId ?? null)].filter(Boolean).join(' · ')" />
            </td>
          </tr>
        </tbody>
        <tfoot>
          <tr class="text-xs text-muted-foreground">
            <td class="px-3 py-2">{{ $t('locations.inOffice') }}</td>
            <td v-for="d in days" :key="d" class="px-1 py-2 text-center tabular-nums">{{ inOffice(d) || '' }}</td>
          </tr>
          <tr v-for="o in offices" :key="o.id" class="text-xs text-muted-foreground">
            <td class="truncate px-3 py-1">{{ o.name }}<span v-if="o.capacity"> ({{ o.capacity }})</span></td>
            <td v-for="d in days" :key="d" class="px-1 py-1 text-center tabular-nums" :class="o.capacity && officeLoad(o, d) > o.capacity ? 'font-semibold text-destructive' : ''" :title="o.capacity && officeLoad(o, d) > o.capacity ? $t('locations.over') : ''">
              {{ officeLoad(o, d) || '' }}
            </td>
          </tr>
        </tfoot>
      </table>
    </div>
    <ul class="flex flex-wrap gap-4 text-xs text-muted-foreground">
      <li v-for="k in KINDS" :key="k" class="flex items-center gap-1.5"><component :is="ICONS[k]" class="size-3.5" :class="COLORS[k]" /> {{ $t(`locations.kinds.${k}`) }}</li>
    </ul>
    <p class="text-xs text-muted-foreground">{{ $t('locations.hint') }}</p>
  </div>

  <Dialog v-model:open="patternOpen">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-md">
      <DialogHeader><DialogTitle>{{ $t('locations.usualWeek') }}</DialogTitle></DialogHeader>
      <form class="grid gap-3" @submit.prevent="savePattern">
        <div v-for="wd in PATTERN_DAYS" :key="wd" class="flex items-center gap-3">
          <span class="w-24 text-sm">{{ format.weekday(addDays(startOfWeek(Date.now()), wd - 1)) }}</span>
          <Select v-model="draft[wd]">
            <SelectTrigger class="flex-1"><SelectValue :placeholder="$t('locations.nothing')" /></SelectTrigger>
            <SelectContent>
              <SelectItem value="none">{{ $t('locations.nothing') }}</SelectItem>
              <template v-for="k in KINDS" :key="k">
                <template v-if="k === 'office' && offices.length">
                  <SelectItem v-for="o in offices" :key="o.id" :value="`office:${o.id}`">{{ o.name }}</SelectItem>
                </template>
                <SelectItem v-else :value="k">{{ $t(`locations.kinds.${k}`) }}</SelectItem>
              </template>
            </SelectContent>
          </Select>
        </div>
        <p class="text-xs text-muted-foreground">{{ $t('locations.patternHint') }}</p>
        <DialogFooter>
          <Button type="button" variant="outline" @click="patternOpen = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit">{{ $t('common.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>

  <Dialog v-model:open="officesOpen">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-md">
      <DialogHeader><DialogTitle>{{ $t('locations.offices') }}</DialogTitle></DialogHeader>
      <ul v-if="offices.length" class="divide-y rounded-md border text-sm">
        <li v-for="o in offices" :key="o.id" class="flex items-center gap-3 px-3 py-2">
          <span class="flex-1">{{ o.name }}</span>
          <span class="text-xs text-muted-foreground">{{ o.capacity ? $t('locations.capacity', { n: o.capacity }) : $t('locations.noLimit') }}</span>
          <Button size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="removeOffice(o.id)"><Trash2 /></Button>
        </li>
      </ul>
      <form class="flex gap-2" @submit.prevent="addOffice">
        <Input v-model="newOffice.name" :placeholder="$t('locations.officeName')" required maxlength="100" />
        <Input v-model="newOffice.capacity" type="number" min="1" class="w-28" :placeholder="$t('locations.capacityPlaceholder')" />
        <Button type="submit"><Plus /></Button>
      </form>
    </DialogContent>
  </Dialog>
</template>
