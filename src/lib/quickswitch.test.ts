import { describe, expect, it } from 'vitest'
import { isExact, matchScore, parseQuery, rank, removeTokens, resolve } from './quickswitch'

describe('parseQuery', () => {
  it('splits finished project and tag tokens from the activity text', () => {
    const p = parseQuery('meet @INT #remote #client ')
    expect(p.text).toBe('meet')
    expect(p.active).toBeNull()
    expect(p.tokens.map((t) => [t.kind, t.term])).toEqual([
      ['project', 'INT'],
      ['tag', 'remote'],
      ['tag', 'client'],
    ])
  })
  it('treats the last token as active while it is typed', () => {
    expect(parseQuery('dev @we').active).toEqual({ kind: 'project', term: 'we', start: 4, end: 7 })
    expect(parseQuery('dev #').active).toEqual({ kind: 'tag', term: '', start: 4, end: 5 })
    expect(parseQuery('dev #').text).toBe('dev')
  })
  it('ends the active token with a space', () => {
    const p = parseQuery('@web ')
    expect(p.active).toBeNull()
    expect(p.tokens).toHaveLength(1)
  })
  it('keeps a lone finished prefix as text', () => {
    expect(parseQuery('a @ b').text).toBe('a @ b')
  })
  it('handles empty input', () => {
    expect(parseQuery('')).toEqual({ tokens: [], active: null, text: '' })
  })
})

describe('removeTokens', () => {
  it('cuts tokens out and keeps a trailing space for the next word', () => {
    const input = 'code @web #dev review'
    const { tokens } = parseQuery(input)
    expect(removeTokens(input, tokens)).toBe('code review ')
  })
  it('removes the active token', () => {
    const input = 'code @we'
    const { active } = parseQuery(input)
    expect(removeTokens(input, [active!])).toBe('code ')
  })
  it('returns an empty string when nothing is left', () => {
    const input = '@we'
    expect(removeTokens(input, [parseQuery(input).active!])).toBe('')
  })
})

describe('matching', () => {
  const projects = [
    { name: 'Website relaunch', alias: 'WEB' },
    { name: 'Internal', alias: 'INT' },
    { name: 'Interview prep', alias: null },
  ]
  it('prefers exact alias over name prefix', () => {
    expect(matchScore(projects[1], 'int')).toBeGreaterThan(matchScore(projects[2], 'int'))
    expect(resolve(projects, 'int')?.name).toBe('Internal')
  })
  it('matches word prefixes and subsequences', () => {
    expect(rank(projects, 'relaunch').map((p) => p.name)).toEqual(['Website relaunch'])
    expect(rank(projects, 'wsr').map((p) => p.name)).toEqual(['Website relaunch'])
  })
  it('returns everything for an empty query in input order', () => {
    expect(rank(projects, '')).toEqual(projects)
  })
  it('returns null when nothing matches', () => {
    expect(resolve(projects, 'zzz')).toBeNull()
  })
  it('detects exact names and aliases', () => {
    expect(isExact(projects[0], 'web')).toBe(true)
    expect(isExact(projects[0], 'website')).toBe(false)
    expect(isExact(projects[0], ' ')).toBe(false)
  })
})
