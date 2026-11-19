#!/usr/bin/env node
// Derives the value types of every synced table from the app's generated TypeScript bindings (src/bindings.ts,
// which specta writes from the Rust models) into server/src/modules/sync/schema.generated.json, so the server can
// check that a pushed field has the type its table says. `--check` fails when the file is out of date (CI/tests).
import { readFileSync, writeFileSync } from 'node:fs'

const root = new URL('..', import.meta.url)
const source = readFileSync(new URL('src/bindings.ts', root), 'utf8')
const uncommented = source.replace(/\/\*[\s\S]*?\*\//g, '')
const out = new URL('server/src/modules/sync/schema.generated.json', root)

/** Wire table → the model type the app serializes for it. */
const TABLES = {
  project: 'Project', tag: 'Tag', time_entry: 'TimeEntry', pause: 'Pause', activity: 'Activity', cost_center: 'CostCenter',
  cost_center_assignment: 'CostCenterAssignment', jurisdiction_selection: 'JurisdictionSelection', work_settings: 'WorkSettings',
  work_schedule: 'WorkSchedule', absence_type: 'AbsenceType', absence: 'Absence', custom_holiday: 'CustomHoliday',
  balance_adjustment: 'BalanceAdjustment', display_settings: 'DisplaySettings', work_location: 'WorkLocation', org_policy: 'OrgPolicy',
  office: 'Office', room: 'Room', meeting: 'Meeting', meeting_response: 'MeetingResponse',
}

function block(name) {
  const m = new RegExp(`export type ${name} = \\{([\\s\\S]*?)\\n\\}`).exec(source)
  return m ? m[1] : null
}

/** `export type Status = "a" | "b"`: a named string union counts as a string. */
function isStringAlias(name) {
  const m = new RegExp(`export type ${name} = ([^;]*);`).exec(uncommented)
  if (!m) return false
  const body = m[1]
  return /^\s*"[^"]*"(\s*\|\s*"[^"]*")*\s*$/.test(body)
}

function kind(type) {
  const t = type.replace(/\s*\|\s*null/g, '').trim()
  if (/^[A-Z][A-Za-z0-9]*$/.test(t) && isStringAlias(t)) return 'string'
  if (t === 'number') return 'number'
  if (t === 'boolean') return 'boolean'
  if (t === 'string[]') return 'strings'
  if (/^("[^"]*"|string)(\s*\|\s*("[^"]*"|string))*$/.test(t)) return 'string'
  return 'json'
}

const schema = {}
for (const [table, type] of Object.entries(TABLES)) {
  const body = block(type)
  if (body === null) {
    // Display settings are per device and never synced into an organization; nothing to check.
    if (table === 'display_settings') { schema[table] = {}; continue }
    throw new Error(`type ${type} not found in bindings.ts`)
  }
  const fields = {}
  const text = body.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/.*$/gm, '')
  for (const m of text.matchAll(/^\s*([A-Za-z][A-Za-z0-9]*)\??:\s*([^\n]+?),?\s*$/gm)) {
    fields[m[1]] = { type: kind(m[2]), nullable: /\|\s*null|\?:/.test(m[0]) }
  }
  schema[table] = fields
}
const json = `${JSON.stringify(schema, null, 2)}\n`
if (process.argv.includes('--check')) {
  if (readFileSync(out, 'utf8') !== json) {
    console.error('server/src/modules/sync/schema.generated.json is out of date: run `node scripts/sync-schema.mjs`')
    process.exit(1)
  }
} else writeFileSync(out, json)
