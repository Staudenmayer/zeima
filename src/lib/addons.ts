// Display helpers for addon permissions and hooks.

import type { Hook, Permission } from '@/bindings'

/** i18n key suffix per permission (keys cannot contain ':'). */
export const PERMISSION_KEYS: Record<Permission, string> = {
  'entries:read': 'entriesRead',
  'entries:write': 'entriesWrite',
  'projects:read': 'projectsRead',
  'tags:read': 'tagsRead',
  'cost-centers:read': 'costCentersRead',
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
  'report.export': 'reportExport',
  'quickswitch.suggest': 'quickswitchSuggest',
  'schedule.tick': 'scheduleTick',
}
