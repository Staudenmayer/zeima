import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { getTimeZone } from '@/lib/time'
import { useSettingsStore } from '@/stores/settings'

// Building an Intl.DateTimeFormat is expensive (especially in WebKit), and
// views re-render every second while the timer runs: build each one once.
const cache = new Map<string, Intl.DateTimeFormat>()
function formatter(locale: string, options: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  const key = `${locale}|${JSON.stringify(options)}`
  let f = cache.get(key)
  if (!f) {
    f = new Intl.DateTimeFormat(locale, options)
    cache.set(key, f)
  }
  return f
}

/**
 * Formatting for the UI language and the regional settings (date, time and
 * number format, time zone). With the "automatic" date format, dates are
 * written as the UI language does in the system's region (e.g. `de-AT`,
 * `en-GB`). Invalid dates render as '' instead of throwing: a throw during
 * render stops the page updating.
 */
export function useFormat() {
  const { locale } = useI18n()
  const settings = useSettingsStore()

  /** UI language + system region, if Intl knows the combination. */
  const intlLocale = computed(() => {
    const region = settings.systemRegion
    if (!region) return locale.value
    try {
      return Intl.getCanonicalLocales(`${locale.value}-${region}`)[0]
    } catch {
      return locale.value
    }
  })
  const automatic = computed(() => (settings.settings?.dateFormat ?? 'auto') === 'auto')
  const hourCycle = computed(() => (settings.hour12 ? 'h12' : 'h23'))

  const fmt = (ms: number, options: Intl.DateTimeFormatOptions) =>
    Number.isFinite(ms)
      ? formatter(intlLocale.value, { timeZone: getTimeZone(), ...options }).format(ms)
      : ''

  /** Numeric date in the chosen order, always with the year: `10.10.2026`. */
  function numeric(ms: number): string {
    if (!Number.isFinite(ms)) return ''
    const p: Record<string, string> = {}
    const f = formatter('en-US', { timeZone: getTimeZone(), year: 'numeric', month: '2-digit', day: '2-digit' })
    for (const part of f.formatToParts(ms)) p[part.type] = part.value
    switch (settings.dateOrder) {
      case 'ymd':
        return `${p.year}-${p.month}-${p.day}`
      case 'mdy':
        return `${p.month}/${p.day}/${p.year}`
      default:
        return `${p.day}.${p.month}.${p.year}`
    }
  }

  return {
    // 14:05 / 2:05 PM (no leading zero in 12-hour times).
    time: (ms: number) =>
      fmt(ms, { hour: settings.hour12 ? 'numeric' : '2-digit', minute: '2-digit', hourCycle: hourCycle.value }),
    weekday: (ms: number) => fmt(ms, { weekday: 'short' }),
    day: (ms: number) =>
      automatic.value
        ? fmt(ms, { weekday: 'long', day: 'numeric', month: 'long' })
        : `${fmt(ms, { weekday: 'long' })}, ${numeric(ms)}`,
    shortDate: (ms: number) => (automatic.value ? fmt(ms, { day: 'numeric', month: 'short' }) : numeric(ms)),
    longDate: (ms: number) =>
      automatic.value ? fmt(ms, { day: 'numeric', month: 'long', year: 'numeric' }) : numeric(ms),
    /** Always numeric, in the date order in effect (also for "automatic"). */
    numericDate: (ms: number) => numeric(ms),
    monthYear: (ms: number) => fmt(ms, { month: 'long', year: 'numeric' }),
    /** Decimal number with the separator in effect, e.g. `7,5`. */
    number: (n: number, maxDigits = 2) => settings.formatNumber(n, maxDigits),
  }
}
