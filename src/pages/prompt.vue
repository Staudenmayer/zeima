<script setup lang="ts">
import { computed, onMounted, onScopeDispose, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Coffee, Moon, Play, Search, Sun, UtensilsCrossed, X } from '@lucide/vue'
import { commands, events, type AwayItem, type AwayResolution, type RecentActivity } from '@/bindings'
import { Button } from '@/components/ui/button'
import ActivityLabel from '@/components/time/ActivityLabel.vue'
import ProjectDot from '@/components/time/ProjectDot.vue'
import ProjectSelect from '@/components/time/ProjectSelect.vue'
import { useFormat } from '@/composables/useFormat'
import { call, errorMessage } from '@/lib/ipc'
import { formatDuration } from '@/lib/time'
import { useActivitiesStore } from '@/stores/activities'
import { useProjectsStore } from '@/stores/projects'
import { useTimerStore } from '@/stores/timer'

// Runs in its own small window (see src-tauri/src/windows.rs).
definePage({ meta: { layout: 'bare' } })

const { t } = useI18n()
const format = useFormat()
const activities = useActivitiesStore()
const projects = useProjectsStore()
const timer = useTimerStore()

const away = ref<AwayItem[]>([])
const morning = ref(false)
const recent = ref<RecentActivity[]>([])
const assigning = ref<string | null>(null)
const assignProject = ref<string | null>(null)

async function load() {
  const state = await call(commands.promptState()).catch(() => null)
  away.value = state?.away ?? []
  morning.value = state?.morning ?? false
  if (morning.value) recent.value = await call(commands.activityRecent(4)).catch(() => [])
  if (away.value.length === 0 && !morning.value) void commands.promptHide()
}

onMounted(load)
const unlisten = events.promptsChanged.listen(() => void load())
onScopeDispose(() => void unlisten.then((fn) => fn()))

function projectName(item: AwayItem) {
  const id = item.entry?.projectId
  return (id && projects.byId.get(id)?.name) || t('common.noProject')
}

function text(item: AwayItem) {
  const p = item.pause
  const end = p.endAt ?? p.startAt
  const params = {
    from: format.time(p.startAt),
    to: format.time(end),
    duration: `${formatDuration(end - p.startAt)} h`,
    project: projectName(item),
  }
  if (p.kind === 'auto_night') return t('prompt.night', params)
  if (p.kind === 'auto_lunch') return t('prompt.lunch', params)
  return p.applied ? t('prompt.awayExcluded', params) : t('prompt.awayAsk', params)
}

const icon = (item: AwayItem) =>
  item.pause.kind === 'auto_night' ? Moon : item.pause.kind === 'auto_lunch' ? UtensilsCrossed : Coffee

async function resolve(item: AwayItem, resolution: AwayResolution) {
  try {
    await call(commands.awayResolve(item.pause.id, resolution))
    assigning.value = null
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

async function startRecent(r: RecentActivity) {
  try {
    await timer.start({ activityId: r.activityId, projectId: r.projectId, tagIds: r.tagIds, note: '', billable: null })
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

const empty = computed(() => away.value.length === 0 && !morning.value)
</script>

<template>
  <div class="window-panel flex h-full flex-col gap-2 overflow-y-auto rounded-xl border bg-popover p-3 text-sm text-popover-foreground">
    <div class="flex items-center justify-between">
      <span class="font-semibold">{{ $t('prompt.title') }}</span>
      <Button size="icon" variant="ghost" class="size-7" @click="commands.promptHide()"><X /></Button>
    </div>

    <div v-if="morning" class="grid gap-2 rounded-lg border p-3">
      <div class="flex items-center gap-2 font-medium"><Sun class="size-4 text-amber-500" /> {{ $t('prompt.morning') }}</div>
      <div class="flex flex-wrap gap-1.5">
        <Button
          v-for="r in recent"
          :key="`${r.activityId}|${r.projectId}|${r.tagIds.join()}`"
          size="sm"
          variant="outline"
          class="max-w-full"
          @click="startRecent(r)"
        >
          <Play /> <ActivityLabel :activity="activities.byId.get(r.activityId)" class="min-w-0" />
          <ProjectDot v-if="r.projectId" :project="projects.byId.get(r.projectId)" class="min-w-0 text-muted-foreground" />
        </Button>
        <Button size="sm" variant="outline" @click="commands.quickswitchShow()"><Search /> {{ $t('prompt.choose') }}</Button>
      </div>
      <Button size="sm" variant="ghost" class="justify-self-end" @click="commands.morningDismiss()">{{ $t('prompt.dismiss') }}</Button>
    </div>

    <div v-for="item in away" :key="item.pause.id" class="grid gap-2 rounded-lg border p-3">
      <div class="flex gap-2">
        <component :is="icon(item)" class="mt-0.5 size-4 shrink-0 text-muted-foreground" />
        <p>{{ text(item) }}</p>
      </div>
      <div v-if="assigning === item.pause.id" class="grid gap-2">
        <ProjectSelect v-model="assignProject" />
        <div class="flex justify-end gap-2">
          <Button size="sm" variant="ghost" @click="assigning = null">{{ $t('common.cancel') }}</Button>
          <Button
            size="sm"
            :disabled="!assignProject"
            @click="resolve(item, { kind: 'assign', projectId: assignProject! })"
          >{{ $t('common.save') }}</Button>
        </div>
      </div>
      <div v-else class="flex flex-wrap justify-end gap-1.5">
        <Button size="sm" variant="ghost" @click="assigning = item.pause.id; assignProject = null">{{ $t('prompt.assign') }}</Button>
        <Button size="sm" variant="outline" @click="resolve(item, { kind: 'keep' })">{{ $t('prompt.wasWork') }}</Button>
        <Button size="sm" @click="resolve(item, { kind: 'exclude' })">
          {{ item.pause.kind === 'auto_night' ? $t('prompt.ok') : item.pause.applied ? $t('prompt.wasBreak') : $t('prompt.removeTime') }}
        </Button>
      </div>
    </div>

    <p v-if="empty" class="py-6 text-center text-muted-foreground">–</p>
  </div>
</template>
