import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import de from './de'
import en from './en'

// Every permission a role can be given has a text, in both languages; the
// catalog is the server's (server/src/plugins/permissions.ts).
const source = readFileSync(new URL('../../server/src/plugins/permissions.ts', import.meta.url), 'utf8')
const actions = [...source.matchAll(/def\('(\w+)', '(?:view|edit|delete)', '([a-z_]+\.[a-z_]+)'/g)].map((m) => m[2]!)
const resources = [...new Set([...source.matchAll(/def\('(\w+)', '(?:view|edit|delete)'/g)].map((m) => m[1]!))]

const lookup = (messages: Record<string, unknown>, path: string) => path.split('.').reduce<unknown>((o, k) => (o as Record<string, unknown> | undefined)?.[k], messages)

describe('the permission texts', () => {
  it('finds the catalog', () => {
    expect(actions.length).toBeGreaterThan(25)
    expect(resources.length).toBeGreaterThan(8)
  })

  for (const [lang, messages] of [['en', en], ['de', de]] as const) {
    it(`${lang} has a text for every permission and resource`, () => {
      for (const action of actions) expect(typeof lookup(messages, `org.perm.${action}`), `org.perm.${action}`).toBe('string')
      for (const r of resources) expect(typeof lookup(messages, `org.permResource.${r}`), `org.permResource.${r}`).toBe('string')
      for (const k of ['view', 'edit', 'delete']) expect(typeof lookup(messages, `org.permKind.${k}`), k).toBe('string')
      for (const s of ['none', 'own', 'group', 'org']) expect(typeof lookup(messages, `org.scope.${s}`), s).toBe('string')
    })
  }
})
