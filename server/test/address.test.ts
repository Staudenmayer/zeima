import { describe, expect, it } from 'vitest'
import { privateAddress } from '../src/modules/orgs/sso.ts'

describe('the private-address filter of the single sign-on fetch', () => {
  it('refuses every special-use address, in every notation', () => {
    for (const ip of [
      '127.0.0.1', '10.1.2.3', '169.254.169.254', '100.64.0.1', '198.18.0.1', '192.0.0.8', '192.0.2.1', '240.0.0.1',
      '::1', '::', '::127.0.0.1', '::ffff:127.0.0.1', '::ffff:7f00:1', '::ffff:a9fe:a9fe', '0:0:0:0:0:ffff:7f00:1', '::ffff:0:7f00:1',
      '64:ff9b::a9fe:a9fe', '64:ff9b::7f00:1', 'fec0::1', 'fc00::1', 'fe80::1', '2002:7f00:1::', '2001:db8::1', '2001:0:1::1', 'ff02::1', 'not an address',
    ]) {
      expect(privateAddress(ip), ip).toBe(true)
    }
  })
  it('accepts ordinary public addresses', () => {
    for (const ip of ['93.184.216.34', '8.8.8.8', '2606:2800:220:1:248:1893:25c8:1946', '64:ff9b::808:808', '::ffff:8.8.8.8']) {
      expect(privateAddress(ip), ip).toBe(false)
    }
  })
})
