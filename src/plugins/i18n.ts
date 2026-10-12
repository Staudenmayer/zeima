import { createI18n } from 'vue-i18n'
import en from '@/i18n/en'
import de from '@/i18n/de'

export const LOCALES = ['en', 'de'] as const
export type Locale = (typeof LOCALES)[number]

function pick(preference: string | null): Locale {
  const lang = (preference ?? navigator.language ?? 'en').slice(0, 2).toLowerCase()
  return (LOCALES as readonly string[]).includes(lang) ? (lang as Locale) : 'en'
}

const i18n = createI18n<[typeof en], Locale, false>({
  legacy: false,
  locale: pick(null),
  fallbackLocale: 'en',
  messages: { en, de },
})

/** `null` follows the system language. */
export function applyLocale(preference: string | null) {
  i18n.global.locale.value = pick(preference)
}

export default i18n
