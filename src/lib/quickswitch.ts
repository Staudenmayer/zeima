// Quick-switch input: `#ALIAS @tag note text`.
//   #token  project, matched by alias, then name (case-insensitive)
//   @token  tag (repeatable), matched by name
//   rest    note
// Without `#`, the text filters the project list instead.

export interface ParsedQuery {
  project: string | null
  tags: string[]
  note: string
}

export function parseQuery(input: string): ParsedQuery {
  let project: string | null = null
  const tags: string[] = []
  const words: string[] = []
  for (const word of input.trim().split(/\s+/).filter(Boolean)) {
    if (word.startsWith('#') && word.length > 1 && project === null) {
      project = word.slice(1)
    } else if (word.startsWith('@') && word.length > 1) {
      tags.push(word.slice(1))
    } else {
      words.push(word)
    }
  }
  return { project, tags, note: words.join(' ') }
}

interface Named {
  name: string
  alias?: string | null
}

/** Higher is better; 0 = no match. */
export function matchScore(item: Named, query: string): number {
  const q = query.trim().toLowerCase()
  if (!q) return 1
  const alias = item.alias?.toLowerCase() ?? ''
  const name = item.name.toLowerCase()
  if (alias && alias === q) return 100
  if (name === q) return 90
  if (alias && alias.startsWith(q)) return 80
  if (name.startsWith(q)) return 70
  if (name.split(/[\s\-_/.]+/).some((w) => w.startsWith(q))) return 60
  if (name.includes(q) || (alias && alias.includes(q))) return 40
  if (isSubsequence(q, name)) return 20
  return 0
}

function isSubsequence(needle: string, haystack: string): boolean {
  let i = 0
  for (const c of haystack) {
    if (c === needle[i]) i++
    if (i === needle.length) return true
  }
  return needle.length === 0
}

/** Items matching `query`, best first; ties keep the input order. */
export function rank<T extends Named>(items: T[], query: string): T[] {
  return items
    .map((item, index) => ({ item, index, score: matchScore(item, query) }))
    .filter((r) => r.score > 0)
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .map((r) => r.item)
}

/** Best match for a `#token` / `@token`, or null. */
export function resolve<T extends Named>(items: T[], token: string): T | null {
  return rank(items, token)[0] ?? null
}
