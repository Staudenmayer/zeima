// The shared merge vectors (packages/shared-fixtures/sync/merge.json); the
// Rust implementation runs the same file.

import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { type Actor, type Change, type Outcome, type RowState, emptyRow, isLive, mergeAll } from '../src/sync/merge.ts'

interface Vectors {
  cases: { name: string; initial: RowState | null; actor?: Actor; changes: Change[]; expect: { state: RowState; results: Outcome[] } }[]
  convergence: { name: string; changes: Change[] }[]
}

const vectors: Vectors = JSON.parse(readFileSync(new URL('../../packages/shared-fixtures/sync/merge.json', import.meta.url), 'utf8'))
const MANAGER: Actor = { manager: true }

function permutations<T>(items: T[]): T[][] {
  if (items.length <= 1) return [items]
  return items.flatMap((head, i) => permutations([...items.slice(0, i), ...items.slice(i + 1)]).map((tail) => [head, ...tail]))
}

describe('sync merge vectors', () => {
  it('has vectors', () => {
    expect(vectors.cases.length).toBeGreaterThan(10)
    expect(vectors.convergence.length).toBeGreaterThan(0)
  })

  for (const c of vectors.cases) {
    it(c.name, () => {
      const { state, outcomes } = mergeAll(c.initial, c.changes, c.actor ?? { manager: false })
      expect(outcomes).toEqual(c.expect.results)
      expect(state ?? emptyRow()).toEqual(c.expect.state)
    })
  }

  for (const c of vectors.convergence) {
    it(`converges: ${c.name}`, () => {
      const finals = permutations(c.changes).map((order) => {
        const { state } = mergeAll(null, order, MANAGER)
        const again = mergeAll(state, order, MANAGER)
        expect(again.outcomes.every((o) => o.status === 'ignored'), 'a replay changes nothing').toBe(true)
        expect(again.state).toEqual(state)
        return state
      })
      for (const f of finals) expect(f).toEqual(finals[0])
    })
  }

  it('a row is live until deleted and not written after', () => {
    const row = emptyRow()
    expect(isLive(row)).toBe(true)
    row.deleted = '000000000000020-000000-a'
    expect(isLive(row)).toBe(false)
    row.fields.name = { v: 'x', h: '000000000000030-000000-a' }
    expect(isLive(row)).toBe(true)
  })

  it('does not modify the state it is given', () => {
    const initial: RowState = { fields: { name: { v: 'A', h: '000000000000010-000000-a' } }, deleted: null }
    const copy = structuredClone(initial)
    mergeAll(initial, [{ table: 'project', rowId: 'r', op: 'upsert', hlc: '000000000000020-000000-a', fields: { name: 'B' } }], MANAGER)
    expect(initial).toEqual(copy)
  })
})
