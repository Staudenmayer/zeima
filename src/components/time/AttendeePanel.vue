<script setup lang="ts">
import { onClickOutside } from '@vueuse/core'
import { X } from '@lucide/vue'
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { Input } from '@/components/ui/input'
import UserAvatar from '@/components/time/UserAvatar.vue'
import { PRESENCE_COLORS } from '@/lib/presence'
import { useOrgStore } from '@/stores/org'

// The people of a meeting: a search field that suggests colleagues, and those added below it,
// grouped by department like the online list.
const props = defineProps<{ people: { id: string; name: string | null }[] }>()
const model = defineModel<string[]>({ required: true })
const { t } = useI18n()
const org = useOrgStore()

const query = ref('')
const open = ref(false)
const root = ref<HTMLElement | null>(null)
onClickOutside(root, () => (open.value = false))

const name = (id: string) => org.nameOf(id) || props.people.find((p) => p.id === id)?.name || t('team.someone')
const department = (id: string) => org.directory.find((e) => e.id === id)?.departments[0] ?? null
const state = (id: string) => org.presence.find((p) => p.userId === id)?.state ?? 'offline'
const stateText = (id: string) => (state(id) === 'offline' ? t('org.offline') : t(`org.presenceStates.${state(id)}`))

const options = computed(() => {
  const q = query.value.trim().toLowerCase()
  return props.people
    .filter((p) => !model.value.includes(p.id) && (!q || name(p.id).toLowerCase().includes(q)))
    .sort((a, b) => name(a.id).localeCompare(name(b.id)))
})
const groups = computed(() => {
  const by = new Map<string | null, string[]>()
  for (const id of model.value) by.set(department(id), [...(by.get(department(id)) ?? []), id])
  const mine = org.myId ? department(org.myId) : null
  return [...by.entries()]
    .sort(([x], [y]) => (x === null ? 1 : y === null ? -1 : x === mine ? -1 : y === mine ? 1 : x.localeCompare(y)))
    .map(([dept, ids]) => ({ dept, ids: [...ids].sort((a, b) => name(a).localeCompare(name(b))) }))
})

function toggle(id: string) {
  model.value = model.value.includes(id) ? model.value.filter((x) => x !== id) : [...model.value, id]
  query.value = ''
}
function onKey(e: KeyboardEvent) {
  if (e.key === 'Enter') {
    e.preventDefault()
    if (options.value[0]) toggle(options.value[0].id)
  } else if (e.key === 'Backspace' && !query.value && model.value.length) {
    model.value = model.value.slice(0, -1)
  } else if (e.key === 'Escape') {
    open.value = false
  }
}
</script>

<template>
  <div class="flex min-h-0 flex-col gap-2">
    <div class="flex items-baseline h-3.5 justify-between gap-2 text-sm leading-none font-medium">
      <span>{{ $t('meetings.attendees') }}</span>
      <span class="text-xs leading-none font-normal text-muted-foreground">{{ model.length ? $t('meetings.invited', { n: model.length }) : $t('meetings.onlyYou') }}</span>
    </div>
    <div ref="root" class="relative">
      <Input v-model="query" autocomplete="off" role="combobox" :aria-expanded="open" :placeholder="$t('meetings.searchPeople')" @focus="open = true" @input="open = true" @keydown="onKey" />
      <ul v-if="open && options.length" class="absolute top-full right-0 left-0 z-50 mt-1 max-h-56 overflow-y-auto rounded-md border bg-popover p-1 text-sm text-popover-foreground shadow-md">
        <li v-for="p in options" :key="p.id">
          <button type="button" class="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left hover:bg-accent" @click="toggle(p.id)">
            <UserAvatar :user-id="p.id" size="xs" />
            <span class="flex-1 truncate">{{ name(p.id) }}</span>
            <span class="truncate text-xs text-muted-foreground">{{ department(p.id) }}</span>
          </button>
        </li>
      </ul>
    </div>
    <div class="grid min-h-0 gap-2 overflow-y-auto">
      <section v-for="g in groups" :key="g.dept ?? ''">
        <h3 v-if="groups.length > 1 || g.dept" class="px-1 pt-1 pb-1 text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">{{ g.dept ?? $t('directory.noDepartment') }} - {{ g.ids.length }}</h3>
        <ul class="grid gap-0.5">
          <li v-for="id in g.ids" :key="id" class="group flex items-center gap-2.5 rounded-lg px-1 py-1 hover:bg-muted/60">
            <span class="relative inline-flex shrink-0">
              <UserAvatar :user-id="id" :name="name(id)" size="md" />
              <span class="absolute -right-0.5 -bottom-0.5 aspect-square size-3 rounded-full ring-2 ring-background" :style="{ backgroundColor: PRESENCE_COLORS[state(id)] }" :title="stateText(id)" />
            </span>
            <span class="grid min-w-0 flex-1 leading-4">
              <span class="truncate text-sm font-medium">{{ name(id) }}</span>
              <span class="truncate text-xs text-muted-foreground">{{ stateText(id) }}</span>
            </span>
            <button type="button" class="rounded p-1 text-muted-foreground opacity-0 hover:text-foreground group-hover:opacity-100 focus-visible:opacity-100" :title="$t('common.delete')" :aria-label="$t('common.delete')" @click="toggle(id)"><X class="size-4" /></button>
          </li>
        </ul>
      </section>
    </div>
  </div>
</template>
