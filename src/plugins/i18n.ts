import { createI18n } from 'vue-i18n';
import en from '@/i18n/en';
import de from '@/i18n/de';

export default createI18n({
    fallbackLocale: 'en',
    //locale: 'de',
    messages: {
        en: en,
        de: de,
    },
});