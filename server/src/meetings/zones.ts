// Wall-clock days in a time zone, as unix milliseconds.

const OFFSET_PARTS = ['year', 'month', 'day', 'hour', 'minute', 'second'] as const

/** Offset of `tz` from UTC at the instant `ms`, in milliseconds. */
function offsetMs(ms: number, tz: string): number {
  const parts = new Intl.DateTimeFormat('en-US', {
    timeZone: tz,
    hourCycle: 'h23',
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  }).formatToParts(new Date(ms))
  const v = Object.fromEntries(parts.filter((p) => (OFFSET_PARTS as readonly string[]).includes(p.type)).map((p) => [p.type, Number(p.value)]))
  return Date.UTC(v.year!, v.month! - 1, v.day!, v.hour!, v.minute!, v.second!) - Math.floor(ms / 1000) * 1000
}

/** Midnight at the start of `date` (`YYYY-MM-DD`) in `tz`, as unix ms. */
export function startOfDay(date: string, tz: string): number {
  const [y, m, d] = date.split('-').map(Number) as [number, number, number]
  const wall = Date.UTC(y, m - 1, d)
  // The offset at midnight itself; one correction covers a change of offset on that day.
  const first = wall - offsetMs(wall, tz)
  return wall - offsetMs(first, tz)
}
