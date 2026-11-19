import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
// @ts-expect-error plain JS tool, shared with the command line
import { appCommands, capabilities } from '../../scripts/tauri-capabilities.mjs'

const dir = join(import.meta.dirname, '../../src-tauri/capabilities')
const committed = (name: string) => JSON.parse(readFileSync(join(dir, name), 'utf8')) as { windows: string[]; permissions: string[] }
const allow = (c: string) => `allow-${c.replace(/_/g, '-')}`

describe('what each window may call (docs/security.md, ZE-14)', () => {
  const made = capabilities() as Record<string, { permissions: string[] }>

  it('is what scripts/tauri-capabilities.mjs makes: run it again when a command or an import changes', () => {
    expect(committed('main.json')).toEqual(made['main.json'])
    expect(committed('bare.json')).toEqual(made['bare.json'])
  })

  it('lets the main window call every command', () => {
    const main = committed('main.json')
    expect(main.windows).toEqual(['main'])
    for (const c of appCommands() as string[]) expect(main.permissions, c).toContain(allow(c))
  })

  it('keeps the small windows to what they need: no addon install, no organization API, no data export, no purchases', () => {
    const bare = committed('bare.json')
    expect(bare.windows.sort()).toEqual(['prompt', 'quickswitch'])
    for (const c of ['addon_install', 'addon_registry_install', 'addon_pick', 'addon_uninstall', 'org_request', 'account_sign_in', 'data_export_json', 'import_run', 'billing_checkout', 'sync_now', 'backup_open_dir', 'report_export_addon']) {
      expect(bare.permissions, c).not.toContain(allow(c))
    }
    // ... and still has what it does: the timer, the lists the quick switch shows, hiding itself.
    for (const c of ['timer_start', 'timer_stop', 'activity_list', 'project_list', 'quickswitch_hide', 'prompt_hide', 'away_resolve', 'prompt_state']) {
      expect(bare.permissions, c).toContain(allow(c))
    }
    // No plugin access (opening links) for the small windows.
    expect(bare.permissions.some((p) => p.startsWith('opener:'))).toBe(false)
  })

  it('has no window that is not described here', () => {
    expect(committed('main.json').windows.concat(committed('bare.json').windows).sort()).toEqual(['main', 'prompt', 'quickswitch'])
  })
})
