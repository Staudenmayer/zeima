<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Pencil, Plus, Trash2 } from '@lucide/vue'
import type { BudgetPeriod, Project, ProjectInput, ProjectStatus, Tag } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Separator } from '@/components/ui/separator'
import { Switch } from '@/components/ui/switch'
import { nextColor } from '@/lib/colors'
import { errorMessage } from '@/lib/ipc'
import { useCostCentersStore } from '@/stores/costCenters'
import { projectToInput, useProjectsStore } from '@/stores/projects'
import { useTagsStore } from '@/stores/tags'
import ColorPicker from './ColorPicker.vue'
import ConfirmDialog from './ConfirmDialog.vue'
import CostCenterSelect from './CostCenterSelect.vue'
import DurationInput from './DurationInput.vue'
import ProjectSelect from './ProjectSelect.vue'
import TagDialog from './TagDialog.vue'

const props = defineProps<{ project: Project | null }>()
const open = defineModel<boolean>('open', { required: true })

const projects = useProjectsStore()
const costCenters = useCostCentersStore()
const tags = useTagsStore()

const form = ref<ProjectInput>(empty())
/** Budget is edited in hours, stored in minutes. */
const budgetMinutes = ref<number | null>(null)
const saving = ref(false)

const periods: BudgetPeriod[] = ['total', 'monthly', 'weekly']
const statuses: ProjectStatus[] = ['active', 'paused', 'archived']

function empty(): ProjectInput {
  return {
    name: '',
    alias: null,
    color: nextColor(projects.all.map((p) => p.color)),
    parentId: null,
    client: null,
    budgetMinutes: null,
    budgetPeriod: 'total',
    budgetWarnPct: 80,
    billableDefault: false,
    status: 'active',
    costCenterId: null,
  }
}

watch(open, (isOpen) => {
  if (!isOpen) return
  form.value = props.project ? projectToInput(props.project) : empty()
  budgetMinutes.value = form.value.budgetMinutes
})

const textField = (key: 'alias' | 'client') =>
  computed({
    get: () => form.value[key] ?? '',
    set: (v: string | number) => (form.value[key] = String(v).trim() || null),
  })
const alias = textField('alias')
const client = textField('client')

async function save() {
  const input = { ...form.value, budgetMinutes: budgetMinutes.value }
  saving.value = true
  try {
    if (props.project) await projects.update(props.project.id, input)
    else await projects.create(input)
    open.value = false
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    saving.value = false
  }
}

// Project-scoped tags
const projectTags = computed(() => (props.project ? tags.all.filter((t) => t.projectId === props.project!.id) : []))
const tagDialogOpen = ref(false)
const editingTag = ref<Tag | null>(null)
const deletingTag = ref<Tag | null>(null)
const confirmTagOpen = ref(false)

function openTag(tag: Tag | null) {
  editingTag.value = tag
  tagDialogOpen.value = true
}

function askDeleteTag(tag: Tag) {
  deletingTag.value = tag
  confirmTagOpen.value = true
}

async function deleteTag() {
  if (!deletingTag.value) return
  try {
    await tags.remove(deletingTag.value.id)
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent :aria-describedby="undefined" class="max-h-[90vh] overflow-y-auto sm:max-w-xl">
      <DialogHeader>
        <DialogTitle>{{ project ? $t('projects.edit') : $t('projects.new') }}</DialogTitle>
      </DialogHeader>
      <form class="grid gap-4" @submit.prevent="save">
        <div class="grid grid-cols-3 gap-3">
          <div class="col-span-2 grid gap-2">
            <Label for="project-name">{{ $t('projects.name') }}</Label>
            <Input id="project-name" v-model="form.name" required autofocus />
          </div>
          <div class="grid gap-2">
            <Label for="project-alias">{{ $t('projects.alias') }}</Label>
            <Input id="project-alias" v-model="alias" class="font-mono" maxlength="32" />
          </div>
        </div>
        <p class="-mt-2 text-xs text-muted-foreground">{{ $t('projects.aliasHint') }}</p>
        <div class="grid gap-2">
          <Label>{{ $t('projects.color') }}</Label>
          <ColorPicker v-model="form.color" />
        </div>
        <div class="grid grid-cols-2 gap-3">
          <div class="grid gap-2">
            <Label for="project-client">{{ $t('projects.client') }}</Label>
            <Input id="project-client" v-model="client" />
          </div>
          <div class="grid gap-2">
            <Label>{{ $t('projects.parent') }}</Label>
            <ProjectSelect v-model="form.parentId" :exclude="project ? [project.id] : []" />
          </div>
        </div>
        <div v-if="costCenters.all.length" class="grid gap-2">
          <Label>{{ $t('costCenters.projectDefault') }}</Label>
          <CostCenterSelect :model-value="form.costCenterId ?? null" :empty-label="$t('common.none')" @update:model-value="form.costCenterId = $event" />
        </div>
        <div class="grid grid-cols-3 gap-3">
          <div class="grid gap-2">
            <Label for="project-budget">{{ $t('projects.budget') }} ({{ $t('projects.budgetHours') }})</Label>
            <DurationInput id="project-budget" v-model="budgetMinutes" />
          </div>
          <div class="grid gap-2">
            <Label>{{ $t('projects.budgetPeriod') }}</Label>
            <Select v-model="form.budgetPeriod">
              <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem v-for="p in periods" :key="p" :value="p">{{ $t(`projects.periods.${p}`) }}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div class="grid gap-2">
            <Label for="project-warn">{{ $t('projects.warnAt') }}</Label>
            <Input id="project-warn" v-model.number="form.budgetWarnPct" type="number" min="1" max="100" />
          </div>
        </div>
        <div class="grid grid-cols-2 items-end gap-3">
          <div class="grid gap-2">
            <Label>{{ $t('projects.status') }}</Label>
            <Select v-model="form.status">
              <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem v-for="s in statuses" :key="s" :value="s">{{ $t(`projects.statuses.${s}`) }}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <label class="flex h-9 items-center gap-2 text-sm">
            <Switch v-model="form.billableDefault" />
            {{ $t('projects.billableDefault') }}
          </label>
        </div>

        <Separator />
        <div class="grid gap-2">
          <div class="flex items-center justify-between">
            <div>
              <Label>{{ $t('projects.projectTags') }}</Label>
              <p class="text-xs text-muted-foreground">{{ $t('projects.projectTagsHint') }}</p>
            </div>
            <Button v-if="project" type="button" size="sm" variant="outline" @click="openTag(null)">
              <Plus /> {{ $t('projects.addTag') }}
            </Button>
          </div>
          <p v-if="!project" class="text-sm text-muted-foreground">{{ $t('projects.saveFirst') }}</p>
          <p v-else-if="projectTags.length === 0" class="text-sm text-muted-foreground">{{ $t('projects.noTags') }}</p>
          <ul v-else class="divide-y rounded-md border">
            <li v-for="tag in projectTags" :key="tag.id" class="flex items-center gap-2 px-3 py-1.5 text-sm">
              <span class="size-2.5 rounded-full" :style="{ backgroundColor: tag.color }" />
              <span class="flex-1">{{ tag.name }}</span>
              <span v-if="tag.groupName" class="text-xs text-muted-foreground">{{ tag.groupName }}</span>
              <Button type="button" size="icon" variant="ghost" class="size-7" @click="openTag(tag)"><Pencil /></Button>
              <Button type="button" size="icon" variant="ghost" class="size-7" @click="askDeleteTag(tag)"><Trash2 /></Button>
            </li>
          </ul>
        </div>

        <DialogFooter>
          <Button type="button" variant="outline" @click="open = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit" :disabled="saving">{{ $t('common.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
  <TagDialog v-model:open="tagDialogOpen" :tag="editingTag" :project-id="project?.id ?? null" fixed-scope />
  <ConfirmDialog
    v-model:open="confirmTagOpen"
    :title="$t('tags.deleteConfirm', { name: deletingTag?.name ?? '' })"
    @confirm="deleteTag"
  />
</template>
