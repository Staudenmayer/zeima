#!/usr/bin/env node
// Checks that an updater signature was made by the key a public key belongs to.
//
//   node scripts/verify-updater-key.mjs <file.sig> <public-key>
//
// <file.sig> is what `tauri signer sign` writes next to the signed file; the
// public key is a path to the .pub file or the base64 string from
// tauri.conf.json (plugins.updater.pubkey). Both are minisign formats. Prints
// "OK" and exits 0 only if the signature verifies with that public key, which
// means the pair matches and the password you signed with was right.
import { createHash, createPublicKey, verify } from 'node:crypto'
import { existsSync, readFileSync } from 'node:fs'

const [sigArg, pubArg] = process.argv.slice(2)
if (!sigArg || !pubArg) {
  console.error('usage: node scripts/verify-updater-key.mjs <file.sig> <public-key file or string>')
  process.exit(2)
}

const text = (b64) => Buffer.from(b64.trim(), 'base64').toString('utf8')
const lines = (s) => s.split('\n').filter(Boolean)
const fail = (msg) => {
  console.error(`FAIL: ${msg}`)
  process.exit(1)
}

const pubText = text(existsSync(pubArg) ? readFileSync(pubArg, 'utf8') : pubArg)
const sigText = text(readFileSync(sigArg, 'utf8'))

const pub = Buffer.from(lines(pubText)[1] ?? '', 'base64') // "Ed" + key id (8) + key (32)
const sig = Buffer.from(lines(sigText)[1] ?? '', 'base64') // algorithm (2) + key id (8) + signature (64)
if (pub.length !== 42 || sig.length !== 74) fail('not a minisign public key / signature')

const keyId = (b) => Buffer.from(b.subarray(2, 10)).reverse().toString('hex').toUpperCase()
console.log(`public key id: ${keyId(pub)}`)
console.log(`signature key id: ${keyId(sig)}`)
if (keyId(pub) !== keyId(sig)) fail('the signature was made by a different key')

// The signed data is the file itself, or its BLAKE2b-512 hash ("ED" signatures).
const file = sigArg.replace(/\.sig$/, '')
if (!existsSync(file)) fail(`the signed file ${file} is not next to the .sig`)
const content = readFileSync(file)
const message = sig.subarray(0, 2).toString() === 'ED' ? createHash('blake2b512').update(content).digest() : content

const key = createPublicKey({ key: { kty: 'OKP', crv: 'Ed25519', x: pub.subarray(10).toString('base64url') }, format: 'jwk' })
if (!verify(null, message, key, sig.subarray(10))) fail('the signature does not verify')
console.log('OK: the signature verifies with this public key')
