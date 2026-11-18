<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import { api, ok, type Reply } from '@/lib/api'
import { errorMessage } from '@/lib/ipc'

// A company directory (LDAP / Active Directory) people can sign in with. Only the people who run
// this installation can set it up (and only a self-hosted server has it), so the panel shows itself
// only when the server lets this person read it.
const { t } = useI18n()

type Ldap = Reply<'/v1/admin/ldap', 'get'>
const available = ref(false)
const current = ref<Ldap | null>(null)
const busy = ref(false)
const confirmOpen = ref(false)

const vendor = ref<'ad' | 'openldap'>('ad')
const url = ref('')
const startTls = ref(false)
const bindDn = ref('')
const bindPassword = ref('')
const usersDn = ref('')
const userFilter = ref('')
const enabled = ref(true)

function fill(l: Ldap) {
  if (!l.configured) return
  vendor.value = l.vendor ?? 'ad'
  url.value = l.url ?? ''
  startTls.value = l.startTls ?? false
  bindDn.value = l.bindDn ?? ''
  bindPassword.value = ''
  usersDn.value = l.usersDn ?? ''
  userFilter.value = l.userFilter ?? ''
  enabled.value = l.enabled
}

onMounted(async () => {
  try {
    current.value = ok(await api.GET('/v1/admin/ldap'))
    available.value = true
    fill(current.value)
  } catch {
    available.value = false // not an administrator of this installation, or the cloud
  }
})

const body = () => ({
  vendor: vendor.value,
  url: url.value.trim(),
  startTls: startTls.value,
  bindDn: bindDn.value.trim(),
  ...(bindPassword.value && { bindPassword: bindPassword.value }),
  usersDn: usersDn.value.trim(),
  ...(userFilter.value.trim() && { userFilter: userFilter.value.trim() }),
  enabled: enabled.value,
})

async function run(job: () => Promise<void>) {
  busy.value = true
  try {
    await job()
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    busy.value = false
  }
}

const test = () =>
  run(async () => {
    ok(await api.POST('/v1/admin/ldap/test', { body: body() }))
    toast.success(t('org.directory.testOk'))
  })

const save = () =>
  run(async () => {
    current.value = ok(await api.PUT('/v1/admin/ldap', { body: body() }))
    fill(current.value)
    toast.success(t('org.directory.saved'))
  })

const sync = () =>
  run(async () => {
    const r = ok(await api.POST('/v1/admin/ldap/sync'))
    toast.success(t('org.directory.synced', { added: r.added, updated: r.updated }))
  })

const remove = () =>
  run(async () => {
    ok(await api.DELETE('/v1/admin/ldap'))
    current.value = { configured: false, enabled: false }
    url.value = bindDn.value = bindPassword.value = usersDn.value = userFilter.value = ''
    toast.success(t('org.directory.removed'))
  })
</script>

<template>
  <section v-if="available" class="grid max-w-3xl gap-4 border-t pt-6">
    <div>
      <h3 class="font-medium">{{ $t('org.directory.title') }}</h3>
      <p class="mt-1 text-sm text-muted-foreground">{{ $t('org.directory.intro') }}</p>
    </div>

    <form class="grid gap-4" @submit.prevent="save">
      <div class="grid gap-2">
        <Label>{{ $t('org.directory.kind') }}</Label>
        <Select v-model="vendor">
          <SelectTrigger class="w-full sm:w-72"><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="ad">{{ $t('org.directory.ad') }}</SelectItem>
            <SelectItem value="openldap">{{ $t('org.directory.openldap') }}</SelectItem>
          </SelectContent>
        </Select>
      </div>
      <div class="grid gap-2">
        <Label for="ldap-url">{{ $t('org.directory.url') }}</Label>
        <Input id="ldap-url" v-model="url" placeholder="ldaps://ad.example.com:636" required />
        <label class="flex items-center gap-2 text-sm"><Switch v-model="startTls" /> {{ $t('org.directory.startTls') }}</label>
      </div>
      <div class="grid gap-4 sm:grid-cols-2">
        <div class="grid gap-2">
          <Label for="ldap-bind">{{ $t('org.directory.bindDn') }}</Label>
          <Input id="ldap-bind" v-model="bindDn" autocomplete="off" placeholder="CN=zeima,OU=Service,DC=example,DC=com" required />
        </div>
        <div class="grid gap-2">
          <Label for="ldap-pw">{{ $t('org.directory.bindPassword') }}</Label>
          <Input id="ldap-pw" v-model="bindPassword" type="password" autocomplete="new-password" :placeholder="current?.configured ? $t('org.sso.secretKept') : ''" :required="!current?.configured" />
        </div>
      </div>
      <div class="grid gap-2">
        <Label for="ldap-users">{{ $t('org.directory.usersDn') }}</Label>
        <Input id="ldap-users" v-model="usersDn" placeholder="OU=People,DC=example,DC=com" required />
      </div>
      <div class="grid gap-2">
        <Label for="ldap-filter">{{ $t('org.directory.filter') }}</Label>
        <Input id="ldap-filter" v-model="userFilter" placeholder="(memberOf=CN=zeima,OU=Groups,DC=example,DC=com)" />
        <p class="text-xs text-muted-foreground">{{ $t('org.directory.filterHint') }}</p>
      </div>
      <label class="flex items-center gap-2 text-sm"><Switch v-model="enabled" /> {{ $t('org.directory.enabled') }}</label>
      <p class="text-xs text-muted-foreground">{{ $t('org.directory.joinNote') }}</p>

      <div class="flex flex-wrap items-center gap-2">
        <Button type="submit" :disabled="busy">{{ $t('common.save') }}</Button>
        <Button type="button" variant="outline" :disabled="busy || !url || !bindDn" @click="test">{{ $t('org.directory.test') }}</Button>
        <Button v-if="current?.configured" type="button" variant="outline" :disabled="busy" @click="sync">{{ $t('org.directory.sync') }}</Button>
        <Button v-if="current?.configured" type="button" variant="outline" :disabled="busy" @click="confirmOpen = true">{{ $t('org.directory.remove') }}</Button>
      </div>
    </form>

    <ConfirmDialog v-model:open="confirmOpen" :title="$t('org.directory.removeConfirm')" @confirm="remove" />
  </section>
</template>
