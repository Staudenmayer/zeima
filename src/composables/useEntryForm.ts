import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { commands, type EntryInput, type TimeEntry } from '@/bindings'
import { call, errorMessage } from '@/lib/ipc'
import { HOUR, addDays, daysBetween, fromInputs, parseDate, rangeFromDates, toDateInput, toTimeInput } from '@/lib/time'
import { useProjectsStore } from '@/stores/projects'
import { useTagsStore } from '@/stores/tags'

/**
 * Form state for creating or editing an entry, shared by the entry dialog
 * and the calendar's edit popover.
 *
 * @param entry  the entry to edit; `null` creates a new one
 * @param day    day preselected for new entries (unix ms)
 */
export function useEntryForm(entry: () => TimeEntry | null, day: () => number | undefined = () => undefined) {
  const { t } = useI18n()
  const projects = useProjectsStore()
  const tags = useTagsStore()

  const activityId = ref<string | null>(null)
  /** Set after a save attempt without activity, to highlight the field. */
  const activityMissing = ref(false)
  const projectId = ref<string | null>(null)
  const tagIds = ref<string[]>([])
  /** Start date; `endDate` differs for entries spanning several days. */
  const date = ref('')
  const endDate = ref('')
  const start = ref('')
  const end = ref('')
  const note = ref('')
  const billable = ref(false)
  /** `null` = automatic (project default, then the user's default). */
  const costCenterId = ref<string | null>(null)
  /** Picked by the user; otherwise it is resolved again on save. */
  const costCenterTouched = ref(false)
  const saving = ref(false)

  const running = computed(() => {
    const e = entry()
    return e !== null && e.endAt === null
  })

  /** Set while values are loaded, so the date watcher leaves them alone. */
  let loading = false

  /** Loads the entry's values, or defaults for a new entry. */
  function reset() {
    loading = true
    try {
      load()
    } finally {
      loading = false
    }
  }

  function load() {
    const e = entry()
    activityMissing.value = false
    if (e) {
      activityId.value = e.activityId
      projectId.value = e.projectId
      tagIds.value = [...e.tagIds]
      date.value = toDateInput(e.startAt)
      endDate.value = toDateInput(e.endAt ?? e.startAt)
      start.value = toTimeInput(e.startAt)
      end.value = e.endAt === null ? '' : toTimeInput(e.endAt)
      note.value = e.note
      billable.value = e.billable
      costCenterId.value = e.costCenterId
    } else {
      // New entry: the hour before now, or 09:00–10:00 on another day.
      const now = Date.now()
      const d = day() ?? now
      const isToday = toDateInput(d) === toDateInput(now)
      const startAt = isToday ? now - HOUR : fromInputs(toDateInput(d), '09:00')
      activityId.value = null
      projectId.value = null
      tagIds.value = []
      date.value = toDateInput(d)
      endDate.value = toDateInput(startAt + HOUR)
      start.value = toTimeInput(startAt)
      end.value = toTimeInput(startAt + HOUR)
      note.value = ''
      billable.value = false
      costCenterId.value = null
    }
    costCenterTouched.value = false
  }

  // The user moving the start date moves the end date along (same length).
  // Synchronous, so loading values (flagged) is never mistaken for that.
  watch(
    date,
    (next, prev) => {
      if (loading) return
      const from = parseDate(prev)
      const to = parseDate(next)
      const end = parseDate(endDate.value)
      if (!Number.isFinite(from) || !Number.isFinite(to) || !Number.isFinite(end)) return
      endDate.value = toDateInput(addDays(end, daysBetween(from, to)))
    },
    { flush: 'sync' },
  )

  /** Sets dates and times at once (e.g. from a calendar drag). */
  function setTimes(startAt: number, endAt: number | null) {
    loading = true
    try {
      date.value = toDateInput(startAt)
      start.value = toTimeInput(startAt)
      if (endAt !== null) {
        endDate.value = toDateInput(endAt)
        end.value = toTimeInput(endAt)
      }
    } finally {
      loading = false
    }
  }

  // Tags of another project do not apply: drop them in the same change,
  // so nothing (e.g. an autosave) ever sees the invalid combination.
  watch(
    projectId,
    (id) => {
      const usable = new Set(tags.usableFor(id).map((t) => t.id))
      if (tagIds.value.some((t) => !usable.has(t))) tagIds.value = tagIds.value.filter((t) => usable.has(t))
    },
    { flush: 'sync' },
  )

  // New entries take the project's billable default.
  watch(projectId, (id) => {
    if (entry()) return
    billable.value = id ? (projects.byId.get(id)?.billableDefault ?? false) : false
  })

  watch(activityId, (id) => {
    if (id) activityMissing.value = false
  })

  function setCostCenter(id: string | null) {
    costCenterId.value = id
    costCenterTouched.value = true
  }

  type Built = { input: EntryInput } | { error: 'activity' | 'time' }

  /** The form as entry input, or what is wrong with it. */
  function build(): Built {
    const e = entry()
    if (!activityId.value) return { error: 'activity' }
    let startAt: number
    let endAt: number | null
    if (running.value) {
      startAt = fromInputs(date.value, start.value)
      endAt = null
    } else {
      ;({ startAt, endAt } = rangeFromDates(date.value, start.value, endDate.value || date.value, end.value))
    }
    if (Number.isNaN(startAt) || (endAt !== null && (Number.isNaN(endAt) || endAt <= startAt))) {
      return { error: 'time' }
    }
    return {
      input: {
        activityId: activityId.value,
        projectId: projectId.value,
        startAt,
        endAt,
        tz: null,
        note: note.value,
        billable: billable.value,
        tagIds: tagIds.value,
        // Untouched: keep the entry's cost center unless the project changed.
        costCenterId: costCenterTouched.value
          ? costCenterId.value
          : e && e.projectId === projectId.value
            ? e.costCenterId
            : null,
      },
    }
  }

  /** Times that cannot be saved (e.g. the end before the start). */
  const timeInvalid = computed(() => {
    const b = build()
    return 'error' in b && b.error === 'time'
  })

  /** The form differs from the stored entry (and is valid). */
  function changed(): boolean {
    const e = entry()
    const b = build()
    if (!e || 'error' in b) return false
    const i = b.input
    const sameTags = [...i.tagIds].sort().join() === [...e.tagIds].sort().join()
    return (
      i.activityId !== e.activityId ||
      i.projectId !== e.projectId ||
      i.startAt !== e.startAt ||
      i.endAt !== e.endAt ||
      i.note.trim() !== e.note ||
      i.billable !== e.billable ||
      !sameTags ||
      (costCenterTouched.value && i.costCenterId !== e.costCenterId)
    )
  }

  /** Validates and writes the entry. Returns whether it was saved. */
  async function save(): Promise<boolean> {
    const e = entry()
    const b = build()
    if ('error' in b) {
      if (b.error === 'activity') {
        activityMissing.value = true
        toast.error(t('activities.required'))
      } else {
        toast.error(t('entry.invalidTime'))
      }
      return false
    }
    saving.value = true
    try {
      if (e) await call(commands.entryUpdate(e.id, b.input))
      else await call(commands.entryCreate(b.input))
      return true
    } catch (err) {
      toast.error(errorMessage(err))
      return false
    } finally {
      saving.value = false
    }
  }

  return {
    activityId,
    activityMissing,
    projectId,
    tagIds,
    date,
    endDate,
    start,
    end,
    note,
    billable,
    costCenterId,
    saving,
    running,
    timeInvalid,
    changed,
    reset,
    setTimes,
    setCostCenter,
    save,
  }
}
