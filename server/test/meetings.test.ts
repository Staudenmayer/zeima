import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { isValidRule, occurrences } from '../src/meetings/occurrences.ts'

interface Case {
  name: string
  startAt: number
  endAt: number
  tz: string
  rrule: string | null
  from: number
  to: number
  expected: [number, number][]
}

const cases = JSON.parse(readFileSync(new URL('../../packages/shared-fixtures/meetings/occurrences.json', import.meta.url), 'utf8')) as Case[]

describe('meeting occurrences (shared vectors with the desktop app)', () => {
  for (const c of cases) {
    it(c.name, () => {
      expect(occurrences(c.startAt, c.endAt, c.tz, c.rrule, c.from, c.to)).toEqual(c.expected)
    })
  }

  it('checks rules', () => {
    expect(isValidRule('FREQ=WEEKLY;BYDAY=MO,WE;COUNT=4')).toBe(true)
    expect(isValidRule('FREQ=HOURLY')).toBe(false)
    expect(isValidRule('INTERVAL=2')).toBe(false)
    expect(isValidRule('FREQ=WEEKLY;BYDAY=XX')).toBe(false)
  })
})
