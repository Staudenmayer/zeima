<script setup lang="ts">
import { useAccountStore } from '@/stores/account'
import { useWorkspaceStore } from '@/stores/workspace'
import { computed, ref } from 'vue'
import { toast } from 'vue-sonner'
import { useI18n } from 'vue-i18n'
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
import SortHead from '@/components/time/SortHead.vue'
import { useSort } from '@/composables/useSort'
import TableToolbar from '@/components/time/TableToolbar.vue'
import MultiFilter, { type FilterOption } from '@/components/time/MultiFilter.vue'
import { passes, useTableSearch } from '@/composables/useTableSearch'

definePage({ meta: { titleKey: 'nav.projects' } })

const { t } = useI18n()
const workspaces = useWorkspaceStore()
const account = useAccountStore()

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

const clients = ref<(string | null)[]>([])
const statuses = ref<(string | null)[]>([])
const clientOptions = computed<FilterOption[]>(() => [...new Set(list.value.map((p) => p.client).filter((c): c is string => !!c))].sort().map((c) => ({ value: c, label: c })))
const statusOptions = computed<FilterOption[]>(() => [...new Set(list.value.map((p) => p.status))].sort().map((s) => ({ value: s, label: t(`projects.statuses.${s}`) })))
const { search, filtered, searching } = useTableSearch(
  list,
  (p) => [p.name, p.alias, p.client, p.status],
  (p) => passes(clients.value, p.client) && passes(statuses.value, p.status),
)
const narrowed = computed(() => searching.value || clients.value.length > 0 || statuses.value.length > 0)
const clearFilters = () => {
  search.value = ''
  clients.value = []
  statuses.value = []
}
const { sort, sorted: sortedList } = useSort(filtered, {
  name: (p) => p.name,
  client: (p) => p.client,
  budget: (p) => p.budgetMinutes,
  tags: (p) => tagCount(p.id),
  status: (p) => p.status,
})
</script>

<template>
  <PageHeader :title="$t('nav.projects')">
    <label class="flex items-center gap-2 text-sm">
      <Switch v-model="showArchived" />
      {{ $t('projects.showArchived') }}
    </label>
    <Button v-if="workspaces.can('project.write')" @click="open(null)"><Plus /> {{ $t('projects.new') }}</Button>
  </PageHeader>

  <EmptyState v-if="!workspaces.can('project.read')" :icon="FolderKanban" :text="$t('noAccess.projects')" />

  <EmptyState v-else-if="list.length === 0" :icon="FolderKanban" :text="$t('projects.empty')">
    <Button v-if="workspaces.can('project.write')" variant="outline" @click="open(null)"><Plus /> {{ $t('projects.new') }}</Button>
  </EmptyState>

  <template v-else>
  <TableToolbar v-model="search" :active="narrowed" @clear="clearFilters">
    <MultiFilter v-if="clientOptions.length" v-model="clients" :label="$t('projects.client')" :options="clientOptions" />
    <MultiFilter v-model="statuses" :label="$t('projects.status')" :options="statusOptions" />
  </TableToolbar>
  <p v-if="filtered.length === 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('table.noMatch') }}</p>
  <div v-else class="overflow-x-auto rounded-lg border">
    <table class="w-full text-sm">
      <thead class="text-left text-xs text-muted-foreground">
        <tr class="border-b">
          <SortHead :sort="sort" column="name" class="px-3 py-2 font-medium">{{ $t('projects.name') }}</SortHead>
          <SortHead :sort="sort" column="client" class="px-3 py-2 font-medium">{{ $t('projects.client') }}</SortHead>
          <SortHead :sort="sort" column="budget" class="px-3 py-2 font-medium">{{ $t('projects.budget') }}</SortHead>
          <SortHead :sort="sort" column="tags" class="px-3 py-2 font-medium">{{ $t('nav.tags') }}</SortHead>
          <SortHead :sort="sort" column="status" class="px-3 py-2 font-medium">{{ $t('projects.status') }}</SortHead>
          <th class="w-20 px-1 py-2"><span class="sr-only">{{ $t('common.edit') }}</span></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="p in sortedList" :key="p.id" class="group cursor-pointer border-b last:border-b-0 hover:bg-muted/30" @click="(workspaces.can('project.write') || p.managerId === account.status.user?.id) && open(p)">
          <td class="px-3 py-1.5">
            <div class="flex items-center gap-2">
              <ProjectDot :project="p" show-alias />
              <span v-if="p.parentId" class="text-xs text-muted-foreground">↳ {{ projects.byId.get(p.parentId)?.name }}</span>
            </div>
          </td>
          <td class="px-3 py-1.5 text-muted-foreground">{{ p.client ?? '–' }}</td>
          <td class="px-3 py-1.5 tabular-nums text-muted-foreground">
            <template v-if="budgetLabel(p)">{{ budgetLabel(p) }} · {{ $t(`projects.periods.${p.budgetPeriod}`) }}</template>
            <template v-else>–</template>
          </td>
          <td class="px-3 py-1.5 text-muted-foreground">{{ tagCount(p.id) || '–' }}</td>
          <td class="px-3 py-1.5">
            <Badge :variant="p.status === 'active' ? 'secondary' : 'outline'">{{ $t(`projects.statuses.${p.status}`) }}</Badge>
          </td>
          <td class="px-1 py-0.5" @click.stop>
            <div class="flex justify-end opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
              <Button v-if="workspaces.can('project.write') || p.managerId === account.status.user?.id" size="icon" variant="ghost" class="size-7" :title="$t('common.edit')" @click="open(p)"><Pencil /></Button>
              <Button v-if="workspaces.can('project.delete')" size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="askDelete(p)"><Trash2 /></Button>
            </div>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
  </template>

  <ProjectDialog v-model:open="dialogOpen" :project="editing" />
  <ConfirmDialog
    v-model:open="confirmOpen"
    :title="$t('projects.deleteConfirm', { name: deleting?.name ?? '' })"
    @confirm="confirmDelete"
  />
</template>
