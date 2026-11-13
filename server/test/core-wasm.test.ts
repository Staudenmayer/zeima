// zeima-core's logic through WebAssembly (packages/core-wasm). Needs
// `bun run --cwd packages/core-wasm build`; CI builds it first.

import { existsSync } from 'node:fs'
import { createRequire } from 'node:module'
import { describe, expect, it } from 'vitest'

const PKG = new URL('../../packages/core-wasm/pkg/zeima_core_wasm.js', import.meta.url)
const built = existsSync(PKG)

describe.skipIf(!built)('@zeima/core-wasm', () => {
  const core = built ? (createRequire(import.meta.url)(PKG.pathname) as typeof import('../../packages/core-wasm/pkg/zeima_core_wasm.js')) : (null as never)

  it('lists the bundled country profiles', () => {
    expect(JSON.parse(core.profiles()).map((p: { id: string }) => p.id)).toEqual(['AT', 'DE', 'CH'])
    expect(JSON.parse(core.suggestProfile('de-AT'))).toBe('AT')
  })

  it('computes public holidays, with Easter-based ones', () => {
    const list = JSON.parse(core.holidays('AT', null, 2026)) as { id: string; date: string; dayOff: boolean }[]
    expect(list.find((h) => h.id === 'new_year')?.date).toBe('2026-01-01')
    expect(list.find((h) => h.id === 'easter_monday')?.date).toBe('2026-04-06')
    // Regions add their own.
    const vienna = JSON.parse(core.holidays('AT', 'AT-9', 2026)) as { id: string }[]
    expect(vienna.length).toBeGreaterThanOrEqual(list.length)
  })

  it('knows the legal break rule', () => {
    expect(JSON.parse(core.requiredBreak('AT', null, 400))).toMatchObject({ break_minutes: 30 })
    expect(JSON.parse(core.requiredBreak('AT', null, 300))).toBeNull()
  })

  const day = (targetMinutes: number) => ({ targetMinutes, coreStart: null, coreEnd: null })
  const week = [day(480), day(480), day(480), day(480), day(480), day(0), day(0)]
  const request = (extra: Record<string, unknown> = {}) =>
    JSON.stringify({
      from: '2026-06-01',
      to: '2026-06-01',
      tz: 'Europe/Vienna',
      profile: 'AT',
      region: null,
      schedules: [{ id: 's', validFrom: '2026-01-01', model: 'fixed', days: week }],
      intervals: [[Date.UTC(2026, 5, 1, 7, 0), Date.UTC(2026, 5, 1, 16, 0)]], // 09:00-18:00 in Vienna
      deductBreaks: true,
      ...extra,
    })

  it('computes a work day: nine hours, the missing break deducted, eight hours target', () => {
    const { days, balanceMs } = JSON.parse(core.computeDays(request()))
    expect(days).toHaveLength(1)
    expect(days[0].breakDeductedMs / 60_000).toBe(30)
    expect(days[0].netMs / 60_000).toBe(510)
    expect(days[0].targetMs / 60_000).toBe(480)
    expect(balanceMs / 60_000).toBe(30)
    expect(core.balanceMs(JSON.stringify(days))).toBe(balanceMs)
  })

  it('turns a holiday into a day off', () => {
    const { days } = JSON.parse(core.computeDays(request({ holidays: { '2026-06-01': 'Company day' }, intervals: [] })))
    expect(days[0].holiday).toBe('Company day')
    expect(days[0].targetMs).toBe(0)
  })

  it('reports bad input as an error, not a crash', () => {
    expect(() => core.computeDays('{}')).toThrow(/missing field/)
    expect(() => core.computeDays(request({ tz: 'Mars/Olympus' }))).toThrow(/time zone/)
    expect(() => core.holidays('XX', null, 2026)).toThrow()
  })
})
