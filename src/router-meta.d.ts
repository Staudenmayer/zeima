import 'vue-router'

export {}

declare module 'vue-router' {
  interface RouteMeta {
    /** i18n key of the page title shown in the header. */
    titleKey?: string
    /** `bare` = no sidebar/header (quick-switch window); `print` = printable page. */
    layout?: 'default' | 'auth' | 'bare' | 'print'
    /** The page is exactly window-high and scrolls inside (e.g. the calendar fills the rest). */
    fill?: boolean
  }
}
