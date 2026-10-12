<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { History, Pause, Play, Plus, Puzzle, Search, Square, X } from '@lucide/vue'
import { commands, type Activity, type EntryInput, type Project, type RecentActivity, type Tag } from '@/bindings'
import { Button } from '@/components/ui/button'
import { nextColor } from '@/lib/colors'
import { call, errorMessage } from '@/lib/ipc'
import { formatClock } from '@/lib/time'
import { isExact, parseQuery, rank, removeTokens, resolve, type Token } from '@/lib/quickswitch'
import { useActivitiesStore } from '@/stores/activities'
import { useProjectsStore } from '@/stores/projects'
import { useTagsStore } from '@/stores/tags'
import { useTimerStore } from '@/stores/timer'
import ActivityLabel from './ActivityLabel.vue'
import ProjectDot from './ProjectDot.vue'
import TagList from './TagList.vue'

// Two modes:
// - Something runs: the first line is the running entry. The input edits its
//   note, `@` / `#` change its project and tags; only pause and stop are
//   offered, no switching to another activity.
// - Otherwise: the input searches activities to start, with `@` / `#` to pick
//   project and tags first.

const emit = defineEmits<{ done: [] }>()

const { t } = useI18n()
const activities = useActivitiesStore()
const projects = useProjectsStore()
const tags = useTagsStore()
const timer = useTimerStore()

const RECENT_SHOWN = 5

const query = ref('')
const selected = ref(0)
const input = ref<HTMLInputElement | null>(null)
/** Project and tags picked for the next start, shown as chips. */
const project = ref<Project | null>(null)
const tagIds = ref<string[]>([])
/** Note taken over from an addon suggestion. */
const note = ref('')
const recent = ref<RecentActivity[]>([])
const recentProjectIds = ref<string[]>([])
/** `@x` / `#x` words already in the running entry's note: note text, not picks. */
const noteWords = ref(new Set<string>())

type Option =
  | { kind: 'recent'; key: string; recent: RecentActivity }
  | { kind: 'activity'; key: string; activity: Activity }
  | { kind: 'project'; key: string; project: Project }
  | { kind: 'tag'; key: string; tag: Tag }
  | { kind: 'create'; key: string; what: 'activity' | 'project' | 'tag'; name: string }
  | { kind: 'addon'; key: string; label: string; detail: string | null; activityId: string | null; projectId: string | null; tagIds: string[]; note: string }

interface Section {
  title: string | null
  options: Option[]
}

const running = computed(() => timer.running)
const editing = computed(() => running.value !== null)

const parsed = computed(() => parseQuery(query.value))
const mode = computed(() => parsed.value.active?.kind ?? 'activity')
const term = computed(() => parsed.value.active?.term ?? parsed.value.text)

/** Chips: the running entry's values while editing, else the picks. */
const chipProject = computed<Project | null>(() => {
  if (!running.value) return project.value
  return running.value.projectId ? (projects.byId.get(running.value.projectId) ?? null) : null
})
const chipTagIds = computed(() => (running.value ? running.value.tagIds : tagIds.value))

// Addon suggestions, fetched shortly after typing stops (only for starting).
const addonOptions = ref<Option[]>([])
let suggestTimer: ReturnType<typeof setTimeout> | undefined
watch(() => parsed.value.text, (q) => {
  clearTimeout(suggestTimer)
  if (editing.value) {
    addonOptions.value = []
    return
  }
  suggestTimer = setTimeout(async () => {
    const list = await call(commands.addonSuggest(q)).catch(() => [])
    if (q !== parsed.value.text || editing.value) return
    addonOptions.value = list.map((s, i) => ({
      kind: 'addon',
      key: `addon:${s.addonId}:${i}`,
      label: s.label,
      detail: s.detail,
      activityId: s.activityId,
      projectId: s.projectId,
      tagIds: s.tagIds,
      note: s.note,
    }))
  }, 150)
}, { immediate: true })

/** Recently used first, then the rest in their usual order. */
function recentFirst<T extends { id: string }>(items: T[], recentIds: string[]): T[] {
  const index = new Map(recentIds.map((id, i) => [id, i]))
  return [...items].sort((a, b) => (index.get(a.id) ?? Infinity) - (index.get(b.id) ?? Infinity))
}

function usable(r: RecentActivity): boolean {
  const activity = activities.byId.get(r.activityId)
  const p = r.projectId ? projects.byId.get(r.projectId) : null
  return !!activity && !activity.archived && p !== undefined && p?.status !== 'archived'
}

const recentActivityIds = computed(() => [...new Set(recent.value.map((r) => r.activityId))])

function activityOptions(q: string): Option[] {
  const list: Option[] = rank(recentFirst(activities.active, recentActivityIds.value), q).map((activity) => ({
    kind: 'activity',
    key: activity.id,
    activity,
  }))
  if (q.trim() && !activities.all.some((a) => isExact(a, q))) {
    list.push({ kind: 'create', key: '__create_activity__', what: 'activity', name: q.trim() })
  }
  return list
}

const sections = computed<Section[]>(() => {
  const q = term.value
  const projectId = chipProject.value?.id ?? null
  if (mode.value === 'project') {
    const ordered = recentFirst(projects.active, recentProjectIds.value)
    const list: Option[] = rank(ordered, q).map((p) => ({ kind: 'project', key: p.id, project: p }))
    if (q && !projects.all.some((p) => isExact(p, q))) {
      list.push({ kind: 'create', key: '__create_project__', what: 'project', name: q })
    }
    return [{ title: t('quickswitch.projects'), options: list }]
  }
  if (mode.value === 'tag') {
    const available = tags.usableFor(projectId).filter((tag) => !chipTagIds.value.includes(tag.id))
    const list: Option[] = rank(available, q).map((tag) => ({ kind: 'tag', key: tag.id, tag }))
    if (q && !tags.usableFor(projectId).some((tag) => isExact(tag, q))) {
      list.push({ kind: 'create', key: '__create_tag__', what: 'tag', name: q })
    }
    return [{ title: t('quickswitch.tags'), options: list }]
  }
  // While editing, plain text is the note: nothing to pick.
  if (editing.value) return []
  // Recent combinations restart in one step, but only when no project or
  // tag was picked yet (they would silently be replaced).
  const out: Section[] = []
  const picked = project.value !== null || tagIds.value.length > 0
  if (!picked) {
    const matching = recent.value
      .filter(usable)
      .filter((r) => !q.trim() || rank([activities.byId.get(r.activityId)!], q).length > 0)
      .slice(0, q.trim() ? 3 : RECENT_SHOWN)
      .map<Option>((r, i) => ({ kind: 'recent', key: `recent:${i}`, recent: r }))
    if (matching.length) out.push({ title: t('quickswitch.recent'), options: matching })
  }
  out.push({ title: t('quickswitch.activities'), options: activityOptions(q) })
  if (addonOptions.value.length) out.push({ title: t('quickswitch.addons'), options: addonOptions.value })
  return out.filter((s) => s.options.length > 0)
})

const options = computed(() => sections.value.flatMap((s) => s.options))
/** Index of each option in `options`, for keyboard selection. */
const indexOf = computed(() => new Map(options.value.map((o, i) => [o.key, i])))

watch(query, () => (selected.value = 0))

// A finished `@token` / `#token` (followed by a space) becomes a chip when
// it matches; unknown ones stay as text and are reported below.
watch(() => parsed.value.tokens, async (tokens) => {
  const done: Token[] = []
  for (const token of tokens) {
    const raw = query.value.slice(token.start, token.end)
    if (editing.value && noteWords.value.has(raw)) continue
    if (token.kind === 'project') {
      const p = resolve(projects.active, token.term)
      if (p) {
        done.push(token)
        await setProject(p)
      }
    } else {
      const tag = resolve(tags.usableFor(chipProject.value?.id ?? null), token.term)
      if (tag) {
        done.push(token)
        await addTag(tag)
      }
    }
  }
  if (done.length) query.value = removeTokens(query.value, done)
})

// While editing, unknown tokens are simply note text.
const unknownTokens = computed(() =>
  editing.value ? [] : parsed.value.tokens.map((tok) => (tok.kind === 'project' ? '@' : '#') + tok.term),
)

/** Saves changes to the running entry; the timer event refreshes the view. */
async function updateRunning(patch: { projectId?: string | null; tagIds?: string[]; note?: string }) {
  const e = running.value
  if (!e) return
  const projectId = patch.projectId !== undefined ? patch.projectId : e.projectId
  const entry: EntryInput = {
    activityId: e.activityId,
    projectId,
    startAt: e.startAt,
    endAt: null,
    tz: e.tz,
    note: patch.note ?? e.note,
    billable: e.billable,
    tagIds: patch.tagIds ?? e.tagIds,
    // A new project brings its own cost center.
    costCenterId: projectId === e.projectId ? e.costCenterId : null,
  }
  try {
    await call(commands.entryUpdate(e.id, entry))
    await timer.load()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

/** Tags still allowed with `projectId` (scoped tags of other projects drop out). */
function tagsFor(ids: string[], projectId: string | null): string[] {
  return ids.filter((id) => {
    const tag = tags.byId.get(id)
    return tag && (tag.projectId === null || tag.projectId === projectId)
  })
}

async function setProject(p: Project | null) {
  const ids = tagsFor(chipTagIds.value, p?.id ?? null)
  if (editing.value) return updateRunning({ projectId: p?.id ?? null, tagIds: ids })
  project.value = p
  tagIds.value = ids
}

async function addTag(tag: Tag) {
  // One tag per group.
  const others = chipTagIds.value.filter((id) => {
    const other = tags.byId.get(id)
    return !(tag.groupName && other?.groupName === tag.groupName)
  })
  const ids = [...new Set([...others, tag.id])]
  if (editing.value) return updateRunning({ tagIds: ids })
  tagIds.value = ids
}

async function removeTag(id: string) {
  const ids = chipTagIds.value.filter((t) => t !== id)
  if (editing.value) return updateRunning({ tagIds: ids })
  tagIds.value = ids
}

/** Drops the token being typed, after its suggestion was picked. */
function consumeActive() {
  const active = parsed.value.active
  if (active) query.value = removeTokens(query.value, [active])
  void nextTick(() => input.value?.focus())
}

async function reset() {
  project.value = null
  tagIds.value = []
  note.value = ''
  // Editing starts from the current note.
  query.value = timer.running?.note ?? ''
  noteWords.value = new Set(query.value.split(/\s+/).filter((w) => /^[@#]./.test(w)))
  selected.value = 0
  const [recentList, recentProjects] = await Promise.all([
    call(commands.activityRecent(20)).catch(() => []),
    call(commands.projectRecent(10)).catch(() => []),
  ])
  recent.value = recentList
  recentProjectIds.value = recentProjects.map((p) => p.id)
  await nextTick()
  input.value?.focus()
  input.value?.select()
}

onMounted(reset)
defineExpose({ reset })

// Tracking started (here or elsewhere) or stopped: switch modes.
watch(editing, () => void reset())

/**
 * Starts tracking and stays open: the panel switches to editing the new
 * entry (see the `editing` watcher), so note and tags can follow right away.
 */
async function start(activityId: string, projectId: string | null, tagList: string[], text: string) {
  try {
    await timer.start({ activityId, projectId, tagIds: tagList, note: text, billable: null })
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

/** Enter while editing: keep the typed note and close. */
async function saveNote() {
  const text = query.value.trim()
  if (running.value && text !== running.value.note) await updateRunning({ note: text })
  emit('done')
}

async function create(what: 'activity' | 'project' | 'tag', name: string) {
  try {
    if (what === 'activity') {
      const activity = await activities.create({ name, color: nextColor(activities.all.map((a) => a.color)), archived: false })
      await activities.load()
      await start(activity.id, project.value?.id ?? null, tagIds.value, note.value)
    } else if (what === 'project') {
      const p = await projects.create({
        name,
        alias: null,
        color: nextColor(projects.all.map((x) => x.color)),
        parentId: null,
        client: null,
        budgetMinutes: null,
        budgetPeriod: 'total',
        budgetWarnPct: 80,
        billableDefault: false,
        status: 'active',
        costCenterId: null,
      })
      await projects.load()
      consumeActive()
      await setProject(p)
      toast.success(t('quickswitch.created', { name }))
    } else {
      const tag = await tags.create({ name, color: nextColor(tags.all.map((x) => x.color)), projectId: null, groupName: null })
      await tags.load()
      consumeActive()
      await addTag(tag)
      toast.success(t('quickswitch.created', { name }))
    }
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

async function choose(option: Option | undefined) {
  if (!option) return
  switch (option.kind) {
    case 'recent':
      return start(option.recent.activityId, option.recent.projectId, option.recent.tagIds, note.value)
    case 'activity':
      return start(option.activity.id, project.value?.id ?? null, tagIds.value, note.value)
    case 'project':
      consumeActive()
      return setProject(option.project)
    case 'tag':
      consumeActive()
      return addTag(option.tag)
    case 'create':
      return create(option.what, option.name)
    case 'addon': {
      if (option.activityId) return start(option.activityId, option.projectId, option.tagIds, option.note)
      // No activity from the addon: keep its values and let the user pick one.
      project.value = option.projectId ? (projects.byId.get(option.projectId) ?? null) : null
      tagIds.value = tagsFor(option.tagIds, project.value?.id ?? null)
      note.value = option.note
      query.value = ''
      toast.info(t('quickswitch.pickActivity'))
      return
    }
  }
}

/** The paused entry, shown with resume and stop. */
const pausedEntry = computed(() => timer.paused?.resumeEntry ?? null)
const activityOf = (id: string | null | undefined) => (id ? activities.byId.get(id) : null)
const projectOf = (id: string | null | undefined) => (id ? projects.byId.get(id) : null)

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
  } else if (e.key === 'Enter' || (e.key === 'Tab' && mode.value !== 'activity')) {
    e.preventDefault()
    if (editing.value && mode.value === 'activity') void saveNote()
    else void choose(options.value[selected.value])
  } else if (
    // Not while editing: holding Backspace to clear the note must not
    // remove the running entry's tags.
    e.key === 'Backspace' && !editing.value && query.value === '' && (tagIds.value.length || project.value)
  ) {
    e.preventDefault()
    if (tagIds.value.length) tagIds.value = tagIds.value.slice(0, -1)
    else void setProject(null)
  } else if (e.key === 'Escape') {
    e.preventDefault()
    emit('done')
  }
}

const startable = computed(() => {
  const o = options.value[selected.value]
  return o && (o.kind === 'activity' || o.kind === 'recent' || (o.kind === 'create' && o.what === 'activity'))
})

const placeholder = computed(() => {
  if (editing.value) return t('quickswitch.placeholderNote')
  return project.value || tagIds.value.length ? t('quickswitch.placeholderActivity') : t('quickswitch.placeholder')
})

const hint = computed(() => {
  if (mode.value !== 'activity') return t('quickswitch.hintPick')
  return editing.value ? t('quickswitch.hintEdit') : t('quickswitch.hint')
})
</script>

<template>
  <div class="flex h-full flex-col overflow-hidden" @keydown="onKeydown">
    <!-- First line: the running entry (editable), or the search with Start. -->
    <div class="flex items-center gap-2 border-b px-3 py-1.5">
      <!-- Entry and input may wrap; the controls on the right never move. -->
      <div class="flex min-w-0 flex-1 flex-wrap items-center gap-1.5">
        <template v-if="running">
          <span class="size-2 shrink-0 rounded-full bg-green-500" :title="t('timesheet.running')" />
          <ActivityLabel :activity="activityOf(running.activityId)" class="min-w-0 font-medium" />
        </template>
        <Search v-else class="size-4 shrink-0 text-muted-foreground" />
        <span
          v-if="chipProject"
          class="inline-flex max-w-48 items-center gap-1 rounded-md border bg-muted/50 py-0.5 pr-1 pl-2 text-xs"
        >
          <ProjectDot :project="chipProject" class="min-w-0" />
          <button type="button" class="rounded text-muted-foreground hover:text-foreground" :aria-label="t('quickswitch.removeProject')" @click="setProject(null)">
            <X class="size-3" />
          </button>
        </span>
        <span
          v-for="id in chipTagIds"
          :key="id"
          class="inline-flex items-center gap-0.5 rounded-md border bg-muted/50 py-0.5 pr-1 pl-1 text-xs"
        >
          <TagList :ids="[id]" />
          <button type="button" class="rounded text-muted-foreground hover:text-foreground" :aria-label="t('quickswitch.removeTag')" @click="removeTag(id)">
            <X class="size-3" />
          </button>
        </span>
        <input
          ref="input"
          v-model="query"
          class="h-9 min-w-32 flex-1 bg-transparent text-base outline-none placeholder:text-muted-foreground"
          :placeholder="placeholder"
          :aria-label="editing ? t('entry.note') : undefined"
          autocomplete="off"
          spellcheck="false"
          role="combobox"
          aria-autocomplete="list"
          :aria-expanded="options.length > 0"
        />
      </div>
      <div class="flex shrink-0 items-center gap-1">
        <template v-if="running">
          <span class="mr-1 font-mono text-sm tabular-nums">{{ formatClock(timer.elapsedMs) }}</span>
          <Button size="icon" variant="outline" class="size-8" :title="t('timer.pause')" :aria-label="t('timer.pause')" @click="act(timer.pause)">
            <Pause />
          </Button>
          <Button size="icon" variant="outline" class="size-8" :title="t('timer.stop')" :aria-label="t('timer.stop')" @click="act(timer.stop)">
            <Square class="fill-current" />
          </Button>
        </template>
        <Button
          v-else
          size="icon"
          class="size-8"
          :title="t('timer.start')"
          :aria-label="t('timer.start')"
          :disabled="!startable"
          @click="choose(options[selected])"
        >
          <Play class="fill-current" />
        </Button>
      </div>
    </div>
    <!-- Paused: nothing runs, so starting is possible; resume or stop here. -->
    <div v-if="timer.paused" class="flex min-h-10 items-center gap-2 border-b px-3 py-1 text-sm">
      <span class="size-2 shrink-0 rounded-full bg-amber-500" />
      <span class="text-amber-600 dark:text-amber-400">{{ t('timer.paused') }}</span>
      <ActivityLabel v-if="pausedEntry" :activity="activityOf(pausedEntry.activityId)" class="min-w-0" />
      <ProjectDot v-if="pausedEntry?.projectId" :project="projectOf(pausedEntry.projectId)" class="hidden min-w-0 text-muted-foreground sm:inline-flex" />
      <div class="ml-auto flex shrink-0 items-center gap-1">
        <Button size="icon" variant="outline" class="size-8" :title="t('timer.resume')" :aria-label="t('timer.resume')" @click="act(timer.resume)">
          <Play />
        </Button>
        <Button size="icon" variant="outline" class="size-8" :title="t('timer.stop')" :aria-label="t('timer.stop')" @click="act(timer.stop)">
          <Square class="fill-current" />
        </Button>
      </div>
    </div>
    <div class="flex-1 overflow-y-auto p-1" role="listbox">
      <p v-if="editing && mode === 'activity'" class="px-3 py-3 text-sm text-muted-foreground">
        {{ t('quickswitch.editHelp') }}
      </p>
      <p v-else-if="mode === 'activity' && activities.active.length === 0 && !term.trim()" class="px-3 py-3 text-sm text-muted-foreground">
        {{ t('quickswitch.noActivities') }}
      </p>
      <p v-else-if="options.length === 0" class="px-3 py-3 text-sm text-muted-foreground">
        {{ mode === 'project' ? t('quickswitch.noProjects') : mode === 'tag' ? t('quickswitch.noTags') : t('quickswitch.noMatch') }}
      </p>
      <template v-for="section in sections" :key="section.title ?? ''">
        <div v-if="section.title" class="px-3 pt-2 pb-1 text-xs font-medium text-muted-foreground">{{ section.title }}</div>
        <div
          v-for="option in section.options"
          :key="option.key"
          role="option"
          :aria-selected="indexOf.get(option.key) === selected"
          class="flex cursor-pointer items-center gap-3 rounded-md px-3 py-2 text-sm"
          :class="indexOf.get(option.key) === selected ? 'bg-accent text-accent-foreground' : ''"
          @mousemove="selected = indexOf.get(option.key) ?? 0"
          @click="choose(option)"
        >
          <template v-if="option.kind === 'recent'">
            <History class="size-3.5 shrink-0 text-muted-foreground" />
            <ActivityLabel :activity="activityOf(option.recent.activityId)" class="min-w-0" />
            <ProjectDot
              v-if="option.recent.projectId"
              :project="projectOf(option.recent.projectId)"
              show-alias
              class="min-w-0 text-muted-foreground"
            />
            <TagList :ids="option.recent.tagIds" />
          </template>
          <template v-else-if="option.kind === 'activity'">
            <Play class="size-3.5 shrink-0 text-muted-foreground" />
            <ActivityLabel :activity="option.activity" class="min-w-0" />
            <span v-if="project || tagIds.length" class="ml-auto truncate text-xs text-muted-foreground">
              {{ t('quickswitch.withPicked') }}
            </span>
          </template>
          <template v-else-if="option.kind === 'project'">
            <ProjectDot :project="option.project" show-alias class="min-w-0" />
          </template>
          <template v-else-if="option.kind === 'tag'">
            <TagList :ids="[option.tag.id]" />
            <span v-if="option.tag.groupName" class="text-xs text-muted-foreground">{{ option.tag.groupName }}</span>
          </template>
          <template v-else-if="option.kind === 'create'">
            <Plus class="size-3.5 shrink-0 text-muted-foreground" />
            <span class="min-w-0 truncate">
              {{ t(`quickswitch.create.${option.what}`, { name: option.name }) }}
            </span>
            <span v-if="option.what === 'activity'" class="ml-auto shrink-0 text-xs text-muted-foreground">{{ t('quickswitch.createAndStart') }}</span>
          </template>
          <template v-else-if="option.kind === 'addon'">
            <Puzzle class="size-3.5 shrink-0 text-muted-foreground" />
            <span class="min-w-0 truncate">
              {{ option.label }}
              <span v-if="option.detail" class="text-muted-foreground"> · {{ option.detail }}</span>
            </span>
          </template>
        </div>
      </template>
    </div>
    <div class="flex items-center justify-between gap-2 border-t px-3 py-1.5 text-xs text-muted-foreground">
      <span v-if="unknownTokens.length" class="truncate text-destructive">{{ t('quickswitch.unknown', { names: unknownTokens.join(' ') }) }}</span>
      <span v-else-if="note" class="truncate">{{ note }}</span>
      <span class="ml-auto shrink-0">{{ hint }}</span>
    </div>
  </div>
</template>
