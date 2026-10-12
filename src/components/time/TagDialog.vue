<script setup lang="ts">
import { ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import type { Tag, TagInput } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { nextColor } from '@/lib/colors'
import { errorMessage } from '@/lib/ipc'
import { useTagsStore } from '@/stores/tags'
import ColorPicker from './ColorPicker.vue'
import ProjectSelect from './ProjectSelect.vue'

const props = defineProps<{
  tag: Tag | null
  /** Scope for new tags; `null` = global. */
  projectId?: string | null
  /** Hide the scope picker (e.g. when adding from a project). */
  fixedScope?: boolean
}>()
const open = defineModel<boolean>('open', { required: true })

const tags = useTagsStore()
const form = ref<TagInput>({ name: '', color: '', projectId: null, groupName: null })
const saving = ref(false)

watch(open, (isOpen) => {
  if (!isOpen) return
  form.value = props.tag
    ? { name: props.tag.name, color: props.tag.color, projectId: props.tag.projectId, groupName: props.tag.groupName }
    : { name: '', color: nextColor(tags.all.map((t) => t.color)), projectId: props.projectId ?? null, groupName: null }
})

async function save() {
  saving.value = true
  try {
    const input = { ...form.value, groupName: form.value.groupName?.trim() || null }
    if (props.tag) await tags.update(props.tag.id, input)
    else await tags.create(input)
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
        <DialogTitle>{{ tag ? $t('tags.edit') : $t('tags.new') }}</DialogTitle>
      </DialogHeader>
      <form class="grid gap-4" @submit.prevent="save">
        <div class="grid gap-2">
          <Label for="tag-name">{{ $t('tags.name') }}</Label>
          <Input id="tag-name" v-model="form.name" required autofocus />
        </div>
        <div class="grid gap-2">
          <Label>{{ $t('tags.color') }}</Label>
          <ColorPicker v-model="form.color" />
        </div>
        <div v-if="!fixedScope && !tag" class="grid gap-2">
          <Label>{{ $t('tags.scope') }}</Label>
          <ProjectSelect v-model="form.projectId" />
        </div>
        <div class="grid gap-2">
          <Label for="tag-group">{{ $t('tags.group') }}</Label>
          <Input id="tag-group" :model-value="form.groupName ?? ''" @update:model-value="form.groupName = String($event)" />
          <p class="text-xs text-muted-foreground">{{ $t('tags.groupHint') }}</p>
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" @click="open = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit" :disabled="saving">{{ $t('common.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
