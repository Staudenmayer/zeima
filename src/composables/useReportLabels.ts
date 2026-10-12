import { useI18n } from 'vue-i18n'
import type { ReportGroup } from '@/bindings'
import { useFormat } from '@/composables/useFormat'
import { getWeekStart, isoWeek, parseDate } from '@/lib/time'
import { costCenterLabel, useCostCentersStore } from '@/stores/costCenters'
import { useProjectsStore } from '@/stores/projects'
import { useTagsStore } from '@/stores/tags'

/** Display label and color for a report row key. */
export function useReportLabels() {
  const { t } = useI18n()
  const format = useFormat()
  const projects = useProjectsStore()
  const tags = useTagsStore()
  const costCenters = useCostCentersStore()

  function label(group: ReportGroup, key: string | null): string {
    switch (group) {
      case 'project':
        return key ? (projects.byId.get(key)?.name ?? key) : t('common.noProject')
      case 'tag':
        return key ? (tags.byId.get(key)?.name ?? key) : t('reports.noTag')
      case 'costCenter':
        return key ? costCenterLabel(costCenters.byId.get(key)) || key : t('reports.noCostCenter')
      case 'billable':
        return key === 'true' ? t('reports.billable') : t('reports.notBillable')
      case 'day':
        return key ? format.day(parseDate(key)) : ''
      case 'week': {
        // Key: first day of the week. Monday weeks have ISO week numbers.
        const ms = key ? parseDate(key) : NaN
        if (!Number.isFinite(ms)) return key ?? ''
        if (getWeekStart() === 1) {
          const { week, year } = isoWeek(key!)
          return t('reports.weekLabel', { week, year })
        }
        return t('reports.weekOf', { date: format.shortDate(ms) })
      }
      case 'month':
        return key ? format.monthYear(parseDate(`${key}-01`)) || key : ''
    }
  }

  function color(group: ReportGroup, key: string | null): string | undefined {
    if (group === 'project') return key ? projects.byId.get(key)?.color : undefined
    if (group === 'tag') return key ? tags.byId.get(key)?.color : undefined
    return undefined
  }

  return { label, color }
}
