// A `mailto:` link from an address the server sent: only a plain address is linked, and it is encoded, so
// nothing in it (`?`, `&`, `%`, spaces) can add headers such as `attach` or `bcc` to the message.

const PLAIN_EMAIL = /^[A-Za-z0-9.!#$%&'*+/=^_`{|}~-]+@[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?)+$/

/** `mailto:` URL for a plain email address, or `null` if it does not look like one. */
export function mailtoUrl(email: string): string | null {
  const address = email.trim()
  if (address.length > 254 || !PLAIN_EMAIL.test(address)) return null
  // `@` stays readable; `?`, `&`, `%` and the like are percent-encoded.
  return `mailto:${encodeURIComponent(address).replace(/%40/g, '@')}`
}
