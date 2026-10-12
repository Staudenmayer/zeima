// Quick-switch input: `activity words @project #tag`.
//   words     search the activities (required; the user picks one)
//   @token    project, matched by alias, then name (case-insensitive)
//   #token    tag (repeatable), matched by name
// The token still being typed at the end of the input switches the list to
// project or tag suggestions; picking one turns it into a chip.

export type TokenKind = 'project' | 'tag'

export interface Token {
  kind: TokenKind
  /** Text after the `@` / `#`; empty right after typing the prefix. */
  term: string
  /** Position of the token in the input, `end` exclusive. */
  start: number
  end: number
}

export interface ParsedQuery {
  /** Finished `@` / `#` tokens (followed by a space). */
  tokens: Token[]
  /** The `@` / `#` token being typed at the end of the input. */
  active: Token | null
  /** All other words: the activity search. */
  text: string
}

const PREFIX: Record<string, TokenKind> = { '@': 'project', '#': 'tag' }

export function parseQuery(input: string): ParsedQuery {
  const tokens: Token[] = []
  const words: string[] = []
  let active: Token | null = null
  for (const m of input.matchAll(/\S+/g)) {
    const word = m[0]
    const start = m.index
    const end = start + word.length
    const kind = PREFIX[word[0]]
    if (kind && end === input.length) {
      active = { kind, term: word.slice(1), start, end }
    } else if (kind && word.length > 1) {
      tokens.push({ kind, term: word.slice(1), start, end })
    } else {
      words.push(word)
    }
  }
  return { tokens, active, text: words.join(' ') }
}

/** `input` without the given tokens, with whitespace tidied up. */
export function removeTokens(input: string, tokens: Token[]): string {
  let out = input
  for (const t of [...tokens].sort((a, b) => b.start - a.start)) {
    out = out.slice(0, t.start) + out.slice(t.end)
  }
  const trimmed = out.replace(/\s+/g, ' ').trimStart()
  // Keep a trailing space so the next word starts fresh.
  return trimmed.trim() ? trimmed.replace(/\s*$/, ' ') : ''
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

/** Best match for an `@token` / `#token`, or null. */
export function resolve<T extends Named>(items: T[], token: string): T | null {
  return rank(items, token)[0] ?? null
}

/** True when `query` names `item` exactly (alias or name), ignoring case. */
export function isExact(item: Named, query: string): boolean {
  const q = query.trim().toLowerCase()
  return !!q && (item.name.toLowerCase() === q || item.alias?.toLowerCase() === q)
}
