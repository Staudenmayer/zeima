import { defineStore } from 'pinia'
import { ref } from 'vue'
import { commands, type UpdateInfo } from '@/bindings'
import { call } from '@/lib/ipc'

/** Checking for and installing app updates (GitHub releases, signed). */
export const useUpdatesStore = defineStore('updates', () => {
  const available = ref<UpdateInfo | null>(null)
  const checking = ref(false)
  const installing = ref(false)

  /** Looks for a newer version; throws if the check itself fails. */
  async function check() {
    checking.value = true
    try {
      available.value = await call(commands.updateCheck())
      return available.value
    } finally {
      checking.value = false
    }
  }

  /** Downloads, installs and restarts the app. */
  async function install() {
    installing.value = true
    try {
      await call(commands.updateInstall())
    } finally {
      installing.value = false
    }
  }

  return { available, checking, installing, check, install }
})
