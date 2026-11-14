<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Button } from '@/components/ui/button'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import AccountSignIn from '@/components/time/AccountSignIn.vue'
import { commands, type ConflictField, type ConflictView } from '@/bindings'
import { call, errorMessage } from '@/lib/ipc'
import { useAccountStore } from '@/stores/account'

const { t } = useI18n()
const account = useAccountStore()

onMounted(async () => {
  await account.load()
  await account.loadSync()
})

const lastSynced = computed(() =>
  account.sync?.lastSyncedAt ? new Date(account.sync.lastSyncedAt).toLocaleString() : null,
)

async function retry(c: ConflictView) {
  try {
    if (await account.retryConflict(c)) toast.success(t('account.retried'))
    else toast.error(t('account.stillBlocked'))
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
/** Conflicts whose field comparison is open, with the fields once loaded. */
const open = ref(new Map<string, ConflictField[]>())
const keyOf = (c: ConflictView) => `${c.table}:${c.rowId}`
async function toggle(c: ConflictView) {
  const next = new Map(open.value)
  if (next.has(keyOf(c))) {
    next.delete(keyOf(c))
  } else {
    try {
      next.set(keyOf(c), await call(commands.syncConflictDetails(c.table, c.rowId)))
    } catch (err) {
      toast.error(errorMessage(err))
      return
    }
  }
  open.value = next
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
</script>

<template>
  <Card>
    <CardHeader>
      <CardTitle>{{ $t('account.title') }}</CardTitle>
      <CardDescription>{{ $t('account.description') }}</CardDescription>
    </CardHeader>
    <CardContent class="grid gap-4">
      <AccountSignIn sync-now />

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
              <Button size="sm" variant="ghost" @click="toggle(c)">{{ open.has(keyOf(c)) ? $t('account.hideDetails') : $t('account.details') }}</Button>
              <Button size="sm" variant="outline" @click="retry(c)">{{ $t('account.retry') }}</Button>
              <Button size="sm" variant="ghost" @click="dismiss(c)">{{ $t('account.dismiss') }}</Button>
            </div>
            <table v-if="open.has(keyOf(c))" class="mt-2 w-full basis-full border-t pt-2 text-left">
              <thead>
                <tr class="text-muted-foreground">
                  <th class="py-1 pr-3 font-normal">{{ $t('account.field') }}</th>
                  <th class="py-1 pr-3 font-normal">{{ $t('account.accountValue') }}</th>
                  <th class="py-1 font-normal">{{ $t('account.thisDevice') }}</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="f in open.get(keyOf(c))" :key="f.field" :class="f.differs ? 'font-medium text-foreground' : ''">
                  <td class="py-0.5 pr-3 align-top">{{ f.field }}</td>
                  <td class="break-words py-0.5 pr-3 align-top">{{ f.server }}</td>
                  <td class="break-words py-0.5 align-top">{{ f.local ?? $t('account.notOnDevice') }}</td>
                </tr>
              </tbody>
            </table>
          </li>
        </ul>
        <div v-if="account.sync.lastError" class="text-destructive">
          <p>{{ $t(`account.syncError.${account.sync.lastErrorKind ?? 'server'}`) }}</p>
          <p class="break-words text-xs opacity-70">{{ account.sync.lastError }}</p>
        </div>
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
