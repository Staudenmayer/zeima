<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { AlertTriangle, Globe, PackagePlus, Puzzle, Settings2, Trash2 } from '@lucide/vue'
import { commands, type AddonInfo, type AddonPackage } from '@/bindings'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import AddonConsentDialog from '@/components/time/AddonConsentDialog.vue'
import AddonSettingsDialog from '@/components/time/AddonSettingsDialog.vue'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import EmptyState from '@/components/time/EmptyState.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import { PERMISSION_KEYS } from '@/lib/addons'
import { call, errorMessage } from '@/lib/ipc'

definePage({ meta: { titleKey: 'nav.addons' } })

const { t } = useI18n()
const addons = ref<AddonInfo[]>([])

async function load() {
  try {
    addons.value = await call(commands.addonList())
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
onMounted(load)

// Install: pick a file, show what it asks for, then install.
const pkg = ref<AddonPackage | null>(null)
const consentOpen = ref(false)
async function pick() {
  try {
    const picked = await call(commands.addonPick())
    if (!picked) return
    pkg.value = picked
    consentOpen.value = true
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
async function installed() {
  await load()
  toast.success(t('addons.installed', { name: pkg.value?.manifest.name ?? '' }))
}

async function setEnabled(addon: AddonInfo, enabled: boolean) {
  try {
    const next = await call(commands.addonSetEnabled(addon.record.id, enabled))
    addons.value = addons.value.map((a) => (a.record.id === next.record.id ? next : a))
    if (next.error) toast.error(next.error)
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

const settingsFor = ref<AddonInfo | null>(null)
const settingsOpen = ref(false)
function openSettings(addon: AddonInfo) {
  settingsFor.value = addon
  settingsOpen.value = true
}

const removing = ref<AddonInfo | null>(null)
const confirmOpen = ref(false)
function askRemove(addon: AddonInfo) {
  removing.value = addon
  confirmOpen.value = true
}
async function remove() {
  if (!removing.value) return
  try {
    await call(commands.addonUninstall(removing.value.record.id))
    await load()
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
</script>

<template>
  <PageHeader :title="$t('nav.addons')">
    <Button @click="pick"><PackagePlus /> {{ $t('addons.installFromFile') }}</Button>
  </PageHeader>

  <EmptyState v-if="addons.length === 0" :icon="Puzzle" :text="$t('addons.empty')">
    <Button variant="outline" @click="pick"><PackagePlus /> {{ $t('addons.installFromFile') }}</Button>
  </EmptyState>

  <div v-else class="grid max-w-4xl gap-3">
    <article v-for="addon in addons" :key="addon.record.id" class="grid gap-3 rounded-lg border p-4">
      <div class="flex items-start gap-3">
        <div class="flex size-9 shrink-0 items-center justify-center rounded-md bg-muted">
          <Puzzle class="size-4 text-muted-foreground" />
        </div>
        <div class="min-w-0 flex-1">
          <h2 class="font-medium">{{ addon.manifest?.name ?? addon.record.id }}</h2>
          <p class="text-xs text-muted-foreground">
            {{ $t('addons.byAuthor', { version: addon.record.version, author: addon.manifest?.author ?? '?' }) }}
            · <span class="font-mono">{{ addon.record.id }}</span>
          </p>
          <p v-if="addon.manifest?.description" class="mt-1 text-sm">{{ addon.manifest.description }}</p>
        </div>
        <div class="flex items-center gap-1">
          <Button
            v-if="(addon.manifest?.settings ?? []).length"
            size="icon"
            variant="ghost"
            :title="$t('addons.settings')"
            @click="openSettings(addon)"
          ><Settings2 /></Button>
          <Button size="icon" variant="ghost" :title="$t('addons.uninstall')" @click="askRemove(addon)"><Trash2 /></Button>
          <Switch
            class="ml-2"
            :model-value="addon.record.enabled"
            :aria-label="$t('addons.enabled')"
            @update:model-value="setEnabled(addon, $event)"
          />
        </div>
      </div>

      <div v-if="addon.record.granted.length || (addon.manifest?.allowed_hosts ?? []).length" class="flex flex-wrap items-center gap-1.5">
        <Badge v-for="p in addon.record.granted" :key="p" variant="secondary">{{ $t(`addons.permissionShort.${PERMISSION_KEYS[p]}`) }}</Badge>
        <template v-if="addon.record.granted.includes('network')">
          <Globe class="ml-1 size-3.5 text-muted-foreground" />
          <span class="font-mono text-xs text-muted-foreground">{{ (addon.manifest?.allowed_hosts ?? []).join(', ') }}</span>
        </template>
      </div>

      <p v-if="addon.error" class="flex items-start gap-2 rounded-md bg-destructive/10 p-2 text-sm text-destructive">
        <AlertTriangle class="mt-0.5 size-4 shrink-0" />
        <span class="break-all">{{ addon.error }}</span>
      </p>
    </article>
    <p class="text-xs text-muted-foreground">{{ $t('addons.sandboxHint') }}</p>
  </div>

  <AddonConsentDialog v-model:open="consentOpen" :pkg="pkg" @installed="installed" />
  <AddonSettingsDialog v-model:open="settingsOpen" :addon="settingsFor" @saved="load" />
  <ConfirmDialog
    v-model:open="confirmOpen"
    :title="$t('addons.uninstallConfirm', { name: removing?.manifest?.name ?? removing?.record.id ?? '' })"
    @confirm="remove"
  />
</template>
