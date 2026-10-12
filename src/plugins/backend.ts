// Loads initial state and keeps stores in sync with backend events.

import type { Router } from 'vue-router'
import { events } from '@/bindings'
import { useActivitiesStore } from '@/stores/activities'
import { useCostCentersStore } from '@/stores/costCenters'
import { useProjectsStore } from '@/stores/projects'
import { useSettingsStore } from '@/stores/settings'
import { useTagsStore } from '@/stores/tags'
import { useTimerStore } from '@/stores/timer'
import { useWorkStore } from '@/stores/work'
import { applyLocale } from '@/plugins/i18n'

export async function connectBackend(router: Router) {
  const activities = useActivitiesStore()
  const projects = useProjectsStore()
  const tags = useTagsStore()
  const timer = useTimerStore()
  const settings = useSettingsStore()
  const work = useWorkStore()
  const costCenters = useCostCentersStore()

  await events.timerChanged.listen((e) => timer.set(e.payload))
  await events.dataChanged.listen(async (e) => {
    const kinds = e.payload.kinds
    if (kinds.includes('activity')) await activities.load()
    if (kinds.includes('project')) await projects.load()
    if (kinds.includes('tag')) await tags.load()
    if (kinds.includes('costCenter')) await costCenters.load()
    if (kinds.includes('work')) await work.load()
    else if (kinds.includes('entry')) await work.loadBalance()
    if (kinds.includes('settings')) {
      // A new time zone or week start changes every date computed so far:
      // start over with the new values.
      if (await settings.load()) {
        window.location.reload()
        return
      }
      applyLocale(settings.settings?.locale ?? null)
      await work.load()
    }
  })
  await events.navigate.listen((e) => void router.push(e.payload.path))

  await Promise.all([activities.load(), projects.load(), tags.load(), costCenters.load(), timer.load(), settings.load()])
  applyLocale(settings.settings?.locale ?? null)
  // Work data uses the UI language (profile and holiday names).
  await work.load()
}
