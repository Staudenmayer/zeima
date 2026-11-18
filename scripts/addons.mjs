#!/usr/bin/env node
// Signing and publishing addons (docs/addons.md, "Signing and the registry").
//
//   node scripts/addons.mjs keygen <key-id> [out-dir]
//   node scripts/addons.mjs sign <package.zip> --key <key-id>.key --key-id <key-id>
//   node scripts/addons.mjs verify <package.zip> --public <public key> [--key-id <key-id>]
//   node scripts/addons.mjs publish <registry-dir> --publisher <key-id>=<Name>=<public key> [...] <package.zip> [...]
//
// A package is signed in place: `addon.sig` is added to the zip. The signed
// message is "zeima-addon-v1\n<sha256 of addon.toml>\n<sha256 of the module>\n"
// (crates/zeima-core/src/addon_sig.rs, the same test vectors). Needs the
// `zip` and `unzip` programs. `publish` builds the folder the server serves
// (ADDON_REGISTRY_DIR): the zips and an `index.json`.

import { createHash, createPrivateKey, createPublicKey, generateKeyPairSync, sign as edSign, verify as edVerify } from 'node:crypto'
import { execFileSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const b64u = (buf) => Buffer.from(buf).toString('base64url')
const sha256 = (buf) => createHash('sha256').update(buf).digest('hex')

/** The bytes that are signed. */
export function message(manifest, wasm) {
  return Buffer.from(`zeima-addon-v1\n${sha256(manifest)}\n${sha256(wasm)}\n`)
}

/** The 32 raw bytes of an Ed25519 public key, base64url. */
export function publicKeyOf(privatePem) {
  const jwk = createPublicKey(createPrivateKey(privatePem)).export({ format: 'jwk' })
  return jwk.x
}

export function signFiles(manifest, wasm, privatePem, keyId) {
  const signature = b64u(edSign(null, message(manifest, wasm), createPrivateKey(privatePem)))
  return { keyId, signature }
}

export function verifyFiles(manifest, wasm, file, publicKey) {
  const key = createPublicKey({ key: { kty: 'OKP', crv: 'Ed25519', x: publicKey }, format: 'jwk' })
  return edVerify(null, message(manifest, wasm), key, Buffer.from(file.signature, 'base64url'))
}

/** The simple top-level keys of an addon.toml (strings and lists of strings). */
export function readManifest(src) {
  const top = src.split(/^\s*\[/m)[0]
  const out = {}
  for (const m of top.matchAll(/^\s*([a-z_]+)\s*=\s*("(?:[^"\\]|\\.)*"|\[[^\]]*\]|[^\n#]+)/gms)) {
    const raw = m[2].trim()
    if (raw.startsWith('[')) out[m[1]] = [...raw.matchAll(/"((?:[^"\\]|\\.)*)"/g)].map((x) => x[1])
    else if (raw.startsWith('"')) out[m[1]] = JSON.parse(raw)
    else out[m[1]] = raw
  }
  return out
}

function unzipFile(zip, name) {
  return execFileSync('unzip', ['-p', zip, name], { maxBuffer: 100 * 1024 * 1024 })
}

function packageFiles(zip) {
  const manifest = unzipFile(zip, 'addon.toml')
  const info = readManifest(manifest.toString('utf8'))
  if (!info.wasm) throw new Error('addon.toml has no wasm')
  return { manifest, wasm: unzipFile(zip, info.wasm), info }
}

function flag(args, name) {
  const i = args.indexOf(name)
  return i >= 0 ? args[i + 1] : undefined
}

function flags(args, name) {
  return args.flatMap((a, i) => (a === name ? [args[i + 1]] : []))
}

const commands = {
  keygen([keyId, dir = '.']) {
    if (!keyId) throw new Error('usage: keygen <key-id> [out-dir]')
    const { privateKey } = generateKeyPairSync('ed25519')
    const pem = privateKey.export({ type: 'pkcs8', format: 'pem' })
    const path = join(dir, `${keyId}.key`)
    mkdirSync(dir, { recursive: true })
    writeFileSync(path, pem, { mode: 0o600 })
    console.log(`private key: ${path}   (keep it secret)`)
    console.log(`public key:  ${publicKeyOf(pem)}`)
  },

  sign(args) {
    const [zip] = args
    const keyFile = flag(args, '--key')
    const keyId = flag(args, '--key-id')
    if (!zip || !keyFile || !keyId) throw new Error('usage: sign <package.zip> --key <file> --key-id <id>')
    const { manifest, wasm } = packageFiles(zip)
    const file = signFiles(manifest, wasm, readFileSync(keyFile, 'utf8'), keyId)
    const dir = mkdtempSync(join(tmpdir(), 'addon-sig-'))
    try {
      writeFileSync(join(dir, 'addon.sig'), JSON.stringify(file) + '\n')
      try {
        execFileSync('zip', ['-q', '-d', resolve(zip), 'addon.sig'], { stdio: 'ignore' })
      } catch {
        // not signed before
      }
      execFileSync('zip', ['-q', '-j', resolve(zip), join(dir, 'addon.sig')])
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
    console.log(`signed ${zip} as ${keyId}`)
  },

  verify(args) {
    const [zip] = args
    const publicKey = flag(args, '--public')
    if (!zip || !publicKey) throw new Error('usage: verify <package.zip> --public <key> [--key-id <id>]')
    const { manifest, wasm } = packageFiles(zip)
    const file = JSON.parse(unzipFile(zip, 'addon.sig').toString('utf8'))
    const wanted = flag(args, '--key-id')
    if (wanted && file.keyId !== wanted) throw new Error(`signed by ${file.keyId}, not ${wanted}`)
    if (!verifyFiles(manifest, wasm, file, publicKey)) throw new Error('the signature does not match')
    console.log(`ok: ${zip} is signed by ${file.keyId}`)
  },

  publish(args) {
    const [dir] = args
    const zips = args.slice(1).filter((a, i, all) => a.endsWith('.zip') && all[i - 1] !== '--publisher')
    if (!dir || zips.length === 0) throw new Error('usage: publish <registry-dir> [--publisher id=Name=key]... <package.zip>...')
    mkdirSync(dir, { recursive: true })
    const indexPath = join(dir, 'index.json')
    const index = existsSync(indexPath) ? JSON.parse(readFileSync(indexPath, 'utf8')) : { version: 1, publishers: [], addons: [] }
    for (const p of flags(args, '--publisher')) {
      const [keyId, name, publicKey] = p.split('=')
      if (!keyId || !name || !publicKey) throw new Error(`--publisher wants id=Name=key, got ${p}`)
      index.publishers = [...index.publishers.filter((x) => x.keyId !== keyId), { keyId, name, publicKey }]
    }
    for (const zip of zips) {
      const { info } = packageFiles(zip)
      let sig = null
      try {
        sig = JSON.parse(unzipFile(zip, 'addon.sig').toString('utf8'))
      } catch {
        // unsigned
      }
      const file = basename(zip)
      copyFileSync(zip, join(dir, file))
      const bytes = readFileSync(join(dir, file))
      const entry = {
        id: info.id,
        name: info.name ?? info.id,
        version: info.version,
        description: info.description ?? '',
        author: info.author ?? '',
        minAppVersion: info.min_app_version ?? null,
        permissions: info.permissions ?? [],
        hooks: info.hooks ?? [],
        file,
        sha256: sha256(bytes),
        size: statSync(join(dir, file)).size,
        keyId: sig?.keyId ?? null,
      }
      index.addons = [...index.addons.filter((a) => !(a.id === entry.id && a.version === entry.version)), entry]
      console.log(`published ${entry.id} ${entry.version}${sig ? ` (signed by ${sig.keyId})` : ' (unsigned)'}`)
    }
    index.addons.sort((a, b) => a.id.localeCompare(b.id) || a.version.localeCompare(b.version, undefined, { numeric: true }))
    writeFileSync(indexPath, JSON.stringify(index, null, 2) + '\n')
  },
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [name, ...rest] = process.argv.slice(2)
  try {
    if (!commands[name]) throw new Error(`commands: ${Object.keys(commands).join(', ')}`)
    commands[name](rest)
  } catch (err) {
    console.error(err.message)
    process.exit(1)
  }
}
