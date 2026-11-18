import { execFileSync, spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
// @ts-expect-error plain JS tool
import { readManifest, verifyFiles } from '../../scripts/addons.mjs'
import vectors from '../../packages/shared-fixtures/addons/vectors.json' with { type: 'json' }

const tool = join(import.meta.dirname, '../../scripts/addons.mjs')
const run = (...args: string[]) => execFileSync('node', [tool, ...args], { encoding: 'utf8' })
const haveZip = spawnSync('zip', ['-v']).status === 0 && spawnSync('unzip', ['-v']).status === 0

describe('addon signatures (shared test vectors)', () => {
  for (const c of vectors.cases) {
    it(c.name, () => {
      const wasm = Buffer.from(c.wasmBase64, 'base64')
      const ok = c.signature ? verifyFiles(Buffer.from(c.manifest), wasm, c.signature, vectors.key.publicKey) : false
      // The tool only knows one key: a different key id is the app's business, the maths still holds.
      expect(ok).toBe(c.expect === 'trusted' || c.expect === 'unknown_key')
    })
  }
})

describe('reading a manifest', () => {
  it('takes the top-level keys and lists, not the tables', () => {
    const m = readManifest('id = "a.b"\nname = "X"\nversion = "1.2.3"\npermissions = [\n "notify",\n "entries:read",\n]\nhooks = ["report.export"]\n[[export_formats]]\nid = "no"\n')
    expect(m).toMatchObject({ id: 'a.b', name: 'X', version: '1.2.3', permissions: ['notify', 'entries:read'], hooks: ['report.export'] })
    expect(m.export_formats).toBeUndefined()
  })
})

describe.skipIf(!haveZip)('the tool', () => {
  it('signs a package, verifies it, and publishes a registry', () => {
    const dir = mkdtempSync(join(tmpdir(), 'addons-tool-'))
    try {
      writeFileSync(join(dir, 'addon.toml'), 'id = "dev.example.demo"\nname = "Demo"\nversion = "1.0.0"\nauthor = "Me"\nwasm = "demo.wasm"\npermissions = ["notify"]\nhooks = ["schedule.tick"]\n')
      writeFileSync(join(dir, 'demo.wasm'), Buffer.from([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0]))
      execFileSync('zip', ['-q', 'demo.zip', 'addon.toml', 'demo.wasm'], { cwd: dir })
      const publicKey = /public key:\s+(\S+)/.exec(run('keygen', 'test-key', dir))![1]!
      run('sign', join(dir, 'demo.zip'), '--key', join(dir, 'test-key.key'), '--key-id', 'test-key')
      expect(run('verify', join(dir, 'demo.zip'), '--public', publicKey, '--key-id', 'test-key')).toContain('signed by test-key')
      // Signing again replaces the signature.
      run('sign', join(dir, 'demo.zip'), '--key', join(dir, 'test-key.key'), '--key-id', 'test-key')
      // Another key does not verify.
      const other = /public key:\s+(\S+)/.exec(run('keygen', 'other', dir))![1]!
      expect(() => run('verify', join(dir, 'demo.zip'), '--public', other)).toThrow()

      run('publish', join(dir, 'registry'), '--publisher', `test-key=Test Publisher=${publicKey}`, join(dir, 'demo.zip'))
      const index = JSON.parse(readFileSync(join(dir, 'registry', 'index.json'), 'utf8'))
      expect(index.publishers).toEqual([{ keyId: 'test-key', name: 'Test Publisher', publicKey }])
      expect(index.addons).toHaveLength(1)
      expect(index.addons[0]).toMatchObject({ id: 'dev.example.demo', version: '1.0.0', file: 'demo.zip', keyId: 'test-key', permissions: ['notify'], hooks: ['schedule.tick'], author: 'Me' })
      expect(index.addons[0].sha256).toMatch(/^[0-9a-f]{64}$/)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
})
