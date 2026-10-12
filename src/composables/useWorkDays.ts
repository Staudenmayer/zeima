import { onScopeDispose, ref, watch, type Ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useIntervalFn } from '@vueuse/core'
import { commands, events, type WorkDay } from '@/bindings'
import { call } from '@/lib/ipc'
import { toDateInput } from '@/lib/time'
import { useTimerStore } from '@/stores/timer'

/**
 * Computed work days (target, net, balance delta, warnings) for a local
 * date range. Reloads on data changes and every minute while tracking.
 */
export function useWorkDays(range: Ref<{ from: number; to: number }>) {
  const days = ref<WorkDay[]>([])
  const holidayNames = ref(new Map<string, string>())
  const timer = useTimerStore()
  const { locale } = useI18n()

  async function reload() {
    // `to` is exclusive (start of the next day).
    const from = toDateInput(range.value.from)
    const to = toDateInput(range.value.to - 1)
    days.value = await call(commands.workDays(from, to)).catch(() => [])
    const years = [...new Set([from.slice(0, 4), to.slice(0, 4)])].map(Number)
    const lists = await Promise.all(years.map((y) => call(commands.holidayList(y, locale.value)).catch(() => [])))
    holidayNames.value = new Map(lists.flat().filter((h) => h.dayOff).map((h) => [h.date, h.name]))
  }

  watch([range, locale], reload, { immediate: true, deep: true })
  useIntervalFn(() => timer.running && void reload(), 60_000)
  const unlisten = events.dataChanged.listen((e) => {
    if (e.payload.kinds.some((k) => k === 'entry' || k === 'work')) void reload()
  })
  onScopeDispose(() => void unlisten.then((fn) => fn()))

  /** Day by `YYYY-MM-DD`. */
  const byDate = (date: string) => days.value.find((d) => d.date === date)

  /** Localized name of the public or company holiday on a date. */
  const holidayName = (date: string) => holidayNames.value.get(date)

  return { days, reload, byDate, holidayName }
}
