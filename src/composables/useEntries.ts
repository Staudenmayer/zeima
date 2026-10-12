import { onScopeDispose, ref, watch, type Ref } from 'vue'
import { commands, events, type Range, type Summary, type TimeEntry } from '@/bindings'
import { call } from '@/lib/ipc'

/** Entries and per-range summaries, reloaded when entries change. */
export function useEntries(range: Ref<Range>, summaryRanges?: Ref<Range[]>) {
  const entries = ref<TimeEntry[]>([])
  const summaries = ref<Summary[]>([])
  const loading = ref(false)

  async function reload() {
    loading.value = true
    try {
      const [e, s] = await Promise.all([
        call(commands.entryList(range.value)),
        summaryRanges ? call(commands.summaries(summaryRanges.value)) : Promise.resolve([]),
      ])
      entries.value = e
      summaries.value = s
    } finally {
      loading.value = false
    }
  }

  watch([range, () => summaryRanges?.value], reload, { immediate: true, deep: true })

  const unlisten = events.dataChanged.listen((e) => {
    if (e.payload.kinds.some((k) => k === 'entry' || k === 'project' || k === 'tag')) void reload()
  })
  onScopeDispose(() => void unlisten.then((fn) => fn()))

  return { entries, summaries, loading, reload }
}
