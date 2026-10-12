<script setup lang="ts">
import { onScopeDispose, ref } from 'vue'
import { commands, events } from '@/bindings'
import QuickSwitchPanel from '@/components/time/QuickSwitchPanel.vue'

// Runs in its own frameless, always-on-top window (see src-tauri/src/windows.rs).
definePage({ meta: { layout: 'bare' } })

const panel = ref<InstanceType<typeof QuickSwitchPanel> | null>(null)

const unlisten = events.quickSwitchOpened.listen(() => void panel.value?.reset())
onScopeDispose(() => void unlisten.then((fn) => fn()))

function close() {
  void commands.quickswitchHide()
}
</script>

<template>
  <div class="window-panel h-full overflow-hidden rounded-3xl border bg-popover text-popover-foreground">
    <QuickSwitchPanel ref="panel" @done="close" />
  </div>
</template>
