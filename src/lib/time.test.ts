import { afterEach, describe, expect, it } from 'vitest'
import {
  HOUR,
  MINUTE,
  addDays,
  addMonths,
  formatClock,
  presetRange,
  startOfMonth,
  formatDateText,
  formatDuration,
  formatTimeText,
  hoursToMinutes,
  minutesToHours,
  isoWeek,
  fromInputs,
  parseDate,
  parseDateText,
  parseTimeText,
  rangeFromInputs,
  setTimeZone,
  setWeekStart,
  startOfDay,
  startOfWeek,
  toDateInput,
  toTimeInput,
  weekDays,
  weekday,
} from './time'

describe('formatDuration', () => {
  it('formats hours and minutes', () => {
    expect(formatDuration(0)).toBe('0:00')
    expect(formatDuration(7 * HOUR + 42 * MINUTE)).toBe('7:42')
    expect(formatDuration(59_999)).toBe('0:00')
  })
  it('formats decimal hours', () => {
    expect(formatDuration(7 * HOUR + 42 * MINUTE, 'decimal')).toBe('7.70')
  })
  it('never shows negative durations', () => {
    expect(formatDuration(-5 * MINUTE)).toBe('0:00')
  })
})

describe('formatClock', () => {
  it('shows seconds', () => {
    expect(formatClock(HOUR + 2 * MINUTE + 3000)).toBe('1:02:03')
  })
})

describe('weeks', () => {
  it('starts on Monday', () => {
    const wednesday = new Date(2026, 9, 7, 15, 30).getTime()
    expect(new Date(startOfWeek(wednesday)).getDay()).toBe(1)
    expect(toDateInput(startOfWeek(wednesday))).toBe('2026-10-05')
  })
  it('has 7 consecutive days', () => {
    const days = weekDays(new Date(2026, 9, 4).getTime())
    expect(days).toHaveLength(7)
    expect(toDateInput(days[0].from)).toBe('2026-09-28')
    expect(toDateInput(days[6].from)).toBe('2026-10-04')
    for (let i = 1; i < 7; i++) expect(days[i].from).toBe(days[i - 1].to)
  })
  it('adds calendar days', () => {
    const d = new Date(2026, 2, 28, 12).getTime()
    expect(toDateInput(addDays(d, 1))).toBe('2026-03-29')
  })
})

describe('inputs', () => {
  it('round-trips date and time', () => {
    const ms = fromInputs('2026-10-04', '09:15')
    expect(toDateInput(ms)).toBe('2026-10-04')
    expect(toTimeInput(ms)).toBe('09:15')
  })
  it('rejects invalid input', () => {
    expect(fromInputs('', '09:00')).toBeNaN()
    expect(fromInputs('2026-10-04', '')).toBeNaN()
  })
  it('treats an end before the start as the next day', () => {
    const { startAt, endAt } = rangeFromInputs('2026-10-04', '22:00', '01:30')
    expect(endAt - startAt).toBe(3.5 * HOUR)
  })
})

describe('hours inputs', () => {
  it('parses decimal and h:mm', async () => {
    const { hoursToMinutes, minutesToHours, formatSigned } = await import('./time')
    expect(hoursToMinutes('7,5')).toBe(450)
    expect(hoursToMinutes('7.75')).toBe(465)
    expect(hoursToMinutes('7:30')).toBe(450)
    expect(hoursToMinutes('-1:15')).toBe(-75)
    expect(hoursToMinutes('')).toBeNull()
    expect(hoursToMinutes('abc')).toBeNull()
    expect(minutesToHours(462)).toBe('7.7')
    expect(formatSigned(-45 * MINUTE)).toBe('−0:45')
    expect(formatSigned(90 * MINUTE)).toBe('+1:30')
  })
})

describe('months and presets', () => {
  // Thursday, 2026-12-17 15:00 local.
  const now = new Date(2026, 11, 17, 15).getTime()

  it('adds months and clamps the day', () => {
    expect(toDateInput(addMonths(new Date(2026, 0, 31).getTime(), 1))).toBe('2026-02-28')
    expect(toDateInput(addMonths(new Date(2026, 11, 1).getTime(), 1))).toBe('2027-01-01')
    expect(toDateInput(startOfMonth(now))).toBe('2026-12-01')
  })

  it('computes preset ranges', () => {
    const r = (p: Parameters<typeof presetRange>[0]) => {
      const { from, to } = presetRange(p, now)
      return [toDateInput(from), toDateInput(to)]
    }
    expect(r('thisWeek')).toEqual(['2026-12-14', '2026-12-21'])
    expect(r('lastWeek')).toEqual(['2026-12-07', '2026-12-14'])
    expect(r('thisMonth')).toEqual(['2026-12-01', '2027-01-01'])
    expect(r('lastMonth')).toEqual(['2026-11-01', '2026-12-01'])
    expect(r('thisYear')).toEqual(['2026-01-01', '2027-01-01'])
    expect(r('lastYear')).toEqual(['2025-01-01', '2026-01-01'])
  })
})

describe('configured time zone and week start', () => {
  afterEach(() => {
    setTimeZone(undefined)
    setWeekStart(1)
  })

  it('computes days in the configured zone, not the runtime zone', () => {
    setTimeZone('Pacific/Auckland')
    // 2026-10-05 20:00 UTC is already Oct 6 09:00 in Auckland (UTC+13).
    const ms = Date.UTC(2026, 9, 5, 20)
    expect(toDateInput(ms)).toBe('2026-10-06')
    expect(toTimeInput(ms)).toBe('09:00')
    expect(startOfDay(ms)).toBe(Date.UTC(2026, 9, 5, 11))
    expect(fromInputs('2026-10-06', '09:00')).toBe(ms)
  })

  it('handles daylight saving transitions', () => {
    setTimeZone('Europe/Vienna')
    // Spring forward on 2026-03-29: the day has 23 hours.
    const day = parseDate('2026-03-29')
    expect(addDays(day, 1) - day).toBe(23 * HOUR)
    expect(toTimeInput(addDays(fromInputs('2026-03-28', '09:00'), 1))).toBe('09:00')
    // 02:30 does not exist that day; it resolves to after the gap.
    expect(toTimeInput(fromInputs('2026-03-29', '02:30'))).toBe('03:30')
    // Fall back on 2026-10-25: 25 hours.
    const autumn = parseDate('2026-10-25')
    expect(addDays(autumn, 1) - autumn).toBe(25 * HOUR)
  })

  it('uses a fixed offset zone without DST', () => {
    setTimeZone('Etc/GMT-1')
    const day = parseDate('2026-03-29')
    expect(addDays(day, 1) - day).toBe(24 * HOUR)
    expect(day).toBe(Date.UTC(2026, 2, 28, 23))
  })

  it('starts weeks on the configured day', () => {
    setTimeZone('UTC')
    const thu = Date.UTC(2026, 9, 8, 12)
    expect(toDateInput(startOfWeek(thu))).toBe('2026-10-05')
    setWeekStart(0)
    expect(toDateInput(startOfWeek(thu))).toBe('2026-10-04')
    expect(toDateInput(weekDays(thu)[6].from)).toBe('2026-10-10')
    setWeekStart(6)
    expect(toDateInput(startOfWeek(thu))).toBe('2026-10-03')
    expect(weekday(thu)).toBe(4)
  })
})

describe('isoWeek', () => {
  it('numbers weeks per ISO 8601', () => {
    expect(isoWeek('2026-10-05')).toEqual({ week: 41, year: 2026 })
    expect(isoWeek('2027-01-01')).toEqual({ week: 53, year: 2026 })
    expect(isoWeek('2026-01-01')).toEqual({ week: 1, year: 2026 })
    expect(isoWeek('2024-12-30')).toEqual({ week: 1, year: 2025 })
  })
})

describe('typed times and dates', () => {
  it('parses 24 h and 12 h times', () => {
    expect(parseTimeText('14:30')).toBe('14:30')
    expect(parseTimeText('1430')).toBe('14:30')
    expect(parseTimeText('9')).toBe('09:00')
    expect(parseTimeText('9.05')).toBe('09:05')
    expect(parseTimeText('2:30 pm')).toBe('14:30')
    expect(parseTimeText('2PM')).toBe('14:00')
    expect(parseTimeText('12 am')).toBe('00:00')
    expect(parseTimeText('12:15 p.m.')).toBe('12:15')
    expect(parseTimeText('24:00')).toBeNull()
    expect(parseTimeText('13 pm')).toBeNull()
    expect(parseTimeText('9:75')).toBeNull()
  })

  it('formats times in 24 h or 12 h', () => {
    expect(formatTimeText('14:30', false)).toBe('14:30')
    expect(formatTimeText('14:30', true)).toBe('2:30 PM')
    expect(formatTimeText('00:05', true)).toBe('12:05 AM')
    expect(formatTimeText('12:00', true)).toBe('12:00 PM')
  })

  it('parses dates in the configured order', () => {
    expect(parseDateText('31.12.2026', 'dmy')).toBe('2026-12-31')
    expect(parseDateText('12/31/2026', 'mdy')).toBe('2026-12-31')
    expect(parseDateText('2026-12-31', 'dmy')).toBe('2026-12-31')
    expect(parseDateText('5.1.26', 'dmy')).toBe('2026-01-05')
    expect(parseDateText('31.02.2026', 'dmy')).toBeNull()
    expect(parseDateText('hello', 'dmy')).toBeNull()
    expect(formatDateText('2026-01-05', 'dmy')).toBe('05.01.2026')
    expect(formatDateText('2026-01-05', 'mdy')).toBe('01/05/2026')
    expect(formatDateText('2026-01-05', 'ymd')).toBe('2026-01-05')
  })
})

describe('hour fields', () => {
  it('reads h,mm in the hours:minutes format', () => {
    expect(hoursToMinutes('7,45', 'hm')).toBe(7 * 60 + 45)
    expect(hoursToMinutes('7.45', 'hm')).toBe(7 * 60 + 45)
    expect(hoursToMinutes('7:45', 'hm')).toBe(7 * 60 + 45)
    expect(hoursToMinutes('7,5', 'hm')).toBe(7 * 60 + 30)
    expect(hoursToMinutes('7,75', 'hm')).toBe(7 * 60 + 45)
    expect(hoursToMinutes('-0:30', 'hm')).toBe(-30)
    expect(hoursToMinutes('+2:30', 'hm')).toBe(150)
    // Five days of 7,45 make 38:45, not 37:15.
    expect(5 * hoursToMinutes('7,45', 'hm')!).toBe(38 * 60 + 45)
  })

  it('reads decimal hours in the decimal format', () => {
    expect(hoursToMinutes('7,45', 'decimal')).toBe(447)
    expect(hoursToMinutes('7,75', 'decimal')).toBe(465)
    expect(hoursToMinutes('7:45', 'decimal')).toBe(465)
    expect(hoursToMinutes('abc', 'decimal')).toBeNull()
    expect(hoursToMinutes('7:75', 'hm')).toBeNull()
  })

  it('writes fields in the duration format', () => {
    expect(minutesToHours(465, 'hm')).toBe('7:45')
    expect(minutesToHours(465, 'decimal', true)).toBe('7,75')
    expect(minutesToHours(-30, 'hm')).toBe('-0:30')
  })
})
