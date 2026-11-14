// Shrinks a chosen picture to a small square so it can be stored and sent to
// everyone in the organization cheaply (about 5–15 kB).

export const AVATAR_SIZE = 160
const MAX_INPUT = 20 * 1024 * 1024

/** Reads an image file, cuts the middle square, scales it to `AVATAR_SIZE` and returns a JPEG data URL. */
export async function shrinkPicture(file: File): Promise<string> {
  if (!file.type.startsWith('image/')) throw new Error('not an image')
  if (file.size > MAX_INPUT) throw new Error('image too large')
  const bitmap = await createImageBitmap(file)
  try {
    const side = Math.min(bitmap.width, bitmap.height)
    const canvas = document.createElement('canvas')
    canvas.width = canvas.height = AVATAR_SIZE
    const ctx = canvas.getContext('2d')
    if (!ctx) throw new Error('no canvas')
    // White behind transparent pictures, as JPEG has no transparency.
    ctx.fillStyle = '#fff'
    ctx.fillRect(0, 0, AVATAR_SIZE, AVATAR_SIZE)
    ctx.drawImage(bitmap, (bitmap.width - side) / 2, (bitmap.height - side) / 2, side, side, 0, 0, AVATAR_SIZE, AVATAR_SIZE)
    return canvas.toDataURL('image/jpeg', 0.85)
  } finally {
    bitmap.close()
  }
}
