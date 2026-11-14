import { computed, ref, toValue, type MaybeRefOrGetter } from 'vue'

/** Whether `values` pass a multi-select filter: nothing selected lets everything through. */
export function passes(selected: readonly (string | null)[], values: string | null | readonly (string | null)[]): boolean {
  if (selected.length === 0) return true
  return (Array.isArray(values) ? (values as readonly (string | null)[]) : [values as string | null]).some((v) => selected.includes(v))
}

/**
 * The text search every list table has: rows whose `text` (the cells worth
 * searching) contain every word typed, ignoring case. `extra` narrows the
 * rows further (the page's column filters); `active` says whether anything
 * narrows the list, and `clear` resets the search (the page resets its filters).
 */
export function useTableSearch<T>(
  rows: MaybeRefOrGetter<readonly T[]>,
  text: (row: T) => readonly (string | number | null | undefined)[],
  extra?: (row: T) => boolean,
) {
  const search = ref('')
  const words = computed(() => search.value.trim().toLowerCase().split(/\s+/).filter(Boolean))
  /** Whether a row has every word typed (the search alone, without `extra`). */
  const matches = (row: T) => {
    if (words.value.length === 0) return true
    const hay = text(row).filter((x) => x !== null && x !== undefined).join(' ').toLowerCase()
    return words.value.every((w) => hay.includes(w))
  }
  const filtered = computed(() => toValue(rows).filter((row) => (!extra || extra(row)) && matches(row)))
  const searching = computed(() => words.value.length > 0)
  return { search, filtered, searching, matches }
}
