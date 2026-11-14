<script setup lang="ts">
import UserAvatar from '@/components/time/UserAvatar.vue'
import { computed, onMounted, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Building, CalendarCheck, ChevronLeft, ChevronRight, Home, MapPinned, Plane, Plus, Settings2 } from '@lucide/vue'
import { commands, type LocationKind, type LocationPattern, type WorkLocation } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
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
/** How many are in the office on a day. */
const inOffice = (day: number) => locations.value.filter((l) => l.date === iso(day) && l.kind === 'office').length

async function load() {
  if (!workspaces.inOrg) return
  try {
    if (org.people.length === 0) await org.load().catch(() => {})
    const from = days.value[0]!
    const to = days.value[days.value.length - 1]!
    ;[locations.value, pattern.value] = await Promise.all([
      call(commands.locationsList(iso(from), iso(to))),
      call(commands.locationPatternGet()),
    ])
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
onMounted(load)
watch([weekStart, view, () => workspaces.active?.id], load)

async function set(day: number, kind: LocationKind | null) {
  try {
    await call(commands.locationSet(iso(day), kind, null))
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
  draft.value = Object.fromEntries(pattern.value.days.map((d) => [d.weekday, d.kind]))
  patternOpen.value = true
}
async function savePattern() {
  const dayList = Object.entries(draft.value)
    .filter(([, v]) => v && v !== 'none')
    .map(([weekday, v]) => ({ weekday: Number(weekday), kind: v as LocationKind, officeId: null }))
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
              <span v-if="p.id === me" class="flex items-center gap-1.5"><UserAvatar :user-id="org.myId" size="xs" /> {{ $t('locations.you') }}</span>
              <span v-else class="flex items-center gap-1.5"><UserAvatar :user-id="p.id" :name="p.name" size="xs" />{{ p.name || $t('locations.someone') }}</span>
            </td>
            <td v-for="d in days" :key="d" class="px-0.5 py-1 text-center">
              <DropdownMenu v-if="p.id === me">
                <DropdownMenuTrigger as-child>
                  <Button variant="ghost" size="sm" class="h-9 w-full px-1" :aria-label="$t('locations.set')">
                    <component :is="ICONS[kindAt(p.id, d)!]" v-if="kindAt(p.id, d)" class="size-4" :class="COLORS[kindAt(p.id, d)!]" />
                    <Plus v-else class="size-3.5 text-muted-foreground opacity-40" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="center">
                  <DropdownMenuItem v-for="k in KINDS" :key="k" class="gap-2" @click="set(d, k)">
                    <component :is="ICONS[k]" class="size-4" :class="COLORS[k]" /> {{ $t(`locations.kinds.${k}`) }}
                  </DropdownMenuItem>
                  <DropdownMenuItem v-if="kindAt(p.id, d)" class="text-muted-foreground" @click="set(d, null)">{{ $t('locations.clear') }}</DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
              <component :is="ICONS[kindAt(p.id, d)!]" v-else-if="kindAt(p.id, d)" class="mx-auto size-4" :class="COLORS[kindAt(p.id, d)!]" :title="$t(`locations.kinds.${kindAt(p.id, d)!}`)" />
            </td>
          </tr>
        </tbody>
        <tfoot>
          <tr class="text-xs text-muted-foreground">
            <td class="px-3 py-2">{{ $t('locations.inOffice') }}</td>
            <td v-for="d in days" :key="d" class="px-1 py-2 text-center tabular-nums">{{ inOffice(d) || '' }}</td>
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
              <SelectItem v-for="k in KINDS" :key="k" :value="k">{{ $t(`locations.kinds.${k}`) }}</SelectItem>
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
</template>
