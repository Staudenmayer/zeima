import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { commands, type AppInfo, type AppSettings } from '@/bindings'
import { call } from '@/lib/ipc'
import { formatDuration as format, setTimeZone, setWeekStart, type DateOrder } from '@/lib/time'

const WEEK_START = { sunday: 0, monday: 1, saturday: 6, auto: 1 } as const

export const useSettingsStore = defineStore('settings', () => {
  const settings = ref<AppSettings | null>(null)
  const info = ref<AppInfo | null>(null)

  const durationFormat = computed(() => settings.value?.durationFormat ?? 'hm')

  // Formats in effect; the backend resolves "automatic" from the system locale.
  const decimalComma = computed(() => info.value?.formats.number === 'comma')
  const hour12 = computed(() => info.value?.formats.time === 'h12')
  const dateOrder = computed<DateOrder>(() => {
    const d = info.value?.formats.date
    return d === 'mdy' || d === 'ymd' ? d : 'dmy'
  })
  /** Region of the system locale (`AT` in `de-AT`), for Intl formatting. */
  const systemRegion = computed(() => /[-_]([A-Z]{2}|\d{3})\b/.exec(info.value?.systemLocale ?? '')?.[1] ?? null)

  /**
   * Loads settings and app info and applies zone and week start to the
   * date helpers. Returns whether either changed since the last load.
   */
  async function load(): Promise<boolean> {
    const [s, i] = await Promise.all([call(commands.settingsGet()), call(commands.appInfo())])
    const before = `${info.value?.defaultTz}|${info.value?.formats.weekStart}`
    const first = settings.value === null
    settings.value = s
    info.value = i
    setTimeZone(i.defaultTz)
    setWeekStart(WEEK_START[i.formats.weekStart])
    return !first && before !== `${i.defaultTz}|${i.formats.weekStart}`
  }

  async function save(next: AppSettings) {
    settings.value = await call(commands.settingsSet(next))
  }

  /** `7.5` / `7,5`, at most `maxDigits` decimals, no grouping. */
  function formatNumber(n: number, maxDigits = 2) {
    const s = String(+n.toFixed(maxDigits))
    return decimalComma.value ? s.replace('.', ',') : s
  }

  /** Duration in the user's preferred format, e.g. `7:42` or `7.70` / `7,70`. */
  function formatDuration(ms: number) {
    const s = format(ms, durationFormat.value)
    return durationFormat.value === 'decimal' && decimalComma.value ? s.replace('.', ',') : s
  }

  /** Signed duration: `+1:30` / `−0,75`. */
  function formatSigned(ms: number) {
    return (ms < 0 ? '−' : '+') + formatDuration(Math.abs(ms))
  }

  return {
    settings,
    info,
    durationFormat,
    decimalComma,
    hour12,
    dateOrder,
    systemRegion,
    load,
    save,
    formatNumber,
    formatDuration,
    formatSigned,
  }
})
