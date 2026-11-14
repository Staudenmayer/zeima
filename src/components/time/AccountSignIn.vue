<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { AppError, errorMessage } from '@/lib/ipc'
import { useAccountStore } from '@/stores/account'

// The one place to sign in: shown in Settings and in the setup. Signed in it
// shows who; signed out it is a server field with a Sign in button next to it
// (signing in saves the server first); while the browser sign-in runs it
// waits with a way to cancel.
const props = defineProps<{
  /** Text under the server field. */
  hint?: string
  /** Also offer "Sync now" next to "Sign out". */
  syncNow?: boolean
  inputId?: string
}>()

const { t } = useI18n()
const account = useAccountStore()

const who = computed(() => account.status.user?.email ?? account.status.user?.name ?? '')

async function signIn() {
  try {
    await account.signInToDraft()
    toast.success(t('account.signedInToast'))
  } catch (err) {
    if (!(err instanceof AppError && err.code === 'cancelled')) toast.error(errorMessage(err))
  }
}
async function signOut() {
  try {
    await account.signOut()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
async function syncNow() {
  try {
    await account.syncNow()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
const id = computed(() => props.inputId ?? 'server-url')
</script>

<template>
  <div v-if="account.status.signedIn" class="flex flex-wrap items-center justify-between gap-3 rounded-lg border px-4 py-3 text-sm">
    <div>
      <p class="font-medium">{{ $t('account.signedInBox') }}</p>
      <p class="text-muted-foreground">{{ who }}</p>
    </div>
    <div class="flex items-center gap-2">
      <Button v-if="syncNow" variant="outline" @click="syncNow">{{ $t('account.syncNow') }}</Button>
      <Button variant="outline" @click="signOut">{{ $t('account.signOut') }}</Button>
    </div>
  </div>

  <div v-else-if="account.busy" class="flex items-center justify-between gap-3 rounded-lg border px-4 py-3 text-sm">
    <span class="text-muted-foreground">{{ $t('account.waiting') }}</span>
    <Button variant="outline" @click="account.cancel()">{{ $t('account.cancel') }}</Button>
  </div>

  <div v-else class="grid gap-2">
    <Label :for="id">{{ $t('account.server') }}</Label>
    <div class="flex gap-2">
      <Input
        :id="id"
        v-model="account.serverDraft"
        :placeholder="$t('account.serverPlaceholder')"
        spellcheck="false"
        autocomplete="off"
        @keydown.enter="signIn"
      />
      <Button class="min-w-26" :disabled="!account.serverDraft.trim()" @click="signIn">{{ $t('account.signIn') }}</Button>
    </div>
    <p class="text-xs text-muted-foreground">{{ hint ?? $t('account.signInHint') }}</p>
  </div>
</template>
