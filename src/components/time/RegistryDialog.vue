<script setup lang="ts">
import { AlertTriangle, ShieldCheck } from '@lucide/vue'
import { ref } from 'vue'
import { toast } from 'vue-sonner'
import { commands, type Permission, type RegistryAddon } from '@/bindings'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { PERMISSION_KEYS } from '@/lib/addons'
import { call, errorMessage } from '@/lib/ipc'

// Shows what an addon of the server's registry asks for before it is installed. The package is
// downloaded and its hash and signature checked when the person agrees.
const props = defineProps<{ addon: RegistryAddon | null }>()
const open = defineModel<boolean>('open', { required: true })
const emit = defineEmits<{ installed: [name: string] }>()

const installing = ref(false)

async function install() {
  if (!props.addon) return
  installing.value = true
  try {
    await call(commands.addonRegistryInstall(props.addon.id, props.addon.version))
    open.value = false
    emit('installed', props.addon.name)
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    installing.value = false
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent v-if="addon" class="sm:max-w-lg">
      <DialogHeader>
        <DialogTitle>{{ addon.installedVersion ? $t('addons.updateTitle', { name: addon.name }) : $t('addons.installTitle', { name: addon.name }) }}</DialogTitle>
        <DialogDescription>
          {{ $t('addons.byAuthor', { version: addon.version, author: addon.author || '?' }) }}
          <template v-if="addon.installedVersion"> · {{ $t('addons.replaces', { version: addon.installedVersion }) }}</template>
        </DialogDescription>
      </DialogHeader>

      <div class="grid gap-4 text-sm">
        <p v-if="addon.description">{{ addon.description }}</p>
        <p v-if="addon.blocked" class="flex items-start gap-2 rounded-md bg-destructive/10 p-3 text-destructive">
          <AlertTriangle class="mt-0.5 size-4 shrink-0" /> {{ addon.blocked }}
        </p>
        <section class="grid gap-2">
          <h3 class="font-medium">{{ $t('addons.permissionsTitle') }}</h3>
          <ul v-if="addon.permissions.length" class="grid gap-1.5">
            <li v-for="p in addon.permissions" :key="p" class="flex items-start gap-2">
              <ShieldCheck class="mt-0.5 size-4 shrink-0 text-muted-foreground" />
              <span>{{ PERMISSION_KEYS[p as Permission] ? $t(`addons.permissions.${PERMISSION_KEYS[p as Permission]}`) : p }}</span>
            </li>
          </ul>
          <p v-else class="text-muted-foreground">{{ $t('addons.noPermissions') }}</p>
        </section>
        <section v-if="addon.hooks.length" class="grid gap-2">
          <h3 class="font-medium">{{ $t('addons.hooksTitle') }}</h3>
          <div class="flex flex-wrap gap-1.5"><Badge v-for="h in addon.hooks" :key="h" variant="outline">{{ h }}</Badge></div>
        </section>
        <p class="text-xs text-muted-foreground">{{ $t('addons.registryCheckHint') }}</p>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="open = false">{{ $t('common.cancel') }}</Button>
        <Button :disabled="installing || !addon.compatible || !!addon.blocked" @click="install">
          {{ addon.installedVersion ? $t('addons.update') : $t('addons.install') }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
