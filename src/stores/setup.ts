import { defineStore } from 'pinia'
import { ref } from 'vue'

/** Lets other pages (Settings) open the setup again after the first start. */
export const useSetupStore = defineStore('setup', () => {
  const requested = ref(false)
  const run = () => (requested.value = true)
  return { requested, run }
})
