<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Building, ChevronLeft, ChevronRight, Home, MapPinned, Plane, Plus, User as UserIcon } from '@lucide/vue'
import { commands, type LocationKind, type WorkLocation } from '@/bindings'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import EmptyState from '@/components/time/EmptyState.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import { useFormat } from '@/composables/useFormat'
import { call, errorMessage } from '@/lib/ipc'
import { addDays, startOfWeek, toDateInput } from '@/lib/time'
import { useAccountStore } from '@/stores/account'
import { useOrgStore } from '@/stores/org'
import { useWorkspaceStore } from '@/stores/workspace'

definePage({ meta: { titleKey: 'nav.locations' } })

const format = useFormat()
const account = useAccountStore()
const org = useOrgStore()
const workspaces = useWorkspaceStore()

const KINDS: LocationKind[] = ['office', 'home', 'travel', 'other']
const ICONS = { office: Building, home: Home, travel: Plane, other: MapPinned } as const
const COLORS: Record<LocationKind, string> = { office: 'text-blue-600', home: 'text-green-600', travel: 'text-amber-600', other: 'text-muted-foreground' }

const weekStart = ref(startOfWeek(Date.now()))
const days = computed(() => Array.from({ length: 7 }, (_, i) => addDays(weekStart.value, i)))
const iso = (ms: number) => toDateInput(ms)
const locations = ref<WorkLocation[]>([])
const me = computed(() => account.status.user?.id ?? '')

/** One row per person: you first, then the others who have a day in this week, then anyone else. */
const people = computed(() => {
  const ids = new Set<string>([me.value, ...locations.value.map((l) => l.userId)])
  const rows = [...ids].filter(Boolean).map((id) => ({ id, name: id === me.value ? '' : org.nameOf(id) }))
  return rows.sort((a, b) => (a.id === me.value ? -1 : b.id === me.value ? 1 : a.name.localeCompare(b.name)))
})
const kindAt = (userId: string, day: number): LocationKind | null => locations.value.find((l) => l.userId === userId && l.date === iso(day))?.kind ?? null
/** How many are in the office on a day. */
const inOffice = (day: number) => locations.value.filter((l) => l.date === iso(day) && l.kind === 'office').length

async function load() {
  if (!workspaces.inOrg) return
  try {
    if (org.people.length === 0) await org.load().catch(() => {})
    locations.value = await call(commands.locationsList(iso(weekStart.value), iso(addDays(weekStart.value, 6))))
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
onMounted(load)
watch([weekStart, () => workspaces.active?.id], load)

async function set(day: number, kind: LocationKind | null) {
  try {
    await call(commands.locationSet(iso(day), kind))
    await load()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
const move = (weeks: number) => (weekStart.value = addDays(weekStart.value, weeks * 7))
const today = computed(() => iso(Date.now()))
</script>

<template>
  <PageHeader :title="$t('nav.locations')">
    <div class="flex items-center gap-1">
      <Button size="icon" variant="outline" :title="$t('locations.previous')" @click="move(-1)"><ChevronLeft /></Button>
      <Button size="sm" variant="outline" @click="weekStart = startOfWeek(Date.now())">{{ $t('locations.thisWeek') }}</Button>
      <Button size="icon" variant="outline" :title="$t('locations.next')" @click="move(1)"><ChevronRight /></Button>
    </div>
  </PageHeader>

  <EmptyState v-if="!workspaces.inOrg" :icon="MapPinned" :text="$t('locations.personal')" />

  <div v-else class="grid gap-3">
    <div class="overflow-x-auto rounded-lg border">
      <table class="w-full min-w-[40rem] text-sm">
        <thead>
          <tr class="border-b text-left text-xs text-muted-foreground">
            <th class="w-40 px-3 py-2 font-normal">{{ format.shortDate(weekStart) }} – {{ format.shortDate(days[6]!) }}</th>
            <th v-for="d in days" :key="d" class="px-2 py-2 text-center font-normal" :class="iso(d) === today ? 'font-medium text-foreground' : ''">
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
            <td v-for="d in days" :key="d" class="px-1 py-1 text-center">
              <DropdownMenu v-if="p.id === me">
                <DropdownMenuTrigger as-child>
                  <Button variant="ghost" size="sm" class="h-9 w-full" :aria-label="$t('locations.set')">
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
            <td v-for="d in days" :key="d" class="px-2 py-2 text-center tabular-nums">{{ inOffice(d) || '' }}</td>
          </tr>
        </tfoot>
      </table>
    </div>
    <ul class="flex flex-wrap gap-4 text-xs text-muted-foreground">
      <li v-for="k in KINDS" :key="k" class="flex items-center gap-1.5"><component :is="ICONS[k]" class="size-3.5" :class="COLORS[k]" /> {{ $t(`locations.kinds.${k}`) }}</li>
    </ul>
    <p class="text-xs text-muted-foreground">{{ $t('locations.hint') }}</p>
  </div>
</template>
