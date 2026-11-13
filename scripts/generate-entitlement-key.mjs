#!/usr/bin/env node
// Generates an Ed25519 key pair for signing entitlement tokens (plans and
// licenses, SPEC §2). Run it from anywhere; it stores nothing unless asked.
//
//   node scripts/generate-entitlement-key.mjs                     print both keys
//   node scripts/generate-entitlement-key.mjs --private-file KEY  write the private key to a
//                                                                 file (mode 600) instead of
//                                                                 printing it
//
// Where the values go:
//   ENTITLEMENT_SIGNING_KEY  (private) the cloud server's secret store; never the repository
//   ZEIMA_ENTITLEMENT_KEYS   (public)  the app build, e.g.
//     gh variable set ZEIMA_ENTITLEMENT_KEYS --repo <owner>/<repo> --body "<public key>"
//   ENTITLEMENT_PUBLIC_KEYS  (public)  self-hosted servers that accept licenses you sign
// Back up the private key: if it is lost, every license must be issued again.
import { generateKeyPairSync } from 'node:crypto'
import { writeFileSync } from 'node:fs'

const args = process.argv.slice(2)
const fileFlag = args.indexOf('--private-file')
const privateFile = fileFlag >= 0 ? args[fileFlag + 1] : null
if (args.includes('--help') || args.includes('-h') || (fileFlag >= 0 && !privateFile)) {
  console.log('usage: node scripts/generate-entitlement-key.mjs [--private-file <path>]')
  process.exit(args.includes('--help') || args.includes('-h') ? 0 : 2)
}

const { privateKey, publicKey } = generateKeyPairSync('ed25519')
const signingKey = privateKey.export({ type: 'pkcs8', format: 'der' }).toString('base64url')
const publicKeyRaw = publicKey.export({ format: 'jwk' }).x

if (privateFile) {
  // 'wx': refuse to overwrite an existing key.
  writeFileSync(privateFile, `${signingKey}\n`, { mode: 0o600, flag: 'wx' })
  console.log(`ENTITLEMENT_SIGNING_KEY written to ${privateFile} (keep it secret, back it up)`)
} else {
  console.log(`ENTITLEMENT_SIGNING_KEY (private, keep secret): ${signingKey}`)
}
console.log(`ZEIMA_ENTITLEMENT_KEYS (public): ${publicKeyRaw}`)
