<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Button } from '@/components/ui/button'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import type { ConflictView } from '@/bindings'
import { commands } from '@/bindings'
import { AppError, call, errorMessage } from '@/lib/ipc'
import { useAccountStore } from '@/stores/account'

const { t } = useI18n()
const account = useAccountStore()

const server = ref('')
const saving = ref(false)
watch(
  () => account.status.serverUrl,
  (url) => (server.value = url ?? ''),
  { immediate: true },
)
onMounted(async () => {
  await account.load()
  await account.loadSync()
})

const syncing = ref(false)
async function syncNow() {
  syncing.value = true
  try {
    await account.syncNow()
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    syncing.value = false
  }
}
const lastSynced = computed(() =>
  account.sync?.lastSyncedAt ? new Date(account.sync.lastSyncedAt).toLocaleString() : null,
)

async function saveServer() {
  saving.value = true
  try {
    await account.setServer(server.value.trim() || null)
    toast.success(t('account.serverSaved'))
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    saving.value = false
  }
}

async function signIn() {
  try {
    await account.signIn()
    toast.success(t('account.signedInToast'))
  } catch (err) {
    if (!(err instanceof AppError && err.code === 'cancelled')) toast.error(errorMessage(err))
  }
}

async function retry(c: ConflictView) {
  try {
    if (await account.retryConflict(c)) toast.success(t('account.retried'))
    else toast.error(t('account.stillBlocked'))
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
async function dismiss(c: ConflictView) {
  try {
    await account.dismissConflict(c)
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

const replaceOpen = ref(false)
async function join(strategy: 'replace_local' | 'merge') {
  try {
    await account.join(strategy)
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

async function buy(key: string) {
  try {
    await call(commands.billingCheckout(key))
    toast.info(t('account.checkoutOpened'))
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
async function managePlan() {
  try {
    await call(commands.billingOpenPortal())
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

async function signOut() {
  try {
    await account.signOut()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
</script>

<template>
  <Card>
    <CardHeader>
      <CardTitle>{{ $t('account.title') }}</CardTitle>
      <CardDescription>{{ $t('account.description') }}</CardDescription>
    </CardHeader>
    <CardContent class="grid gap-4">
      <div class="grid gap-2">
        <Label for="server-url">{{ $t('account.server') }}</Label>
        <div class="flex gap-2">
          <Input
            id="server-url"
            v-model="server"
            :placeholder="$t('account.serverPlaceholder')"
            spellcheck="false"
            autocomplete="off"
            @keydown.enter="saveServer"
          />
          <Button
            variant="outline"
            :disabled="saving || server.trim() === (account.status.serverUrl ?? '')"
            @click="saveServer"
          >
            {{ $t('account.save') }}
          </Button>
        </div>
        <p class="text-xs text-muted-foreground">{{ $t('account.serverHint') }}</p>
      </div>

      <div v-if="account.status.serverUrl" class="flex flex-wrap items-center justify-between gap-3">
        <div v-if="account.status.signedIn" class="text-sm">
          <p class="font-medium">{{ $t('account.signedInAs', { name: account.status.user?.name ?? account.status.user?.email ?? '' }) }}</p>
          <p v-if="account.status.user?.email && account.status.user?.name" class="text-muted-foreground">{{ account.status.user.email }}</p>
        </div>
        <p v-else class="text-sm text-muted-foreground">{{ $t('account.signedOut') }}</p>

        <div v-if="account.status.signedIn" class="flex items-center gap-2">
          <Button variant="outline" :disabled="syncing" @click="syncNow">{{ $t('account.syncNow') }}</Button>
          <Button variant="outline" @click="signOut">{{ $t('account.signOut') }}</Button>
        </div>
        <div v-else-if="account.busy" class="flex items-center gap-2">
          <span class="text-sm text-muted-foreground">{{ $t('account.waiting') }}</span>
          <Button variant="outline" @click="account.cancel()">{{ $t('account.cancel') }}</Button>
        </div>
        <Button v-else @click="signIn">{{ $t('account.signIn') }}</Button>
      </div>

      <div v-if="account.status.signedIn && account.sync?.joinChoice" class="grid gap-3 rounded-md border border-primary/40 bg-primary/5 p-3 text-sm">
        <div>
          <p class="font-medium">{{ $t('account.joinTitle') }}</p>
          <p class="text-muted-foreground">{{ $t('account.joinText') }}</p>
        </div>
        <div class="flex flex-wrap gap-2">
          <Button variant="outline" @click="join('merge')">{{ $t('account.joinMerge') }}</Button>
          <Button variant="outline" @click="replaceOpen = true">{{ $t('account.joinReplace') }}</Button>
        </div>
      </div>

      <div v-if="account.status.signedIn && account.sync" class="text-xs text-muted-foreground">
        <div v-if="account.offers.length && account.plan?.status === 'free'" class="mb-2 flex flex-wrap items-center gap-2">
          <span>{{ $t('account.upgrade') }}</span>
          <Button v-for="offer in account.offers" :key="offer.key" size="sm" variant="outline" @click="buy(offer.key)">{{ offer.key }}</Button>
        </div>
        <div v-else-if="account.subscription?.portalUrl" class="mb-2">
          <Button size="sm" variant="outline" @click="managePlan">{{ $t('account.manage') }}</Button>
        </div>
        <p v-if="account.plan">
          {{ $t('account.plan', { plan: account.plan.plan }) }}
          <template v-if="account.plan.status === 'grace' && account.plan.expiresAt">
            · {{ $t('account.planGrace', { date: new Date(account.plan.expiresAt).toLocaleDateString() }) }}
          </template>
          <template v-else-if="account.plan.status === 'free'"> · {{ $t('account.planFree') }}</template>
        </p>
        <p>
          {{ $t('account.pending', { n: account.sync.pending }) }}
          <template v-if="lastSynced"> · {{ $t('account.lastSynced', { at: lastSynced }) }}</template>
        </p>
        <p v-if="account.sync.conflicts > 0">{{ $t('account.conflicts', { n: account.sync.conflicts }) }}</p>
        <ul v-if="account.conflicts.length" class="mt-2 grid gap-2">
          <li v-for="c in account.conflicts" :key="c.table + c.rowId" class="flex flex-wrap items-center justify-between gap-2 rounded-md border p-2">
            <div class="min-w-0">
              <p class="truncate font-medium text-foreground">{{ c.name ?? c.rowId }} <span class="font-normal text-muted-foreground">({{ c.table }})</span></p>
              <p class="truncate">{{ c.message }}</p>
            </div>
            <div class="flex gap-2">
              <Button size="sm" variant="outline" @click="retry(c)">{{ $t('account.retry') }}</Button>
              <Button size="sm" variant="ghost" @click="dismiss(c)">{{ $t('account.dismiss') }}</Button>
            </div>
          </li>
        </ul>
        <p v-if="account.sync.lastError" class="text-destructive">{{ account.sync.lastError }}</p>
      </div>
    </CardContent>
    <ConfirmDialog
      v-model:open="replaceOpen"
      :title="$t('account.joinReplaceConfirm')"
      :description="$t('account.joinReplaceDescription')"
      :confirm-label="$t('account.joinReplace')"
      @confirm="join('replace_local')"
    />
  </Card>
</template>
