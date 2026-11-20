import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { isValidRule, occurrences } from '../src/meetings/occurrences.ts'
import { anyOverlap } from '../src/meetings/rooms.ts'

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

describe('anyOverlap', () => {
  it('finds exactly the overlaps of the nested comparison', () => {
    const naive = (a: [number, number][], b: [number, number][]) => a.some(([s, e]) => b.some(([c, d]) => s < d && c < e))
    const list = (start: number, step: number, len: number, n: number): [number, number][] => Array.from({ length: n }, (_, i) => [start + i * step, start + i * step + len])
    const cases: [[number, number][], [number, number][]][] = [
      [list(0, 10, 5, 20), list(5, 10, 5, 20)], // back to back
      [list(0, 10, 5, 20), list(4, 10, 5, 20)], // overlapping by one
      [list(0, 10, 0, 5), list(0, 10, 10, 5)], // zero length
      [list(0, 7, 3, 30), list(100, 11, 2, 30)],
      [[], list(0, 1, 1, 3)],
    ]
    for (const [a, b] of cases) expect(anyOverlap(a, b)).toBe(naive(a, b))
  })
})
