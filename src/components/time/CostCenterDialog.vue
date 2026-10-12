<script setup lang="ts">
import { ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import type { CostCenter, CostCenterInput } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import DateInput from '@/components/time/DateInput.vue'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { errorMessage } from '@/lib/ipc'
import { useCostCentersStore } from '@/stores/costCenters'
import CostCenterSelect from './CostCenterSelect.vue'

const props = defineProps<{ costCenter: CostCenter | null }>()
const open = defineModel<boolean>('open', { required: true })

const costCenters = useCostCentersStore()
const empty = (): CostCenterInput => ({ code: '', name: '', parentId: null, active: true, validFrom: null, validTo: null })
const form = ref<CostCenterInput>(empty())
const saving = ref(false)

watch(open, (isOpen) => {
  if (!isOpen) return
  const c = props.costCenter
  form.value = c
    ? { code: c.code, name: c.name, parentId: c.parentId, active: c.active, validFrom: c.validFrom, validTo: c.validTo }
    : empty()
})

async function save() {
  saving.value = true
  try {
    await costCenters.save(props.costCenter?.id ?? null, {
      ...form.value,
      validFrom: form.value.validFrom || null,
      validTo: form.value.validTo || null,
    })
    open.value = false
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-md">
      <DialogHeader>
        <DialogTitle>{{ costCenter ? $t('costCenters.edit') : $t('costCenters.new') }}</DialogTitle>
      </DialogHeader>
      <form class="grid gap-4" @submit.prevent="save">
        <div class="grid grid-cols-[8rem_1fr] gap-3">
          <div class="grid gap-2">
            <Label for="cc-code">{{ $t('costCenters.code') }}</Label>
            <Input id="cc-code" v-model="form.code" class="font-mono" required autofocus />
          </div>
          <div class="grid gap-2">
            <Label for="cc-name">{{ $t('costCenters.name') }}</Label>
            <Input id="cc-name" v-model="form.name" required />
          </div>
        </div>
        <div class="grid gap-2">
          <Label>{{ $t('costCenters.parent') }}</Label>
          <CostCenterSelect
            v-model="form.parentId"
            :empty-label="$t('common.none')"
            :exclude="costCenter ? [costCenter.id] : []"
          />
        </div>
        <div class="grid grid-cols-2 gap-3">
          <div class="grid gap-2">
            <Label for="cc-from">{{ $t('costCenters.validFrom') }}</Label>
            <DateInput id="cc-from" :model-value="form.validFrom ?? ''" @update:model-value="form.validFrom = $event || null" />
          </div>
          <div class="grid gap-2">
            <Label for="cc-to">{{ $t('costCenters.validTo') }}</Label>
            <DateInput id="cc-to" :model-value="form.validTo ?? ''" @update:model-value="form.validTo = $event || null" />
          </div>
        </div>
        <label class="flex items-center justify-between gap-3 text-sm">
          <span>{{ $t('costCenters.active') }}</span>
          <Switch v-model="form.active" />
        </label>
        <DialogFooter>
          <Button type="button" variant="outline" @click="open = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit" :disabled="saving">{{ $t('common.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
