// Builds crates/zeima-core-wasm and generates the Node bindings in ./pkg.
// Needs the wasm32 target and the wasm-bindgen CLI matching the crate:
//   rustup target add wasm32-unknown-unknown
//   cargo install wasm-bindgen-cli --version 0.2.100 --locked
import { execFileSync } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const root = join(here, '../..')
const run = (cmd, args) => execFileSync(cmd, args, { cwd: root, stdio: 'inherit' })

run('cargo', ['build', '--release', '--target', 'wasm32-unknown-unknown', '-p', 'zeima-core-wasm'])
run('wasm-bindgen', [
  'target/wasm32-unknown-unknown/release/zeima_core_wasm.wasm',
  '--target', 'nodejs',
  '--out-dir', join(here, 'pkg'),
  '--out-name', 'zeima_core_wasm',
])
console.log('built packages/core-wasm/pkg')
