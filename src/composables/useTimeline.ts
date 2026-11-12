import { onScopeDispose, ref, watch, type Ref } from 'vue'
import { commands, events, type AddonTimelineItem, type Range } from '@/bindings'
import { call } from '@/lib/ipc'

/**
 * Addon timeline items (e.g. calendar events) for `range`, reloaded when an
 * addon synced. Failing addons are reported on the Addons page, so errors
 * leave the list empty here.
 */
export function useTimeline(range: Ref<Range>) {
  const items = ref<AddonTimelineItem[]>([])
  let request = 0

  async function reload() {
    const mine = ++request
    try {
      const list = await call(commands.addonTimeline(range.value.from, range.value.to))
      if (mine === request) items.value = list
    } catch {
      if (mine === request) items.value = []
    }
  }

  watch(range, reload, { immediate: true, deep: true })

  const unlisten = events.dataChanged.listen((e) => {
    if (e.payload.kinds.includes('timeline')) void reload()
  })
  onScopeDispose(() => void unlisten.then((fn) => fn()))

  return { items, reload }
}
