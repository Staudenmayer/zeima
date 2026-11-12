// The contract (SPEC §7.0): every route is documented completely, and the
// committed openapi.json matches the code.

import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { CONTRACT_PATH, exportOpenApi } from '../scripts/openapi-export.ts'

interface Operation {
  tags?: string[]
  summary?: string
  security?: Record<string, string[]>[]
  responses?: Record<string, unknown>
}

describe('OpenAPI contract', async () => {
  const generated = await exportOpenApi()
  const doc = JSON.parse(generated) as { paths: Record<string, Record<string, Operation>> }
  const operations = Object.entries(doc.paths).flatMap(([path, methods]) =>
    Object.entries(methods).map(([method, op]) => ({ name: `${method.toUpperCase()} ${path}`, op })),
  )

  it.each(operations)('$name has tags, a summary and response schemas', ({ op }) => {
    expect(op.tags?.length).toBeGreaterThan(0)
    expect(op.summary).toBeTruthy()
    const statuses = Object.keys(op.responses ?? {})
    expect(statuses.some((s) => s.startsWith('2'))).toBe(true)
    // Protected routes document their auth failures.
    if (op.security?.length) expect(statuses).toEqual(expect.arrayContaining(['401', '403']))
  })

  it('versions the API under /v1', () => {
    const unversioned = Object.keys(doc.paths).filter((p) => !p.startsWith('/v1/') && !['/healthz', '/readyz'].includes(p))
    expect(unversioned).toEqual([])
  })

  it('is committed in packages/api-contract (run `bun run --cwd server openapi:export`)', () => {
    expect(readFileSync(CONTRACT_PATH, 'utf8')).toBe(generated)
  })
})
