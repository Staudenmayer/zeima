<script setup lang="ts">
import { Check, Copy, Trash2 } from '@lucide/vue'
import { onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import { deleteV1OrgsByOrgIdScimTokensById, getV1OrgsByOrgIdScim, ok, postV1OrgsByOrgIdScimTokens } from '@/lib/api'
import type { GetV1OrgsByOrgIdScimResponses } from '@zeima/api-contract/client'
import { errorMessage } from '@/lib/ipc'
import { useOrgStore } from '@/stores/org'

// Provisioning (SCIM): the organization's identity provider adds, disables and removes people by
// itself. This makes the keys it signs in with; a key is shown once.
const { t } = useI18n()
const org = useOrgStore()

type Scim = GetV1OrgsByOrgIdScimResponses[200]
const current = ref<Scim | null>(null)
const name = ref('')
const fresh = ref('')
const copied = ref<string | null>(null)
const removing = ref<string | null>(null)
const confirmOpen = ref(false)
const busy = ref(false)

const orgId = () => org.activeId as string

async function load() {
  if (!org.activeId) return
  try {
    current.value = ok(await getV1OrgsByOrgIdScim({ path: { orgId: orgId() } }))
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
onMounted(load)
watch(() => org.activeId, load)

async function create() {
  busy.value = true
  try {
    const made = ok(await postV1OrgsByOrgIdScimTokens({ path: { orgId: orgId() }, body: { name: name.value.trim() } }))
    fresh.value = made.token
    name.value = ''
    await load()
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    busy.value = false
  }
}

async function remove() {
  if (!removing.value) return
  try {
    await deleteV1OrgsByOrgIdScimTokensById({ path: { orgId: orgId(), id: removing.value } })
    removing.value = null
    fresh.value = ''
    await load()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

async function copy(text: string, key: string) {
  try {
    await navigator.clipboard.writeText(text)
    copied.value = key
    setTimeout(() => (copied.value = null), 1500)
  } catch {
    toast.error(t('org.sso.copyFailed'))
  }
}

const day = (ms: number) => new Date(ms).toLocaleDateString()
</script>

<template>
  <section v-if="current" class="grid max-w-3xl gap-4 border-t pt-6">
    <div>
      <h3 class="font-medium">{{ $t('org.scim.title') }}</h3>
      <p class="mt-1 text-sm text-muted-foreground">{{ $t('org.scim.intro') }}</p>
    </div>

    <div class="grid gap-1.5 rounded-lg border bg-muted/30 p-3 text-sm">
      <p class="font-medium">{{ $t('org.scim.address') }}</p>
      <p class="text-muted-foreground">{{ $t('org.scim.addressHint') }}</p>
      <div class="flex items-center gap-2">
        <code class="min-w-0 flex-1 truncate rounded bg-background px-2 py-1.5 text-xs">{{ current.baseUrl }}</code>
        <Button type="button" size="sm" variant="outline" class="shrink-0" @click="copy(current.baseUrl, 'url')">
          <component :is="copied === 'url' ? Check : Copy" /> {{ $t(copied === 'url' ? 'org.sso.copied' : 'org.sso.copy') }}
        </Button>
      </div>
    </div>

    <div v-if="fresh" class="grid gap-1.5 rounded-lg border border-amber-500/40 bg-amber-500/5 p-3 text-sm">
      <p class="font-medium">{{ $t('org.scim.fresh') }}</p>
      <div class="flex items-center gap-2">
        <code class="min-w-0 flex-1 truncate rounded bg-background px-2 py-1.5 text-xs">{{ fresh }}</code>
        <Button type="button" size="sm" variant="outline" class="shrink-0" @click="copy(fresh, 'key')">
          <component :is="copied === 'key' ? Check : Copy" /> {{ $t(copied === 'key' ? 'org.sso.copied' : 'org.sso.copy') }}
        </Button>
      </div>
    </div>

    <ul v-if="current.tokens.length" class="divide-y rounded-lg border text-sm">
      <li v-for="k in current.tokens" :key="k.id" class="flex items-center gap-3 px-3 py-2">
        <div class="min-w-0 flex-1">
          <p class="truncate font-medium">{{ k.name }}</p>
          <p class="text-xs text-muted-foreground">
            {{ k.hint }}… · {{ $t('org.scim.created', { date: day(k.createdAt) }) }} ·
            {{ k.lastUsedAt ? $t('org.scim.used', { date: day(k.lastUsedAt) }) : $t('org.scim.unused') }}
          </p>
        </div>
        <Button type="button" size="icon" variant="ghost" :aria-label="$t('common.delete')" @click="((removing = k.id), (confirmOpen = true))"><Trash2 /></Button>
      </li>
    </ul>

    <form class="flex flex-wrap items-end gap-2" @submit.prevent="create">
      <div class="grid gap-1.5">
        <Label for="scim-name">{{ $t('org.scim.name') }}</Label>
        <Input id="scim-name" v-model="name" maxlength="80" class="w-64" :placeholder="$t('org.scim.namePlaceholder')" required />
      </div>
      <Button type="submit" :disabled="busy || !name.trim() || current.tokens.length >= 5">{{ $t('org.scim.make') }}</Button>
    </form>
    <p class="text-xs text-muted-foreground">{{ $t('org.scim.note') }}</p>

    <ConfirmDialog v-model:open="confirmOpen" :title="$t('org.scim.removeConfirm')" @confirm="remove" />
  </section>
</template>
