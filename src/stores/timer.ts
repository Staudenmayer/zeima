import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import { useIntervalFn } from '@vueuse/core'
import { commands, type StartTimer, type TimerState } from '@/bindings'
import { call } from '@/lib/ipc'

export const useTimerStore = defineStore('timer', () => {
  const state = ref<TimerState>({ status: 'idle' })
  /** Ticks every second while tracking, for live elapsed time. */
  const now = ref(Date.now())

  const running = computed(() => (state.value.status === 'running' ? state.value.entry : null))
  const paused = computed(() => (state.value.status === 'paused' ? state.value : null))
  const elapsedMs = computed(() => (running.value ? now.value - running.value.startAt : 0))

  const ticker = useIntervalFn(() => (now.value = Date.now()), 1000, { immediate: false })
  watch(
    running,
    (entry) => {
      now.value = Date.now()
      if (entry) ticker.resume()
      else ticker.pause()
    },
    { immediate: true },
  )

  function set(next: TimerState) {
    state.value = next
  }

  async function load() {
    set(await call(commands.timerState()))
  }

  const start = async (start: StartTimer) => set(await call(commands.timerStart(start)))
  const stop = async () => set(await call(commands.timerStop()))
  const pause = async () => set(await call(commands.timerPause()))
  const resume = async () => set(await call(commands.timerResume()))

  /** Start the same work again (activity, project, tags, note). */
  async function continueEntry(entry: {
    activityId: string | null
    projectId: string | null
    tagIds: string[]
    note: string
    billable: boolean
  }) {
    await start({
      activityId: entry.activityId,
      projectId: entry.projectId,
      tagIds: entry.tagIds,
      note: entry.note,
      billable: entry.billable,
    })
  }

  return { state, now, running, paused, elapsedMs, set, load, start, stop, pause, resume, continueEntry }
})
