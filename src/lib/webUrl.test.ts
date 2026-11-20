import { describe, expect, it } from 'vitest'
import { webUrl } from './webUrl'

describe('webUrl', () => {
  it('accepts http and https addresses', () => {
    expect(webUrl('https://meet.example.com/room')).toBe('https://meet.example.com/room')
    expect(webUrl(' http://meet.example.com/a ')).toBe('http://meet.example.com/a')
  })

  it('refuses other schemes and things that are not addresses', () => {
    expect(webUrl('file:///etc/passwd')).toBeNull()
    expect(webUrl('javascript:alert(1)')).toBeNull()
    expect(webUrl('ms-msdt:/id x')).toBeNull()
    expect(webUrl('meet.example.com/room')).toBeNull()
    expect(webUrl('')).toBeNull()
    expect(webUrl(null)).toBeNull()
  })
})
