import { execFileSync } from 'node:child_process'
import { describe, expect, it } from 'vitest'
import { typesAcceptable } from '../src/sync/schema.ts'

describe('sync schema', () => {
  it('is current with the app\'s bindings (run `node scripts/sync-schema.mjs`)', () => {
    expect(() => execFileSync('node', [new URL('../../scripts/sync-schema.mjs', import.meta.url).pathname, '--check'], { stdio: 'pipe' })).not.toThrow()
  })

  it('checks known fields by their type, groups member by member, and lets unknown names through within limits', () => {
    expect(typesAcceptable('time_entry', { note: 'x', billable: true, tagIds: ['a'], span: { startAt: 1, endAt: null, tz: 'UTC' } })).toBe(true)
    expect(typesAcceptable('time_entry', { note: 5 })).toBe(false)
    expect(typesAcceptable('time_entry', { billable: 'yes' })).toBe(false)
    expect(typesAcceptable('time_entry', { span: { startAt: 'x' } })).toBe(false)
    expect(typesAcceptable('time_entry', { span: { other: 1 } })).toBe(false)
    expect(typesAcceptable('time_entry', { note: null })).toBe(false) // not nullable
    expect(typesAcceptable('time_entry', { projectId: null })).toBe(true)
    expect(typesAcceptable('time_entry', { newerAppField: 'x' })).toBe(true)
    expect(typesAcceptable('time_entry', { newerAppField: 'x'.repeat(20_000) })).toBe(false)
  })
})
