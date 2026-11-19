<script setup lang="ts">
import { ShieldAlert, ShieldCheck, ShieldQuestion } from '@lucide/vue'
import { computed } from 'vue'
import type { SignatureCheck } from '@/bindings'

// Who made an addon, as far as the app can tell (docs/addons.md, "Signing and the registry").
const props = defineProps<{ signature: SignatureCheck }>()

const look = computed(() => {
  switch (props.signature.status) {
    case 'trusted':
      return { icon: ShieldCheck, cls: 'text-green-700 dark:text-green-400', key: 'trusted' }
    case 'server_trusted':
      return { icon: ShieldCheck, cls: 'text-sky-700 dark:text-sky-400', key: 'serverTrusted' }
    case 'unknown_key':
      return { icon: ShieldQuestion, cls: 'text-amber-700 dark:text-amber-400', key: 'unknownKey' }
    case 'invalid':
      return { icon: ShieldAlert, cls: 'text-destructive', key: 'invalid' }
    default:
      return { icon: ShieldQuestion, cls: 'text-muted-foreground', key: 'unsigned' }
  }
})
</script>

<template>
  <span class="inline-flex items-center gap-1 text-xs" :class="look.cls">
    <component :is="look.icon" class="size-3.5" />
    {{ $t(`addons.signature.${look.key}`, { signer: signature.signer ?? '' }) }}
  </span>
</template>
