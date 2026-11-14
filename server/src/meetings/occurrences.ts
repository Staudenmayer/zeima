// When a meeting happens (mirrors crates/zeima-core/src/meeting.rs; both run
// packages/shared-fixtures/meetings/occurrences.json).
//
// Rules: FREQ=DAILY|WEEKLY|MONTHLY, INTERVAL, COUNT, UNTIL=YYYYMMDD (inclusive)
// and, for weekly rules, BYDAY. Repeats keep the local time of day in the
// meeting's zone and its length.

type Freq = 'DAILY' | 'WEEKLY' | 'MONTHLY'
interface Rule {
  freq: Freq
  interval: number
  count: number | null
  until: string | null
  byDay: number[]
}

/** Monday = 0 … Sunday = 6. */
const DAYS = ['MO', 'TU', 'WE', 'TH', 'FR', 'SA', 'SU']

function parse(text: string): Rule | null {
  const rule: Rule = { freq: 'DAILY', interval: 1, count: null, until: null, byDay: [] }
  let have = false
  for (const part of text.split(';')) {
    const [k, v] = part.split('=')
    if (!k || v === undefined) return null
    switch (k.toUpperCase()) {
      case 'FREQ': {
        const f = v.toUpperCase()
        if (f !== 'DAILY' && f !== 'WEEKLY' && f !== 'MONTHLY') return null
        rule.freq = f
        have = true
        break
      }
      case 'INTERVAL':
        rule.interval = Math.max(1, Number.parseInt(v, 10) || 1)
        break
      case 'COUNT': {
        const n = Number.parseInt(v, 10)
        if (!Number.isFinite(n)) return null
        rule.count = n
        break
      }
      case 'UNTIL':
        if (!/^\d{8}/.test(v)) return null
        rule.until = `${v.slice(0, 4)}-${v.slice(4, 6)}-${v.slice(6, 8)}`
        break
      case 'BYDAY': {
        const days = v.split(',').map((d) => DAYS.indexOf(d.trim().toUpperCase()))
        if (days.includes(-1)) return null
        rule.byDay = days
        break
      }
    }
  }
  return have ? rule : null
}

export const isValidRule = (rule: string) => parse(rule) !== null

const parts = (ms: number, tz: string) => {
  const v = Object.fromEntries(
    new Intl.DateTimeFormat('en-US', { timeZone: tz, hourCycle: 'h23', year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit' })
      .formatToParts(new Date(ms))
      .filter((p) => p.type !== 'literal')
      .map((p) => [p.type, Number(p.value)]),
  ) as Record<'year' | 'month' | 'day' | 'hour' | 'minute' | 'second', number>
  return v
}

/** Offset of `tz` from UTC at the instant `ms`. */
const offsetAt = (ms: number, tz: string) => {
  const p = parts(ms, tz)
  return Date.UTC(p.year, p.month - 1, p.day, p.hour, p.minute, p.second) - Math.floor(ms / 1000) * 1000
}

/** Unix ms of the wall-clock time `secondsOfDay` on the date `[y, m, d]` in `tz`. */
function zoned(y: number, m: number, d: number, secondsOfDay: number, tz: string): number {
  const wall = Date.UTC(y, m - 1, d) + secondsOfDay * 1000
  const first = wall - offsetAt(wall, tz)
  return wall - offsetAt(first, tz)
}

const dateKey = (y: number, m: number, d: number) => `${y}-${String(m).padStart(2, '0')}-${String(d).padStart(2, '0')}`
const addDays = (y: number, m: number, d: number, n: number): [number, number, number] => {
  const t = new Date(Date.UTC(y, m - 1, d + n))
  return [t.getUTCFullYear(), t.getUTCMonth() + 1, t.getUTCDate()]
}

/** Start and end of each time the meeting happens that overlaps `[from, to)`. */
export function occurrences(startAt: number, endAt: number, tz: string, rrule: string | null, from: number, to: number): [number, number][] {
  const len = Math.max(0, endAt - startAt)
  const overlaps = (s: number) => s < to && s + len > from
  const single = (): [number, number][] => (overlaps(startAt) ? [[startAt, startAt + len]] : [])
  const rule = rrule ? parse(rrule) : null
  if (!rule) return single()
  let first: ReturnType<typeof parts>
  try {
    first = parts(startAt, tz)
  } catch {
    return single() // an unknown zone
  }
  const seconds = first.hour * 3600 + first.minute * 60 + first.second
  const d0: [number, number, number] = [first.year, first.month, first.day]
  const weekday0 = (new Date(Date.UTC(d0[0], d0[1] - 1, d0[2])).getUTCDay() + 6) % 7

  const datesOf = (p: number): [number, number, number][] => {
    switch (rule.freq) {
      case 'DAILY':
        return [addDays(...d0, p * rule.interval)]
      case 'WEEKLY': {
        const monday = addDays(...d0, -weekday0 + p * rule.interval * 7)
        const days = [...new Set(rule.byDay.length ? rule.byDay : [weekday0])].sort((a, b) => a - b)
        return days.map((d) => addDays(...monday, d))
      }
      case 'MONTHLY': {
        const index = d0[0] * 12 + (d0[1] - 1) + p * rule.interval
        const y = Math.floor(index / 12)
        const m = (index % 12) + 1
        // A month without that day has no meeting.
        const real = new Date(Date.UTC(y, m - 1, d0[2]))
        return real.getUTCMonth() === m - 1 ? [[y, m, d0[2]]] : []
      }
    }
  }

  const out: [number, number][] = []
  let produced = 0
  for (let p = 0; p < 5000; p++) {
    for (const [y, m, d] of datesOf(p)) {
      const key = dateKey(y, m, d)
      if (key < dateKey(...d0)) continue
      if ((rule.until !== null && key > rule.until) || (rule.count !== null && produced >= rule.count)) return out
      const s = zoned(y, m, d, seconds, tz)
      if (s >= to) return out
      produced++
      if (overlaps(s)) out.push([s, s + len])
    }
  }
  return out
}
