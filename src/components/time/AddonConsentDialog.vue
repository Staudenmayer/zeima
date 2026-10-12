<script setup lang="ts">
import { ref } from 'vue'
import { toast } from 'vue-sonner'
import { AlertTriangle, Globe, ShieldCheck } from '@lucide/vue'
import { commands, type AddonPackage } from '@/bindings'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { HOOK_KEYS, PERMISSION_KEYS } from '@/lib/addons'
import { call, errorMessage } from '@/lib/ipc'

// Shows what a package asks for before it is installed.
const props = defineProps<{ pkg: AddonPackage | null }>()
const open = defineModel<boolean>('open', { required: true })
const emit = defineEmits<{ installed: [] }>()

const installing = ref(false)

async function install() {
  if (!props.pkg) return
  installing.value = true
  try {
    await call(commands.addonInstall(props.pkg.path))
    open.value = false
    emit('installed')
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    installing.value = false
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent v-if="pkg" class="sm:max-w-lg">
      <DialogHeader>
        <DialogTitle>{{ pkg.installedVersion ? $t('addons.updateTitle', { name: pkg.manifest.name }) : $t('addons.installTitle', { name: pkg.manifest.name }) }}</DialogTitle>
        <DialogDescription>
          {{ $t('addons.byAuthor', { version: pkg.manifest.version, author: pkg.manifest.author }) }}
          <template v-if="pkg.installedVersion"> · {{ $t('addons.replaces', { version: pkg.installedVersion }) }}</template>
        </DialogDescription>
      </DialogHeader>

      <div class="grid gap-4 text-sm">
        <p v-if="pkg.manifest.description">{{ pkg.manifest.description }}</p>

        <p v-if="!pkg.compatible" class="flex items-start gap-2 rounded-md bg-destructive/10 p-3 text-destructive">
          <AlertTriangle class="mt-0.5 size-4 shrink-0" />
          {{ $t('addons.incompatible', { version: pkg.manifest.min_app_version }) }}
        </p>

        <section class="grid gap-2">
          <h3 class="font-medium">{{ $t('addons.permissionsTitle') }}</h3>
          <ul v-if="(pkg.manifest.permissions ?? []).length" class="grid gap-1.5">
            <li v-for="p in (pkg.manifest.permissions ?? [])" :key="p" class="flex items-start gap-2">
              <ShieldCheck class="mt-0.5 size-4 shrink-0 text-muted-foreground" />
              <span>{{ $t(`addons.permissions.${PERMISSION_KEYS[p]}`) }}</span>
            </li>
          </ul>
          <p v-else class="text-muted-foreground">{{ $t('addons.noPermissions') }}</p>
          <div v-if="(pkg.manifest.allowed_hosts ?? []).length" class="flex flex-wrap items-center gap-1.5 pl-6">
            <Globe class="size-3.5 text-muted-foreground" />
            <Badge v-for="h in (pkg.manifest.allowed_hosts ?? [])" :key="h" variant="secondary" class="font-mono">{{ h }}</Badge>
          </div>
        </section>

        <section v-if="(pkg.manifest.hooks ?? []).length" class="grid gap-2">
          <h3 class="font-medium">{{ $t('addons.hooksTitle') }}</h3>
          <div class="flex flex-wrap gap-1.5">
            <Badge v-for="h in (pkg.manifest.hooks ?? [])" :key="h" variant="outline">{{ $t(`addons.hooks.${HOOK_KEYS[h]}`) }}</Badge>
          </div>
        </section>

        <p class="text-xs text-muted-foreground">{{ $t('addons.sandboxHint') }}</p>
      </div>

      <DialogFooter>
        <Button variant="outline" @click="open = false">{{ $t('common.cancel') }}</Button>
        <Button :disabled="installing || !pkg.compatible" @click="install">
          {{ pkg.installedVersion ? $t('addons.update') : $t('addons.install') }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
