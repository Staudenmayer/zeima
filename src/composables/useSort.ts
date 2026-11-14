import { computed, reactive, toValue, type MaybeRefOrGetter } from 'vue'

export type SortValue = string | number | boolean | null | undefined
export type SortDir = 'asc' | 'desc'

/** What a sortable column header (`SortHead`) needs. */
export interface SortState {
  key: string | null
  dir: SortDir
  toggle: (key: string) => void
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' })

function compare(a: SortValue, b: SortValue): number {
  if (typeof a === 'number' && typeof b === 'number') return a - b
  if (typeof a === 'boolean' || typeof b === 'boolean') return Number(a) - Number(b)
  return collator.compare(String(a), String(b))
}

/**
 * Click-to-sort for a table. A column is sorted ascending on the first click,
 * descending on the second, and back to the original order on the third.
 * `accessors` gives the value of each column by its key; empty values (null,
 * undefined, '') always sort last. The sort is stable.
 */
export function useSort<T>(
  rows: MaybeRefOrGetter<readonly T[]> | null,
  accessors: Record<string, (row: T) => SortValue>,
  initial?: { key: string; dir?: SortDir },
) {
  const sort = reactive<SortState>({
    key: initial?.key ?? null,
    dir: initial?.dir ?? 'asc',
    toggle(key: string) {
      if (sort.key !== key) {
        sort.key = key
        sort.dir = 'asc'
      } else if (sort.dir === 'asc') sort.dir = 'desc'
      else sort.key = null
    },
  })
  /** The rows in the order of the sort; usable for several lists that share the sort. */
  function apply<U extends T>(source: readonly U[]): U[] {
    const list = [...source]
    const get = sort.key ? accessors[sort.key] : undefined
    if (!get) return list
    const sign = sort.dir === 'asc' ? 1 : -1
    const empty = (v: SortValue) => v === null || v === undefined || v === ''
    return list
      .map((row, i) => ({ row, i, v: get(row) }))
      .sort((x, y) => {
        if (empty(x.v) || empty(y.v)) return empty(x.v) === empty(y.v) ? x.i - y.i : empty(x.v) ? 1 : -1
        return sign * compare(x.v, y.v) || x.i - y.i
      })
      .map((x) => x.row)
  }
  const sorted = computed(() => apply(rows === null ? [] : toValue(rows)))
  return { sort, sorted, apply }
}
