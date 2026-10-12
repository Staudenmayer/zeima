<script setup lang="ts">
import { ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { commands } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { call, errorMessage } from '@/lib/ipc'
import { useWorkStore } from '@/stores/work'
import CountryPicker from './CountryPicker.vue'

// Asks for the country once, before work-time numbers make sense.
const work = useWorkStore()
const open = ref(false)
const profileId = ref('AT')
const region = ref<string | null>(null)
const saving = ref(false)

watch(
  () => work.needsOnboarding,
  async (needed) => {
    if (!needed) return
    profileId.value = (await commands.profileSuggest()) ?? 'AT'
    region.value = null
    open.value = true
  },
  { immediate: true },
)

async function save() {
  saving.value = true
  try {
    await call(commands.jurisdictionSet(profileId.value, region.value))
    open.value = false
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <Dialog :open="open">
    <DialogContent class="sm:max-w-lg" :show-close-button="false" @escape-key-down.prevent @pointer-down-outside.prevent>
      <DialogHeader>
        <DialogTitle>{{ $t('onboarding.title') }}</DialogTitle>
        <DialogDescription>{{ $t('onboarding.description') }}</DialogDescription>
      </DialogHeader>
      <CountryPicker v-model:profile-id="profileId" v-model:region="region" />
      <DialogFooter>
        <Button :disabled="saving" @click="save">{{ $t('onboarding.continue') }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
