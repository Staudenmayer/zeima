#!/usr/bin/env node
// Keeps the app version in one place per ecosystem and checks they agree.
//
//   node scripts/version.mjs check [vX.Y.Z]   fail if the files disagree
//                                             (or with the given tag)
//   node scripts/version.mjs set X.Y.Z        write the version everywhere
//
// Sources: [workspace.package] in Cargo.toml (src-tauri/tauri.conf.json has no
// "version", so Tauri and the in-app "About" use the Cargo one), and the
// package.json files. Cargo.lock is refreshed by `set`.
import { execSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const SEMVER =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+[0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*)?$/
const packageJsons = ['package.json', 'server/package.json', 'packages/api-contract/package.json']
const cargoToml = join(root, 'Cargo.toml')
const tauriConf = join(root, 'src-tauri/tauri.conf.json')

const WORKSPACE_VERSION = /(\[workspace\.package\][^[]*?\nversion\s*=\s*")([^"]*)(")/

function read(file) {
  return readFileSync(join(root, file), 'utf8')
}

function versions() {
  const found = {}
  found['Cargo.toml'] = read('Cargo.toml').match(WORKSPACE_VERSION)?.[2]
  for (const file of packageJsons) found[file] = JSON.parse(read(file)).version
  const tauri = JSON.parse(readFileSync(tauriConf, 'utf8'))
  if (tauri.version !== undefined) {
    found['src-tauri/tauri.conf.json'] = tauri.version
  }
  return found
}

const [cmd, arg] = process.argv.slice(2)

if (cmd === 'check') {
  const found = versions()
  const expected = arg ? arg.replace(/^v/, '') : found['Cargo.toml']
  const errors = []
  if (!SEMVER.test(expected ?? '')) errors.push(`"${expected}" is not a semantic version`)
  for (const [file, version] of Object.entries(found)) {
    if (version !== expected) errors.push(`${file} has ${version}, expected ${expected}`)
  }
  if (errors.length) {
    console.error(errors.join('\n'))
    console.error('Run: node scripts/version.mjs set <version>')
    process.exit(1)
  }
  console.log(`All versions are ${expected}`)
} else if (cmd === 'set') {
  const version = (arg ?? '').replace(/^v/, '')
  if (!SEMVER.test(version)) {
    console.error('Usage: node scripts/version.mjs set X.Y.Z[-prerelease]')
    process.exit(1)
  }
  writeFileSync(cargoToml, read('Cargo.toml').replace(WORKSPACE_VERSION, `$1${version}$3`))
  for (const file of packageJsons) {
    const json = JSON.parse(read(file))
    json.version = version
    writeFileSync(join(root, file), JSON.stringify(json, null, 2) + '\n')
  }
  // Refresh the workspace crates' entries in Cargo.lock without upgrading anything.
  execSync('cargo update --workspace --offline', { cwd: root, stdio: 'inherit' })
  console.log(`Set version ${version}`)
} else {
  console.error('Usage: node scripts/version.mjs check [tag] | set X.Y.Z')
  process.exit(1)
}
