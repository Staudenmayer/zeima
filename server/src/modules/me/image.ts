// What a profile picture is, read from its header without decoding it: the type must be what the data URL
// says, and the size in pixels must be small. Pictures are shown to every member of the person's
// organizations, so a malformed or huge one should never reach their image decoders.

export interface ImageInfo {
  type: 'png' | 'jpeg' | 'webp'
  width: number
  height: number
}

/** The app shrinks pictures to a small square; this allows some room. */
export const MAX_AVATAR_PIXELS = 512

const u16be = (b: Buffer, o: number) => b.readUInt16BE(o)
const u16le = (b: Buffer, o: number) => b.readUInt16LE(o)
const u24le = (b: Buffer, o: number) => b[o]! | (b[o + 1]! << 8) | (b[o + 2]! << 16)

function png(b: Buffer): ImageInfo | null {
  if (b.length < 24 || !b.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])) || b.toString('latin1', 12, 16) !== 'IHDR') return null
  return { type: 'png', width: b.readUInt32BE(16), height: b.readUInt32BE(20) }
}

function jpeg(b: Buffer): ImageInfo | null {
  if (b.length < 4 || b[0] !== 0xff || b[1] !== 0xd8) return null
  let o = 2
  while (o + 4 <= b.length) {
    if (b[o] !== 0xff) return null
    const marker = b[o + 1]!
    if (marker === 0xff) {
      o++
      continue
    }
    if (marker === 0xd8 || (marker >= 0xd0 && marker <= 0xd7) || marker === 0x01) {
      o += 2
      continue
    }
    const length = u16be(b, o + 2)
    // Start-of-frame markers carry the size (not C4 DHT, C8 JPG, CC DAC).
    if (marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc) {
      if (o + 9 > b.length) return null
      return { type: 'jpeg', height: u16be(b, o + 5), width: u16be(b, o + 7) }
    }
    if (length < 2) return null
    o += 2 + length
  }
  return null
}

function webp(b: Buffer): ImageInfo | null {
  if (b.length < 30 || b.toString('latin1', 0, 4) !== 'RIFF' || b.toString('latin1', 8, 12) !== 'WEBP') return null
  const chunk = b.toString('latin1', 12, 16)
  if (chunk === 'VP8X') return { type: 'webp', width: u24le(b, 24) + 1, height: u24le(b, 27) + 1 }
  if (chunk === 'VP8 ') {
    if (b[23] !== 0x9d || b[24] !== 0x01 || b[25] !== 0x2a) return null
    return { type: 'webp', width: u16le(b, 26) & 0x3fff, height: u16le(b, 28) & 0x3fff }
  }
  if (chunk === 'VP8L') {
    if (b[20] !== 0x2f) return null
    const bits = b.readUInt32LE(21)
    return { type: 'webp', width: (bits & 0x3fff) + 1, height: ((bits >> 14) & 0x3fff) + 1 }
  }
  return null
}

/** The type and size of a PNG, JPEG or WebP, or `null` if the bytes are none of them. */
export function imageInfo(bytes: Buffer): ImageInfo | null {
  return png(bytes) ?? jpeg(bytes) ?? webp(bytes)
}

/** Checks a data URL: the bytes are the image type it says, and no larger than `MAX_AVATAR_PIXELS` each way. */
export function checkAvatar(dataUrl: string): string | null {
  const m = /^data:image\/(png|jpeg|webp);base64,([A-Za-z0-9+/]+={0,2})$/.exec(dataUrl)
  if (!m) return 'the picture must be a PNG, JPEG or WebP data URL'
  const info = imageInfo(Buffer.from(m[2]!, 'base64'))
  if (!info) return 'the picture is not a valid image'
  if (info.type !== m[1]) return `the picture says it is ${m[1]} but it is ${info.type}`
  if (info.width < 1 || info.height < 1 || info.width > MAX_AVATAR_PIXELS || info.height > MAX_AVATAR_PIXELS) {
    return `the picture must be at most ${MAX_AVATAR_PIXELS} by ${MAX_AVATAR_PIXELS} pixels`
  }
  return null
}
