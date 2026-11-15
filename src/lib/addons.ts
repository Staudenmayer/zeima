// Display helpers for addon permissions and hooks.

import type { Hook, Permission } from '@/bindings'

/** i18n key suffix per permission (keys cannot contain ':'). */
export const PERMISSION_KEYS: Record<Permission, string> = {
  'entries:read': 'entriesRead',
  'entries:write': 'entriesWrite',
  'projects:read': 'projectsRead',
  'activities:read': 'activitiesRead',
  'tags:read': 'tagsRead',
  'cost-centers:read': 'costCentersRead',
  'meetings:read': 'meetingsRead',
  'absences:read': 'absencesRead',
  'work:read': 'workRead',
  notify: 'notify',
  network: 'network',
}

/** i18n key suffix per hook. */
export const HOOK_KEYS: Record<Hook, string> = {
  'entry.started': 'entryStarted',
  'entry.stopped': 'entryStopped',
  'entry.created': 'entryCreated',
  'entry.updated': 'entryUpdated',
  'entry.deleted': 'entryDeleted',
  'day.closed': 'dayClosed',
  'meeting.changed': 'meetingChanged',
  'absence.changed': 'absenceChanged',
  'report.export': 'reportExport',
  'quickswitch.suggest': 'quickswitchSuggest',
  'schedule.tick': 'scheduleTick',
  'timeline.events': 'timelineEvents',
}
