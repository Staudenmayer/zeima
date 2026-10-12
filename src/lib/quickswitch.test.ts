import { describe, expect, it } from 'vitest'
import { matchScore, parseQuery, rank, resolve } from './quickswitch'

describe('parseQuery', () => {
  it('splits project, tags and note', () => {
    expect(parseQuery('#INT @meeting @remote sprint planning')).toEqual({
      project: 'INT',
      tags: ['meeting', 'remote'],
      note: 'sprint planning',
    })
  })
  it('works in any order and ignores extra spaces', () => {
    expect(parseQuery('  fix login   #web  @dev ')).toEqual({ project: 'web', tags: ['dev'], note: 'fix login' })
  })
  it('keeps a lone # or @ as text and uses only the first project', () => {
    expect(parseQuery('# @ #a #b')).toEqual({ project: 'a', tags: [], note: '# @ #b' })
  })
  it('handles empty input', () => {
    expect(parseQuery('')).toEqual({ project: null, tags: [], note: '' })
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
})
