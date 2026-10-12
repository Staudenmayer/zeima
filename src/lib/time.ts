// Date and duration helpers. Times are unix milliseconds (UTC); day and
// week boundaries are computed in the configured time zone (Settings →
// Region), not necessarily the browser's.

export const MINUTE = 60_000
export const HOUR = 60 * MINUTE

export type DurationFormat = 'hm' | 'decimal'

/** IANA zone; `undefined` = the runtime's local zone. */
let zone: string | undefined
/** 0 = Sunday … 6 = Saturday. */
let weekStartsOn = 1

/** Sets the zone used by all helpers below (from the backend's effective zone). */
export function setTimeZone(tz: string | undefined) {
  zone = tz
  partsFormat = undefined
}

export function getTimeZone(): string | undefined {
  return zone
}

export function setWeekStart(day: number) {
  weekStartsOn = day
}

export function getWeekStart(): number {
  return weekStartsOn
}

interface Parts {
  year: number
  month: number
  day: number
  hour: number
  minute: number
  second: number
  /** 0 = Sunday. */
  weekday: number
}

let partsFormat: Intl.DateTimeFormat | undefined
const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']

/** Wall-clock fields of `ms` in the configured zone. */
function parts(ms: number): Parts {
  partsFormat ??= new Intl.DateTimeFormat('en-US', {
    timeZone: zone,
    hourCycle: 'h23',
    year: 'numeric',
    month: 'numeric',
    day: 'numeric',
    hour: 'numeric',
    minute: 'numeric',
    second: 'numeric',
    weekday: 'short',
  })
  const out: Record<string, string> = {}
  for (const p of partsFormat.formatToParts(ms)) out[p.type] = p.value
  return {
    year: Number(out.year),
    month: Number(out.month),
    day: Number(out.day),
    hour: Number(out.hour) % 24,
    minute: Number(out.minute),
    second: Number(out.second),
    weekday: WEEKDAYS.indexOf(out.weekday),
  }
}

/** Zone offset at `ms` in ms (local − UTC), to the second. */
function offsetAt(ms: number): number {
  const p = parts(ms)
  const asUtc = Date.UTC(p.year, p.month - 1, p.day, p.hour, p.minute, p.second)
  return asUtc - Math.floor(ms / 1000) * 1000
}

/**
 * Unix ms of a wall-clock time in the configured zone. Fields may
 * overflow (day 32 → next month). A time inside a DST gap resolves to the
 * instant after the gap.
 */
function fromLocal(year: number, month: number, day: number, hour = 0, minute = 0, second = 0, millis = 0): number {
  const asUtc = Date.UTC(year, month - 1, day, hour, minute, second, millis)
  const first = asUtc - offsetAt(asUtc)
  const second_ = asUtc - offsetAt(first)
  return first === second_ ? first : Math.max(first, second_)
}

/** Day of the week of `ms`, 0 = Sunday. */
export function weekday(ms: number): number {
  return parts(ms).weekday
}

export function startOfDay(ms: number): number {
  const p = parts(ms)
  return fromLocal(p.year, p.month, p.day)
}

/** Adds calendar days, keeping the wall-clock time (DST-safe, unlike adding 24 h). */
export function addDays(ms: number, days: number): number {
  const p = parts(ms)
  return fromLocal(p.year, p.month, p.day + days, p.hour, p.minute, p.second, ((ms % 1000) + 1000) % 1000)
}

/** First day (00:00) of the week containing `ms`, per the week start setting. */
export function startOfWeek(ms: number): number {
  const day = startOfDay(ms)
  const back = (weekday(day) - weekStartsOn + 7) % 7
  return addDays(day, -back)
}

export interface DayRange {
  from: number
  to: number
}

export function dayRange(ms: number): DayRange {
  const from = startOfDay(ms)
  return { from, to: addDays(from, 1) }
}

/** The 7 days of the week containing `ms`, starting on the week start day. */
export function weekDays(ms: number): DayRange[] {
  const first = startOfWeek(ms)
  return Array.from({ length: 7 }, (_, i) => dayRange(addDays(first, i)))
}

/** `7:42` or `7.70` (hours). */
export function formatDuration(ms: number, format: DurationFormat = 'hm'): string {
  const minutes = Math.max(0, Math.floor(ms / MINUTE))
  if (format === 'decimal') {
    return (minutes / 60).toFixed(2)
  }
  return `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, '0')}`
}

/** Running timer display: `1:02:03`. */
export function formatClock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  return `${h}:${String(m).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`
}

const pad = (n: number) => String(n).padStart(2, '0')

/** `YYYY-MM-DD` in the configured zone, for `<input type="date">`. */
export function toDateInput(ms: number): string {
  const p = parts(ms)
  return `${p.year}-${pad(p.month)}-${pad(p.day)}`
}

/** `HH:MM` in the configured zone, for `<input type="time">`. */
export function toTimeInput(ms: number): string {
  const p = parts(ms)
  return `${pad(p.hour)}:${pad(p.minute)}`
}

/** Date + time inputs in the configured zone to unix ms. `NaN` if invalid. */
export function fromInputs(date: string, time: string): number {
  const [y, mo, d] = date.split('-').map(Number)
  const [h, mi] = time.split(':').map(Number)
  if ([y, mo, d, h, mi].some((n) => n === undefined || !Number.isFinite(n))) return NaN
  return fromLocal(y, mo, d, h, mi)
}

/**
 * Start/end from a date and two times. An end time before the start time
 * means the entry crosses midnight.
 */
export function rangeFromInputs(date: string, start: string, end: string): { startAt: number; endAt: number } {
  const startAt = fromInputs(date, start)
  let endAt = fromInputs(date, end)
  if (endAt < startAt) endAt = addDays(endAt, 1)
  return { startAt, endAt }
}

/** Minutes for an hours field: `7:45` (hours:minutes) or `7.75` / `7,75` (decimal). */
export function minutesToHours(minutes: number, format: DurationFormat = 'decimal', decimalComma = false): string {
  const sign = minutes < 0 ? '-' : ''
  const abs = Math.abs(minutes)
  if (format === 'hm') return `${sign}${Math.floor(abs / 60)}:${pad(abs % 60)}`
  const s = sign + String(+(abs / 60).toFixed(2))
  return decimalComma ? s.replace('.', ',') : s
}

/**
 * Typed duration → minutes, or `null` if invalid. `7:45` is always hours and
 * minutes. With the hours:minutes format, `7,45` / `7.45` (two digits, < 60)
 * mean 7:45 too, as people write it; `7,5` stays 7½ h. With the decimal
 * format `7,45` is 7.45 h. A leading `+` or `-` is allowed.
 */
export function hoursToMinutes(value: string, format: DurationFormat = 'decimal'): number | null {
  const v = value.trim().replace(/^\+/, '').replace('−', '-')
  if (v === '') return null
  const hm = /^(-?)(\d+):(\d{1,2})$/.exec(v)
  const hmComma = format === 'hm' ? /^(-?)(\d+)[.,](\d{2})$/.exec(v) : null
  const match = hm ?? (hmComma && Number(hmComma[3]) < 60 ? hmComma : null)
  if (match) {
    if (Number(match[3]) > 59) return null
    const minutes = Number(match[2]) * 60 + Number(match[3])
    return match[1] === '-' ? -minutes : minutes
  }
  if (!/^-?(\d+([.,]\d*)?|[.,]\d+)$/.test(v)) return null
  const n = Number(v.replace(',', '.'))
  return Number.isFinite(n) ? Math.round(n * 60) : null
}

/** Signed duration: `+1:30` / `−0:45`. */
export function formatSigned(ms: number, format: DurationFormat = 'hm'): string {
  const sign = ms < 0 ? '−' : '+'
  return sign + formatDuration(Math.abs(ms), format)
}

/** `YYYY-MM-DD` as midnight in the configured zone (unlike `new Date(str)`, which is UTC). */
export function parseDate(date: string): number {
  return fromInputs(date, '00:00')
}

/** First day of the month containing `ms`, 00:00. */
export function startOfMonth(ms: number): number {
  const p = parts(ms)
  return fromLocal(p.year, p.month, 1)
}

/** Adds calendar months; the day is clamped to the target month's length. */
export function addMonths(ms: number, months: number): number {
  const p = parts(ms)
  const target = new Date(Date.UTC(p.year, p.month - 1 + months, 1))
  const last = new Date(Date.UTC(target.getUTCFullYear(), target.getUTCMonth() + 1, 0)).getUTCDate()
  return fromLocal(target.getUTCFullYear(), target.getUTCMonth() + 1, Math.min(p.day, last), p.hour, p.minute, p.second)
}

export type PeriodPreset = 'thisWeek' | 'lastWeek' | 'thisMonth' | 'lastMonth' | 'thisYear' | 'lastYear'

/** `[from, to)` of a preset period around `now`, in the configured zone. */
export function presetRange(preset: PeriodPreset, now: number): DayRange {
  const week = startOfWeek(now)
  const month = startOfMonth(now)
  const year = fromLocal(parts(now).year, 1, 1)
  switch (preset) {
    case 'thisWeek':
      return { from: week, to: addDays(week, 7) }
    case 'lastWeek':
      return { from: addDays(week, -7), to: week }
    case 'thisMonth':
      return { from: month, to: addMonths(month, 1) }
    case 'lastMonth':
      return { from: addMonths(month, -1), to: month }
    case 'thisYear':
      return { from: year, to: addMonths(year, 12) }
    case 'lastYear':
      return { from: addMonths(year, -12), to: year }
  }
}

/** ISO 8601 week number and week-based year of a `YYYY-MM-DD` date. */
export function isoWeek(date: string): { week: number; year: number } {
  const [y, m, d] = date.split('-').map(Number)
  const day = new Date(Date.UTC(y, m - 1, d))
  // The Thursday of the same week decides the year.
  day.setUTCDate(day.getUTCDate() + 4 - (day.getUTCDay() || 7))
  const year = day.getUTCFullYear()
  const week = Math.ceil(((day.getTime() - Date.UTC(year, 0, 1)) / 86_400_000 + 1) / 7)
  return { week, year }
}

export type DateOrder = 'dmy' | 'mdy' | 'ymd'

/**
 * Typed time → `HH:MM` (24 h), or `null`. Accepts `14:30`, `14.30`, `1430`,
 * `9`, `2:30 pm`, `2pm`, `12 am`.
 */
export function parseTimeText(text: string): string | null {
  const m = /^(\d{1,2})(?:[:.h]?(\d{2}))?\s*(?:([ap])\.?\s*m?\.?)?$/i.exec(text.trim())
  if (!m) return null
  let h = Number(m[1])
  const min = m[2] === undefined ? 0 : Number(m[2])
  if (min > 59) return null
  if (m[3]) {
    if (h < 1 || h > 12) return null
    h = (h % 12) + (m[3].toLowerCase() === 'p' ? 12 : 0)
  } else if (h > 23) {
    return null
  }
  return `${pad(h)}:${pad(min)}`
}

/** `HH:MM` (24 h) for display: `14:30` or `2:30 PM`. */
export function formatTimeText(hhmm: string, h12: boolean): string {
  const [h, m] = hhmm.split(':').map(Number)
  if (!Number.isFinite(h) || !Number.isFinite(m)) return ''
  if (!h12) return `${pad(h)}:${pad(m)}`
  return `${h % 12 || 12}:${pad(m)} ${h < 12 ? 'AM' : 'PM'}`
}

/**
 * Typed date → `YYYY-MM-DD`, or `null`. Day, month and year in `order`,
 * separated by `.`, `/`, `-` or spaces; a 4-digit first part is always
 * read as year-month-day. Two-digit years are 20xx.
 */
export function parseDateText(text: string, order: DateOrder): string | null {
  const parts = text.trim().split(/[./\-\s]+/).filter(Boolean)
  if (parts.length !== 3 || parts.some((p) => !/^\d+$/.test(p))) return null
  const n = parts.map(Number)
  let [y, m, d] = [0, 0, 0]
  if (parts[0].length === 4 || order === 'ymd') [y, m, d] = n
  else if (order === 'mdy') [m, d, y] = n
  else [d, m, y] = n
  if (y < 100) y += 2000
  const date = new Date(Date.UTC(y, m - 1, d))
  if (date.getUTCFullYear() !== y || date.getUTCMonth() !== m - 1 || date.getUTCDate() !== d) return null
  return `${y}-${pad(m)}-${pad(d)}`
}

/** `YYYY-MM-DD` for display in the date order. */
export function formatDateText(iso: string, order: DateOrder): string {
  const [y, m, d] = iso.split('-')
  if (!y || !m || !d) return ''
  if (order === 'ymd') return iso
  return order === 'mdy' ? `${m}/${d}/${y}` : `${d}.${m}.${y}`
}
