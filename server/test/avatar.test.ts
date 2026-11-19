import { deflateSync } from 'node:zlib'
import { describe, expect, it } from 'vitest'
import { checkAvatar, imageInfo } from '../src/modules/me/image.ts'

function pngOf(width: number, height: number) {
  const chunk = (type: string, data: Buffer) => {
    const len = Buffer.alloc(4)
    len.writeUInt32BE(data.length)
    return Buffer.concat([len, Buffer.from(type), data, Buffer.alloc(4)])
  }
  const ihdr = Buffer.alloc(13)
  ihdr.writeUInt32BE(width, 0)
  ihdr.writeUInt32BE(height, 4)
  ihdr[8] = 8
  ihdr[9] = 2
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(Buffer.alloc(0))), chunk('IEND', Buffer.alloc(0))])
}

function jpegOf(width: number, height: number) {
  const sof = Buffer.from([0xff, 0xc0, 0x00, 0x0b, 0x08, height >> 8, height & 255, width >> 8, width & 255, 0x01, 0x01, 0x11, 0x00])
  const app0 = Buffer.from([0xff, 0xe0, 0x00, 0x04, 0x00, 0x00])
  return Buffer.concat([Buffer.from([0xff, 0xd8]), app0, sof, Buffer.from([0xff, 0xd9])])
}

function webpLossy(width: number, height: number) {
  const body = Buffer.alloc(10)
  body.set([0x00, 0x00, 0x00, 0x9d, 0x01, 0x2a], 0)
  body.writeUInt16LE(width, 6)
  body.writeUInt16LE(height, 8)
  const chunk = Buffer.concat([Buffer.from('VP8 '), Buffer.from([10, 0, 0, 0]), body])
  const header = Buffer.concat([Buffer.from('RIFF'), Buffer.from([chunk.length + 4, 0, 0, 0]), Buffer.from('WEBP')])
  return Buffer.concat([header, chunk, Buffer.alloc(4)])
}

function webpExtended(width: number, height: number) {
  const body = Buffer.alloc(10)
  body.writeUIntLE(width - 1, 4, 3)
  body.writeUIntLE(height - 1, 7, 3)
  const chunk = Buffer.concat([Buffer.from('VP8X'), Buffer.from([10, 0, 0, 0]), body])
  return Buffer.concat([Buffer.from('RIFF'), Buffer.from([chunk.length + 4, 0, 0, 0]), Buffer.from('WEBP'), chunk, Buffer.alloc(8)])
}

const url = (type: string, bytes: Buffer) => `data:image/${type};base64,${bytes.toString('base64')}`

describe('profile pictures', () => {
  it('are read from their headers', () => {
    expect(imageInfo(pngOf(64, 48))).toEqual({ type: 'png', width: 64, height: 48 })
    expect(imageInfo(jpegOf(100, 80))).toEqual({ type: 'jpeg', width: 100, height: 80 })
    expect(imageInfo(webpLossy(120, 90))).toEqual({ type: 'webp', width: 120, height: 90 })
    expect(imageInfo(webpExtended(200, 150))).toEqual({ type: 'webp', width: 200, height: 150 })
    expect(imageInfo(Buffer.from('GIF89a......'))).toBeNull()
    expect(imageInfo(Buffer.alloc(0))).toBeNull()
  })

  it('are accepted when they are real, small and what they say they are', () => {
    expect(checkAvatar(url('png', pngOf(256, 256)))).toBeNull()
    expect(checkAvatar(url('jpeg', jpegOf(512, 512)))).toBeNull()
    expect(checkAvatar(url('webp', webpLossy(64, 64)))).toBeNull()
  })

  it('are refused when they are not images, are the wrong type, or are huge in pixels', () => {
    expect(checkAvatar(url('png', Buffer.from('<svg onload=alert(1)>')))).toMatch(/not a valid image/)
    expect(checkAvatar(url('jpeg', pngOf(10, 10)))).toMatch(/says it is jpeg but it is png/)
    expect(checkAvatar(url('png', pngOf(513, 10)))).toMatch(/at most 512/)
    expect(checkAvatar(url('png', pngOf(60_000, 60_000)))).toMatch(/at most 512/) // a decompression bomb in a few bytes
    expect(checkAvatar(url('png', pngOf(0, 10)))).toMatch(/at most 512/)
    expect(checkAvatar(url('webp', webpExtended(5000, 5000)))).toMatch(/at most 512/)
    expect(checkAvatar('data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=')).toMatch(/PNG, JPEG or WebP/)
    expect(checkAvatar('https://example.com/a.png')).toMatch(/data URL/)
  })
})
