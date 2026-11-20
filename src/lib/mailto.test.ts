import { describe, expect, it } from 'vitest'
import { mailtoUrl } from './mailto'

describe('mailtoUrl', () => {
  it('links a plain address', () => {
    expect(mailtoUrl('ann.lee+x@example.com')).toBe('mailto:ann.lee%2Bx@example.com')
  })

  it('refuses an address that would add parameters', () => {
    expect(mailtoUrl('a@b?attach=/x')).toBeNull()
    expect(mailtoUrl('a@b.com?bcc=c@d.com')).toBeNull()
    expect(mailtoUrl('a@b.com&cc=c@d.com')).toBeNull()
    expect(mailtoUrl('a b@c.com')).toBeNull()
    expect(mailtoUrl('')).toBeNull()
  })

  it('never yields a query for an address that is accepted', () => {
    for (const a of ["o'brien@example.org", 'a&b@example.com', 'a%3Fb@example.com']) {
      const url = mailtoUrl(a)
      if (url) expect(url).not.toMatch(/[?&]/)
    }
  })
})
