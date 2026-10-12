<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { commands, type AddonInfo } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { call, errorMessage } from '@/lib/ipc'

// The settings form an addon declares in addon.toml.
const props = defineProps<{ addon: AddonInfo | null }>()
const open = defineModel<boolean>('open', { required: true })
const emit = defineEmits<{ saved: [] }>()

const { t } = useI18n()
const values = ref<Record<string, string>>({})
const saving = ref(false)

watch(open, (isOpen) => {
  if (!isOpen || !props.addon?.manifest) return
  const saved = props.addon.record.settings
  values.value = Object.fromEntries(
    (props.addon.manifest.settings ?? []).map((f) => [f.key, saved[f.key] ?? f.default ?? (f.type === 'boolean' ? 'false' : '')]),
  )
})

async function save() {
  if (!props.addon) return
  saving.value = true
  try {
    await call(commands.addonSaveSettings(props.addon.record.id, values.value))
    open.value = false
    toast.success(t('common.saved'))
    emit('saved')
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent v-if="addon?.manifest" :aria-describedby="undefined" class="sm:max-w-lg">
      <DialogHeader>
        <DialogTitle>{{ $t('addons.settingsTitle', { name: addon.manifest.name }) }}</DialogTitle>
      </DialogHeader>
      <form class="grid gap-4" @submit.prevent="save">
        <div v-for="field in (addon.manifest.settings ?? [])" :key="field.key" class="grid gap-2">
          <template v-if="field.type === 'boolean'">
            <label class="flex items-center justify-between gap-3 text-sm">
              <span>{{ field.label }}</span>
              <Switch
                :model-value="values[field.key] === 'true'"
                @update:model-value="values[field.key] = $event ? 'true' : 'false'"
              />
            </label>
          </template>
          <template v-else>
            <Label :for="`setting-${field.key}`">{{ field.label }}<span v-if="field.required"> *</span></Label>
            <Select v-if="field.type === 'select'" v-model="values[field.key]">
              <SelectTrigger :id="`setting-${field.key}`" class="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem v-for="o in field.options" :key="o.value" :value="o.value">{{ o.label }}</SelectItem>
              </SelectContent>
            </Select>
            <Input
              v-else
              :id="`setting-${field.key}`"
              v-model="values[field.key]"
              :type="field.type === 'password' ? 'password' : 'text'"
              :inputmode="field.type === 'number' ? 'decimal' : undefined"
              :required="field.required"
              autocomplete="off"
            />
          </template>
          <p v-if="field.description" class="text-xs text-muted-foreground">{{ field.description }}</p>
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" @click="open = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit" :disabled="saving">{{ $t('common.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
