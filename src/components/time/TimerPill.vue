<script setup lang="ts">
import { computed, ref } from 'vue'
import { toast } from 'vue-sonner'
import { Pause, Play, Square, Zap } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import { errorMessage } from '@/lib/ipc'
import { formatClock } from '@/lib/time'
import { useProjectsStore } from '@/stores/projects'
import { useTimerStore } from '@/stores/timer'
import ProjectDot from './ProjectDot.vue'
import QuickSwitchPanel from './QuickSwitchPanel.vue'

const timer = useTimerStore()
const projects = useProjectsStore()
const switchOpen = ref(false)

const project = computed(() => {
  const id = timer.running?.projectId ?? timer.paused?.resumeEntry?.projectId ?? null
  return id ? projects.byId.get(id) : null
})

async function run(action: () => Promise<void>) {
  try {
    await action()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
</script>

<template>
  <div class="flex min-w-0 items-center gap-0.5 rounded-full border p-0.5 text-sm">
    <!-- The activity (or "not tracking") opens the quick switch. -->
    <button
      type="button"
      class="flex h-7 min-w-0 items-center gap-1 rounded-full px-2.5 transition-colors hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none cursor-pointer"
      :title="$t('timer.quickSwitch')"
      @click="switchOpen = true"
    >
      <template v-if="timer.running">
        <ProjectDot :project="project" class="max-w-48" />
        <span v-if="timer.running.note" class="hidden max-w-40 truncate text-muted-foreground lg:inline">{{ timer.running.note }}</span>
        <span class="ml-1 font-mono tabular-nums">{{ formatClock(timer.elapsedMs) }}</span>
      </template>
      <template v-else-if="timer.paused">
        <span class="text-amber-600 dark:text-amber-400">{{ $t('timer.paused') }}</span>
        <ProjectDot :project="project" class="max-w-48" />
      </template>
      <template v-else>
        <Zap class="size-3.5 text-muted-foreground" />
        <span class="text-muted-foreground">{{ $t('timer.notTracking') }}</span>
      </template>
    </button>
    <template v-if="timer.running">
      <Button size="icon" variant="ghost" class="size-7 rounded-full" :title="$t('timer.pause')" @click="run(timer.pause)">
        <Pause />
      </Button>
      <Button size="icon" variant="ghost" class="size-7 rounded-full" :title="$t('timer.stop')" @click="run(timer.stop)">
        <Square class="fill-current" />
      </Button>
    </template>
    <template v-else-if="timer.paused">
      <Button size="icon" variant="ghost" class="size-7 rounded-full" :title="$t('timer.resume')" @click="run(timer.resume)">
        <Play />
      </Button>
      <Button size="icon" variant="ghost" class="size-7 rounded-full" :title="$t('timer.stop')" @click="run(timer.stop)">
        <Square class="fill-current" />
      </Button>
    </template>
  </div>
  <Dialog v-model:open="switchOpen">
    <DialogContent :aria-describedby="undefined" class="h-96 gap-0 p-0 sm:max-w-xl" :show-close-button="false">
      <DialogTitle class="sr-only">{{ $t('timer.quickSwitch') }}</DialogTitle>
      <QuickSwitchPanel v-if="switchOpen" @done="switchOpen = false" />
    </DialogContent>
  </Dialog>
</template>
