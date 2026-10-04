import { createApp } from "vue";
import App from "./App.vue";
import { createPinia } from 'pinia'
import { router } from "./plugins/router.ts";
import './style.css';
import i18n from "./plugins/i18n.ts";

createApp(App)
    .use(router)
    .use(createPinia())
    .use(i18n)
    .mount("#app");
