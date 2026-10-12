/** Default colors offered for projects and tags. */
export const PALETTE = [
  '#3b82f6', '#22c55e', '#f59e0b', '#ef4444', '#a855f7',
  '#06b6d4', '#ec4899', '#84cc16', '#f97316', '#64748b',
]

/** Black or white, whichever reads better on the hex color `bg`. */
export function textOn(bg: string): '#000000' | '#ffffff' {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(bg.slice(i, i + 2), 16) / 255
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
  })
  const luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b
  // Contrast with black vs. with white: (L + 0.05) / 0.05 vs. 1.05 / (L + 0.05).
  return (luminance + 0.05) ** 2 > 0.0525 ? '#000000' : '#ffffff'
}

export function nextColor(used: string[]): string {
  return PALETTE.find((c) => !used.includes(c)) ?? PALETTE[used.length % PALETTE.length]
}
