<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { getV1OrgsByOrgIdAddons, ok, putV1OrgsByOrgIdAddons } from '@/lib/api'
import { errorMessage } from '@/lib/ipc'
import { useOrgStore } from '@/stores/org'

// Which addons the people of the organization may use. The apps enforce it: an addon the rules do
// not allow is not installed, and one that is installed stops running.
const { t } = useI18n()
const org = useOrgStore()

const mode = ref<'open' | 'allowlist'>('open')
const requireSigned = ref(false)
const allowed = ref('')
const saving = ref(false)

const orgId = () => org.activeId as string

async function load() {
  if (!org.activeId) return
  try {
    const p = ok(await getV1OrgsByOrgIdAddons({ path: { orgId: orgId() } }))
    mode.value = p.mode
    requireSigned.value = p.requireSigned
    allowed.value = p.allowed.join('\n')
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
onMounted(load)
watch(() => org.activeId, load)

async function save() {
  saving.value = true
  try {
    const p = ok(
      await putV1OrgsByOrgIdAddons({ path: { orgId: orgId() }, body: { mode: mode.value, requireSigned: requireSigned.value, allowed: allowed.value.split(/[\s,;]+/).filter(Boolean) },
      }),
    )
    allowed.value = p.allowed.join('\n')
    toast.success(t('org.addons.saved'))
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <form class="grid max-w-3xl gap-5" @submit.prevent="save">
    <p class="text-sm text-muted-foreground">{{ $t('org.addons.intro') }}</p>

    <div class="grid gap-2">
      <Label>{{ $t('org.addons.mode') }}</Label>
      <Select v-model="mode">
        <SelectTrigger class="w-full sm:w-72"><SelectValue /></SelectTrigger>
        <SelectContent>
          <SelectItem value="open">{{ $t('org.addons.open') }}</SelectItem>
          <SelectItem value="allowlist">{{ $t('org.addons.allowlist') }}</SelectItem>
        </SelectContent>
      </Select>
    </div>

    <div v-if="mode === 'allowlist'" class="grid gap-2">
      <Label for="addon-allowed">{{ $t('org.addons.allowed') }}</Label>
      <Textarea id="addon-allowed" v-model="allowed" rows="5" class="font-mono text-xs" placeholder="com.example.hours-export" />
      <p class="text-xs text-muted-foreground">{{ $t('org.addons.allowedHint') }}</p>
    </div>

    <label class="flex items-center gap-2 text-sm">
      <Switch v-model="requireSigned" />
      {{ $t('org.addons.requireSigned') }}
    </label>
    <p class="-mt-3 text-xs text-muted-foreground">{{ $t('org.addons.requireSignedHint') }}</p>

    <div><Button type="submit" :disabled="saving">{{ $t('common.save') }}</Button></div>
  </form>
</template>
