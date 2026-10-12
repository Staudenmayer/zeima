import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  commands,
  type AbsenceType,
  type Balance,
  type JurisdictionSelection,
  type ProfileInfo,
  type ProfileSummary,
  type WorkSchedule,
  type WorkSettings,
} from '@/bindings'
import { call } from '@/lib/ipc'
import i18n from '@/plugins/i18n'

/** Country profile, schedules, work settings and balance. */
export const useWorkStore = defineStore('work', () => {
  const profiles = ref<ProfileInfo[]>([])
  const jurisdiction = ref<JurisdictionSelection | null>(null)
  const summary = ref<ProfileSummary | null>(null)
  const settings = ref<WorkSettings | null>(null)
  const schedules = ref<WorkSchedule[]>([])
  const absenceTypes = ref<AbsenceType[]>([])
  const balance = ref<Balance | null>(null)
  const loaded = ref(false)

  const needsOnboarding = computed(() => loaded.value && jurisdiction.value === null)
  const absenceTypesById = computed(() => new Map(absenceTypes.value.map((t) => [t.id, t])))

  const lang = () => i18n.global.locale.value

  async function load() {
    const [p, j, s, w, sch, types] = await Promise.all([
      commands.profileList(),
      call(commands.jurisdictionGet()),
      call(commands.profileSummary(lang())),
      call(commands.workSettingsGet()),
      call(commands.scheduleList()),
      call(commands.absenceTypeList()),
    ])
    profiles.value = p
    jurisdiction.value = j
    summary.value = s
    settings.value = w
    schedules.value = sch
    absenceTypes.value = types
    loaded.value = true
    await loadBalance()
  }

  async function loadBalance() {
    balance.value = await call(commands.balance())
  }

  /** Display name of a built-in or custom absence type. */
  function absenceTypeName(t: AbsenceType): string {
    const key = `absences.builtin.${t.key}`
    return t.key && i18n.global.te(key) ? i18n.global.t(key) : t.name
  }

  return {
    profiles,
    jurisdiction,
    summary,
    settings,
    schedules,
    absenceTypes,
    absenceTypesById,
    balance,
    loaded,
    needsOnboarding,
    load,
    loadBalance,
    absenceTypeName,
  }
})

/** Localized name from a `{ de: …, en: … }` map. */
export function localized(text: Partial<Record<string, string>>): string {
  const lang = i18n.global.locale.value
  return text[lang] ?? text.en ?? Object.values(text)[0] ?? ''
}
