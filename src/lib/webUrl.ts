// A link from a meeting that the server sent: only an http(s) address is opened, never another scheme
// (`file:`, `javascript:`, an app's own scheme) that the system would hand to a program.

/** The normalized address if `raw` is an http(s) URL, otherwise `null`. */
export function webUrl(raw: string | null | undefined): string | null {
  if (!raw) return null
  try {
    const url = new URL(raw.trim())
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : null
  } catch {
    return null
  }
}
