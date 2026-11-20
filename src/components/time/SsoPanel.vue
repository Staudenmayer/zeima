<script setup lang="ts">
import { Check, Copy } from '@lucide/vue'
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import { deleteV1OrgsByOrgIdSso, getV1OrgsByOrgIdSso, ok, putV1OrgsByOrgIdSso } from '@/lib/api'
import type { GetV1OrgsByOrgIdSsoResponses } from '@zeima/api-contract/client'
import { errorMessage } from '@/lib/ipc'
import { useOrgStore } from '@/stores/org'

// The organization's single sign-on: an identity provider (Microsoft Entra ID, Google Workspace, any
// OpenID Connect or SAML provider) that the people with an email address in the organization's domains
// sign in with. Keycloak does the work; this is the form.
const { t } = useI18n()
const org = useOrgStore()

type Sso = GetV1OrgsByOrgIdSsoResponses[200]
type Preset = 'entra' | 'google' | 'oidc' | 'saml'
const PRESETS: Preset[] = ['entra', 'google', 'oidc', 'saml']
const ENTRA = 'https://login.microsoftonline.com/'
const GOOGLE = 'https://accounts.google.com'

const current = ref<Sso | null>(null)
const loading = ref(true)
const saving = ref(false)
const confirmOpen = ref(false)
const copied = ref(false)

const preset = ref<Preset>('entra')
const name = ref('')
const tenant = ref('')
const issuer = ref('')
const metadataUrl = ref('')
const clientId = ref('')
const clientSecret = ref('')
const domains = ref('')
const enabled = ref(true)

const orgId = () => org.activeId as string

/** The form from what the server has. */
function fill(sso: Sso) {
  name.value = sso.name ?? ''
  clientId.value = sso.clientId ?? ''
  clientSecret.value = ''
  domains.value = sso.domains.join('\n')
  enabled.value = sso.configured ? sso.enabled : true
  const url = sso.sourceUrl ?? ''
  if (sso.protocol === 'saml') {
    preset.value = 'saml'
    metadataUrl.value = url
  } else if (url.startsWith(ENTRA)) {
    preset.value = 'entra'
    tenant.value = url.slice(ENTRA.length).split('/')[0] ?? ''
  } else if (url.replace(/\/+$/, '') === GOOGLE) {
    preset.value = 'google'
  } else if (url) {
    preset.value = 'oidc'
    issuer.value = url
  }
}

async function load() {
  if (!org.activeId) return
  loading.value = true
  try {
    current.value = ok(await getV1OrgsByOrgIdSso({ path: { orgId: orgId() } }))
    if (current.value?.configured) fill(current.value)
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    loading.value = false
  }
}
onMounted(load)
watch(() => org.activeId, load)

const isSaml = computed(() => preset.value === 'saml')
const sourceUrl = computed(() => {
  if (preset.value === 'entra') return tenant.value.trim() ? `${ENTRA}${tenant.value.trim()}/v2.0` : ''
  if (preset.value === 'google') return GOOGLE
  if (preset.value === 'saml') return metadataUrl.value.trim()
  return issuer.value.trim()
})
const domainList = computed(() => domains.value.split(/[\s,;]+/).map((d) => d.trim()).filter(Boolean))
const storedSecret = computed(() => current.value?.configured && current.value.protocol === 'oidc')
const ready = computed(
  () => name.value.trim() && sourceUrl.value && domainList.value.length > 0 && (isSaml.value || (clientId.value.trim() && (clientSecret.value || storedSecret.value))),
)

async function save() {
  saving.value = true
  try {
    current.value = ok(
      await putV1OrgsByOrgIdSso({ path: { orgId: orgId() }, body: {
          protocol: isSaml.value ? 'saml' : 'oidc',
          name: name.value.trim(),
          sourceUrl: sourceUrl.value,
          ...(!isSaml.value && { clientId: clientId.value.trim(), ...(clientSecret.value && { clientSecret: clientSecret.value }) }),
          domains: domainList.value,
          enabled: enabled.value,
        },
      }),
    )
    clientSecret.value = ''
    toast.success(t('org.sso.saved'))
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    saving.value = false
  }
}

async function remove() {
  try {
    await deleteV1OrgsByOrgIdSso({ path: { orgId: orgId() } })
    toast.success(t('org.sso.removed'))
    name.value = clientId.value = clientSecret.value = domains.value = tenant.value = issuer.value = metadataUrl.value = ''
    await load()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

/** The address the provider is pointed at: what the person copies. */
const redirect = computed(() => (isSaml.value ? (current.value?.samlMetadataUrl ?? '') : (current.value?.redirectUri ?? '')))
async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text)
    copied.value = true
    setTimeout(() => (copied.value = false), 1500)
  } catch {
    toast.error(t('org.sso.copyFailed'))
  }
}
</script>

<template>
  <div class="grid max-w-3xl gap-6">
    <p class="text-sm text-muted-foreground">{{ $t('org.sso.intro') }}</p>

    <form v-if="!loading" class="grid gap-5" @submit.prevent="save">
      <div class="grid gap-2">
        <Label>{{ $t('org.sso.provider') }}</Label>
        <Select v-model="preset">
          <SelectTrigger class="w-full sm:w-72"><SelectValue /></SelectTrigger>
          <SelectContent><SelectItem v-for="p in PRESETS" :key="p" :value="p">{{ $t(`org.sso.presets.${p}`) }}</SelectItem></SelectContent>
        </Select>
      </div>

      <!-- Where the provider has to be pointed: the first thing to do there. -->
      <div class="grid gap-1.5 rounded-lg border bg-muted/30 p-3 text-sm">
        <p class="font-medium">{{ $t(isSaml ? 'org.sso.metadataTitle' : 'org.sso.redirectTitle') }}</p>
        <p class="text-muted-foreground">{{ $t(isSaml ? 'org.sso.metadataHint' : `org.sso.redirectHint.${preset}`) }}</p>
        <div class="flex items-center gap-2">
          <code class="min-w-0 flex-1 truncate rounded bg-background px-2 py-1.5 text-xs">{{ redirect }}</code>
          <Button type="button" size="sm" variant="outline" class="shrink-0" @click="copy(redirect)"><component :is="copied ? Check : Copy" /> {{ $t(copied ? 'org.sso.copied' : 'org.sso.copy') }}</Button>
        </div>
      </div>

      <div class="grid gap-2">
        <Label for="sso-name">{{ $t('org.sso.name') }}</Label>
        <Input id="sso-name" v-model="name" maxlength="80" :placeholder="$t('org.sso.namePlaceholder')" required />
        <p class="text-xs text-muted-foreground">{{ $t('org.sso.nameHint') }}</p>
      </div>

      <div v-if="preset === 'entra'" class="grid gap-2">
        <Label for="sso-tenant">{{ $t('org.sso.tenant') }}</Label>
        <Input id="sso-tenant" v-model="tenant" placeholder="00000000-0000-0000-0000-000000000000" required />
        <p class="text-xs text-muted-foreground">{{ $t('org.sso.tenantHint') }}</p>
      </div>
      <div v-else-if="preset === 'oidc'" class="grid gap-2">
        <Label for="sso-issuer">{{ $t('org.sso.issuer') }}</Label>
        <Input id="sso-issuer" v-model="issuer" type="url" placeholder="https://idp.example.com/realms/acme" required />
        <p class="text-xs text-muted-foreground">{{ $t('org.sso.issuerHint') }}</p>
      </div>
      <div v-else-if="preset === 'saml'" class="grid gap-2">
        <Label for="sso-metadata">{{ $t('org.sso.metadata') }}</Label>
        <Input id="sso-metadata" v-model="metadataUrl" type="url" placeholder="https://idp.example.com/metadata.xml" required />
        <p class="text-xs text-muted-foreground">{{ $t('org.sso.metadataFieldHint') }}</p>
      </div>

      <template v-if="!isSaml">
        <div class="grid gap-4 sm:grid-cols-2">
          <div class="grid gap-2">
            <Label for="sso-client">{{ $t('org.sso.clientId') }}</Label>
            <Input id="sso-client" v-model="clientId" autocomplete="off" required />
          </div>
          <div class="grid gap-2">
            <Label for="sso-secret">{{ $t('org.sso.clientSecret') }}</Label>
            <Input id="sso-secret" v-model="clientSecret" type="password" autocomplete="new-password" :placeholder="storedSecret ? $t('org.sso.secretKept') : ''" :required="!storedSecret" />
          </div>
        </div>
      </template>

      <div class="grid gap-2">
        <Label for="sso-domains">{{ $t('org.sso.domains') }}</Label>
        <Textarea id="sso-domains" v-model="domains" rows="3" placeholder="example.com" />
        <p class="text-xs text-muted-foreground">{{ $t('org.sso.domainsHint') }}</p>
        <div v-if="current?.domainChallenge" class="grid gap-1 rounded-lg border border-amber-500/40 bg-amber-500/5 p-3 text-xs">
          <p class="font-medium">{{ $t('org.sso.proofTitle') }}</p>
          <p class="text-muted-foreground">{{ $t('org.sso.proofHint', { name: current.domainChallenge.name }) }}</p>
          <div class="flex items-center gap-2">
            <code class="min-w-0 flex-1 truncate rounded bg-background px-2 py-1.5">{{ current.domainChallenge.value }}</code>
            <Button type="button" size="sm" variant="outline" class="shrink-0" @click="copy(current.domainChallenge.value)"><Copy /> {{ $t('org.sso.copy') }}</Button>
          </div>
        </div>
      </div>

      <label class="flex items-center gap-2 text-sm">
        <Switch v-model="enabled" />
        {{ $t('org.sso.enabled') }}
      </label>

      <p class="text-xs text-muted-foreground">{{ $t('org.sso.joinNote') }}</p>

      <div class="flex flex-wrap items-center gap-2">
        <Button type="submit" :disabled="saving || !ready">{{ $t('common.save') }}</Button>
        <Button v-if="current?.configured" type="button" variant="outline" @click="confirmOpen = true">{{ $t('org.sso.remove') }}</Button>
      </div>
    </form>

    <ConfirmDialog v-model:open="confirmOpen" :title="$t('org.sso.removeConfirm')" @confirm="remove" />
  </div>
</template>
