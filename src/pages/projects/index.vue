<script setup lang="ts">
import { computed, ref } from 'vue'
import { toast } from 'vue-sonner'
import { FolderKanban, Pencil, Plus, Trash2 } from '@lucide/vue'
import type { Project } from '@/bindings'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import EmptyState from '@/components/time/EmptyState.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import ProjectDialog from '@/components/time/ProjectDialog.vue'
import ProjectDot from '@/components/time/ProjectDot.vue'
import { errorMessage } from '@/lib/ipc'
import { useProjectsStore } from '@/stores/projects'
import { useSettingsStore } from '@/stores/settings'
import { useTagsStore } from '@/stores/tags'

definePage({ meta: { titleKey: 'nav.projects' } })

const projects = useProjectsStore()
const tags = useTagsStore()
const settings = useSettingsStore()

const showArchived = ref(false)
const list = computed(() => (showArchived.value ? projects.all : projects.active))

const dialogOpen = ref(false)
const editing = ref<Project | null>(null)
function open(project: Project | null) {
  editing.value = project
  dialogOpen.value = true
}

const deleting = ref<Project | null>(null)
const confirmOpen = ref(false)
function askDelete(project: Project) {
  deleting.value = project
  confirmOpen.value = true
}
async function confirmDelete() {
  if (!deleting.value) return
  try {
    await projects.remove(deleting.value.id)
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

const tagCount = (id: string) => tags.all.filter((t) => t.projectId === id).length
const budgetLabel = (p: Project) => (p.budgetMinutes === null ? null : `${settings.formatNumber(p.budgetMinutes / 60)} h`)
</script>

<template>
  <PageHeader :title="$t('nav.projects')">
    <label class="flex items-center gap-2 text-sm">
      <Switch v-model="showArchived" />
      {{ $t('projects.showArchived') }}
    </label>
    <Button @click="open(null)"><Plus /> {{ $t('projects.new') }}</Button>
  </PageHeader>

  <EmptyState v-if="list.length === 0" :icon="FolderKanban" :text="$t('projects.empty')">
    <Button variant="outline" @click="open(null)"><Plus /> {{ $t('projects.new') }}</Button>
  </EmptyState>

  <div v-else class="overflow-hidden rounded-lg border">
    <table class="w-full text-sm">
      <thead class="bg-muted/50 text-left text-xs text-muted-foreground">
        <tr>
          <th class="px-3 py-2 font-medium">{{ $t('projects.name') }}</th>
          <th class="px-3 py-2 font-medium">{{ $t('projects.client') }}</th>
          <th class="px-3 py-2 font-medium">{{ $t('projects.budget') }}</th>
          <th class="px-3 py-2 font-medium">{{ $t('nav.tags') }}</th>
          <th class="px-3 py-2 font-medium">{{ $t('projects.status') }}</th>
          <th class="w-20" />
        </tr>
      </thead>
      <tbody class="divide-y">
        <tr v-for="p in list" :key="p.id" class="group hover:bg-muted/40" @dblclick="open(p)">
          <td class="px-3 py-2">
            <div class="flex items-center gap-2">
              <ProjectDot :project="p" show-alias />
              <span v-if="p.parentId" class="text-xs text-muted-foreground">↳ {{ projects.byId.get(p.parentId)?.name }}</span>
            </div>
          </td>
          <td class="px-3 py-2 text-muted-foreground">{{ p.client ?? '–' }}</td>
          <td class="px-3 py-2 tabular-nums text-muted-foreground">
            <template v-if="budgetLabel(p)">{{ budgetLabel(p) }} · {{ $t(`projects.periods.${p.budgetPeriod}`) }}</template>
            <template v-else>–</template>
          </td>
          <td class="px-3 py-2 text-muted-foreground">{{ tagCount(p.id) || '–' }}</td>
          <td class="px-3 py-2">
            <Badge :variant="p.status === 'active' ? 'secondary' : 'outline'">{{ $t(`projects.statuses.${p.status}`) }}</Badge>
          </td>
          <td class="px-2 py-1 text-right">
            <div class="flex justify-end opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
              <Button size="icon" variant="ghost" class="size-7" :title="$t('common.edit')" @click="open(p)"><Pencil /></Button>
              <Button size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="askDelete(p)"><Trash2 /></Button>
            </div>
          </td>
        </tr>
      </tbody>
    </table>
  </div>

  <ProjectDialog v-model:open="dialogOpen" :project="editing" />
  <ConfirmDialog
    v-model:open="confirmOpen"
    :title="$t('projects.deleteConfirm', { name: deleting?.name ?? '' })"
    @confirm="confirmDelete"
  />
</template>
