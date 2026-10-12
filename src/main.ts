import { createApp } from 'vue'
import { createPinia } from 'pinia'
import 'unfonts.css'
import App from './App.vue'
import { router } from './plugins/router.ts'
import i18n from './plugins/i18n.ts'
import { connectBackend } from './plugins/backend.ts'
import './style.css'

const app = createApp(App).use(createPinia()).use(router).use(i18n)

// Load stores before mounting so the first render has data.
connectBackend(router)
  .catch((err) => console.error('failed to connect to backend', err))
  .finally(() => app.mount('#app'))
