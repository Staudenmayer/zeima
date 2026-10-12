<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Pause, Play, Puzzle, Search, Square } from '@lucide/vue'
import { commands, type Project, type Tag } from '@/bindings'
import { Button } from '@/components/ui/button'
import { call, errorMessage } from '@/lib/ipc'
import { formatClock } from '@/lib/time'
import { parseQuery, rank, resolve } from '@/lib/quickswitch'
import { useProjectsStore } from '@/stores/projects'
import { useTagsStore } from '@/stores/tags'
import { useTimerStore } from '@/stores/timer'
import ProjectDot from './ProjectDot.vue'
import TagList from './TagList.vue'

const emit = defineEmits<{ done: [] }>()

const { t } = useI18n()
const projects = useProjectsStore()
const tags = useTagsStore()
const timer = useTimerStore()

const query = ref('')
const selected = ref(0)
const recentIds = ref<string[]>([])
const input = ref<HTMLInputElement | null>(null)

interface Option {
  key: string
  project: Project | null
  tagIds: string[]
  note: string
  /** From an addon (quickswitch.suggest): shown with its own label. */
  addon?: { label: string; detail: string | null }
}

// Addon suggestions, fetched shortly after typing stops.
const addonOptions = ref<Option[]>([])
let suggestTimer: ReturnType<typeof setTimeout> | undefined
watch(query, (q) => {
  clearTimeout(suggestTimer)
  suggestTimer = setTimeout(async () => {
    const list = await call(commands.addonSuggest(q)).catch(() => [])
    if (q !== query.value) return
    addonOptions.value = list.map((s, i) => ({
      key: `addon:${s.addonId}:${i}`,
      project: s.projectId ? (projects.byId.get(s.projectId) ?? null) : null,
      tagIds: s.tagIds,
      note: s.note,
      addon: { label: s.label, detail: s.detail },
    }))
  }, 150)
}, { immediate: true })

const parsed = computed(() => parseQuery(query.value))

function resolveTags(projectId: string | null): { ids: string[]; unknown: string[] } {
  const usable = tags.usableFor(projectId)
  const ids: string[] = []
  const unknown: string[] = []
  for (const name of parsed.value.tags) {
    const tag: Tag | null = resolve(usable, name)
    if (tag) ids.push(tag.id)
    else unknown.push(name)
  }
  return { ids: [...new Set(ids)], unknown }
}

/** Recently used projects first, then the rest alphabetically. */
const orderedProjects = computed(() => {
  const recent = recentIds.value.map((id) => projects.byId.get(id)).filter((p): p is Project => !!p && p.status !== 'archived')
  const rest = projects.active.filter((p) => !recentIds.value.includes(p.id))
  return [...recent, ...rest]
})

const unknownProject = computed(() => {
  const token = parsed.value.project
  return token !== null && !resolve(projects.active, token) ? token : null
})

const options = computed<Option[]>(() => {
  const p = parsed.value
  if (p.project !== null) {
    // Explicit `#ALIAS`: a single, fully specified option.
    const project = resolve(projects.active, p.project)
    if (!project) return []
    return [{ key: project.id, project, tagIds: resolveTags(project.id).ids, note: p.note }]
  }
  // Addon options sit between the matching projects and "without project".
  // Free text filters the projects; the last option starts without project
  // and keeps the text as note.
  const list: Option[] = rank(orderedProjects.value, p.note).map((project) => ({
    key: project.id,
    project,
    tagIds: resolveTags(project.id).ids,
    note: '',
  }))
  list.push(...addonOptions.value)
  list.push({ key: '__none__', project: null, tagIds: resolveTags(null).ids, note: p.note })
  return list
})

const unknownTags = computed(() => {
  const option = options.value[selected.value]
  return option ? resolveTags(option.project?.id ?? null).unknown : []
})

watch(query, () => (selected.value = 0))

async function reset() {
  query.value = ''
  selected.value = 0
  recentIds.value = (await call(commands.projectRecent(10)).catch(() => [])).map((p) => p.id)
  await nextTick()
  input.value?.focus()
}

onMounted(reset)
defineExpose({ reset })

async function start(option: Option | undefined) {
  if (!option) return
  try {
    await timer.start({ projectId: option.project?.id ?? null, tagIds: option.tagIds, note: option.note, billable: null })
    emit('done')
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

/** Project of the running or paused activity. */
const current = computed(() => {
  const id = timer.running?.projectId ?? timer.paused?.resumeEntry?.projectId ?? null
  return id ? projects.byId.get(id) : null
})

/** Pause, resume or stop from the palette, then close it. */
async function act(action: () => Promise<unknown>) {
  try {
    await action()
    emit('done')
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

function onKeydown(e: KeyboardEvent) {
  const count = options.value.length
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    selected.value = count ? (selected.value + 1) % count : 0
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    selected.value = count ? (selected.value - 1 + count) % count : 0
  } else if (e.key === 'Enter') {
    e.preventDefault()
    void start(options.value[selected.value])
  } else if (e.key === 'Escape') {
    e.preventDefault()
    emit('done')
  }
}
</script>

<template>
  <div class="flex h-full flex-col overflow-hidden" @keydown="onKeydown">
    <div class="flex items-center gap-2 border-b px-3">
      <Search class="size-4 shrink-0 text-muted-foreground" />
      <input
        ref="input"
        v-model="query"
        class="h-12 w-full bg-transparent text-base outline-none placeholder:text-muted-foreground"
        :placeholder="t('quickswitch.placeholder')"
        autocomplete="off"
        spellcheck="false"
      />
    </div>
    <!-- Current activity with its controls; Start when nothing runs. -->
    <div class="flex min-h-11 items-center gap-2 border-b px-3 py-1.5 text-sm">
      <template v-if="timer.running">
        <span class="size-2 shrink-0 rounded-full bg-green-500" />
        <ProjectDot :project="current" class="min-w-0" />
        <span v-if="timer.running.note" class="hidden truncate text-muted-foreground sm:inline">{{ timer.running.note }}</span>
        <span class="ml-auto font-mono tabular-nums">{{ formatClock(timer.elapsedMs) }}</span>
        <Button size="sm" variant="outline" class="h-7" @click="act(timer.pause)"><Pause /> {{ t('timer.pause') }}</Button>
        <Button size="sm" variant="outline" class="h-7" @click="act(timer.stop)"><Square class="fill-current" /> {{ t('timer.stop') }}</Button>
      </template>
      <template v-else-if="timer.paused">
        <span class="size-2 shrink-0 rounded-full bg-amber-500" />
        <span class="text-amber-600 dark:text-amber-400">{{ t('timer.paused') }}</span>
        <ProjectDot :project="current" class="min-w-0" />
        <Button size="sm" variant="outline" class="ml-auto h-7" @click="act(timer.resume)"><Play /> {{ t('timer.resume') }}</Button>
        <Button size="sm" variant="outline" class="h-7" @click="act(timer.stop)"><Square class="fill-current" /> {{ t('timer.stop') }}</Button>
      </template>
      <template v-else>
        <span class="size-2 shrink-0 rounded-full bg-muted-foreground/40" />
        <span class="text-muted-foreground">{{ t('timer.notTracking') }}</span>
        <Button size="sm" class="ml-auto h-7" :disabled="!options[selected]" @click="start(options[selected])">
          <Play /> {{ t('timer.start') }}
        </Button>
      </template>
    </div>
    <ul class="flex-1 overflow-y-auto p-1" role="listbox">
      <li
        v-for="(option, i) in options"
        :key="option.key"
        role="option"
        :aria-selected="i === selected"
        class="flex cursor-pointer items-center gap-3 rounded-md px-3 py-2 text-sm"
        :class="i === selected ? 'bg-accent text-accent-foreground' : ''"
        @mousemove="selected = i"
        @click="start(option)"
      >
        <Puzzle v-if="option.addon" class="size-3.5 shrink-0 text-muted-foreground" />
        <Play v-else class="size-3.5 shrink-0 text-muted-foreground" />
        <span v-if="option.addon" class="min-w-0 truncate">
          {{ option.addon.label }}
          <span v-if="option.addon.detail" class="text-muted-foreground"> · {{ option.addon.detail }}</span>
        </span>
        <ProjectDot v-else-if="option.project" :project="option.project" show-alias class="min-w-0" />
        <span v-else class="text-muted-foreground">{{ t('quickswitch.startWithoutProject') }}</span>
        <TagList :ids="option.tagIds" />
        <span v-if="option.note && !option.addon" class="ml-auto truncate text-muted-foreground">{{ option.note }}</span>
        <span
          v-if="timer.running && timer.running.projectId === (option.project?.id ?? null) && !option.note"
          class="ml-auto text-xs text-muted-foreground"
        >●</span>
      </li>
      <li v-if="unknownProject" class="px-3 py-2 text-sm text-destructive">
        {{ t('quickswitch.unknownProject', { name: unknownProject }) }}
      </li>
    </ul>
    <div class="flex items-center justify-between gap-2 border-t px-3 py-1.5 text-xs text-muted-foreground">
      <span v-if="unknownTags.length" class="text-destructive">{{ t('quickswitch.unknownTag', { name: unknownTags.join(', ') }) }}</span>
      <span class="ml-auto">{{ t('quickswitch.hint') }}</span>
    </div>
  </div>
</template>
