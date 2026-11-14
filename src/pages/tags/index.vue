<script setup lang="ts">
import { useWorkspaceStore } from '@/stores/workspace'
import { computed, ref } from 'vue'
import { toast } from 'vue-sonner'
import { Pencil, Plus, Tags, Trash2 } from '@lucide/vue'
import type { Tag } from '@/bindings'
import { Button } from '@/components/ui/button'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import EmptyState from '@/components/time/EmptyState.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import ProjectDot from '@/components/time/ProjectDot.vue'
import TagDialog from '@/components/time/TagDialog.vue'
import { errorMessage } from '@/lib/ipc'
import { useProjectsStore } from '@/stores/projects'
import { useTagsStore } from '@/stores/tags'
import SortHead from '@/components/time/SortHead.vue'
import { useSort } from '@/composables/useSort'

definePage({ meta: { titleKey: 'nav.tags' } })

const workspaces = useWorkspaceStore()

const tags = useTagsStore()
const projects = useProjectsStore()

/** Global tags first, then one section per project with tags. */
const sections = computed(() => {
  const byProject = new Map<string | null, Tag[]>()
  for (const tag of tags.all) {
    byProject.set(tag.projectId, [...(byProject.get(tag.projectId) ?? []), tag])
  }
  return [...byProject.entries()]
    .map(([projectId, list]) => ({ projectId, project: projectId ? projects.byId.get(projectId) : null, list }))
    .sort((a, b) => (a.projectId === null ? -1 : b.projectId === null ? 1 : (a.project?.name ?? '').localeCompare(b.project?.name ?? '')))
})

const dialogOpen = ref(false)
const editing = ref<Tag | null>(null)
function open(tag: Tag | null) {
  editing.value = tag
  dialogOpen.value = true
}

const deleting = ref<Tag | null>(null)
const confirmOpen = ref(false)
function askDelete(tag: Tag) {
  deleting.value = tag
  confirmOpen.value = true
}
async function confirmDelete() {
  if (!deleting.value) return
  try {
    await tags.remove(deleting.value.id)
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

const { sort, apply: sortTags } = useSort<Tag>(null, { name: (t) => t.name, group: (t) => t.groupName })
</script>

<template>
  <PageHeader :title="$t('nav.tags')">
    <Button v-if="workspaces.canEditShared" @click="open(null)"><Plus /> {{ $t('tags.new') }}</Button>
  </PageHeader>

  <EmptyState v-if="tags.all.length === 0" :icon="Tags" :text="$t('tags.empty')">
    <Button v-if="workspaces.canEditShared" variant="outline" @click="open(null)"><Plus /> {{ $t('tags.new') }}</Button>
  </EmptyState>

  <div v-else class="grid gap-6">
    <section v-for="section in sections" :key="section.projectId ?? 'global'">
      <h2 class="mb-2 text-sm font-medium">
        <span v-if="section.projectId === null">{{ $t('tags.global') }}</span>
        <ProjectDot v-else :project="section.project" show-alias />
      </h2>
      <div class="overflow-x-auto rounded-lg border">
        <table class="w-full text-sm">
          <thead class="text-left text-xs text-muted-foreground">
            <tr class="border-b">
              <SortHead :sort="sort" column="name" class="px-3 py-2 font-medium">{{ $t('tags.name') }}</SortHead>
              <SortHead :sort="sort" column="group" class="px-3 py-2 font-medium">{{ $t('tags.group') }}</SortHead>
              <th class="w-20 px-1 py-2"><span class="sr-only">{{ $t('common.edit') }}</span></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="tag in sortTags(section.list)" :key="tag.id" class="group border-b last:border-b-0 hover:bg-muted/30" :class="workspaces.canEditShared ? 'cursor-pointer' : ''" @click="workspaces.canEditShared && open(tag)">
              <td class="px-3 py-1.5">
                <span class="flex items-center gap-2"><span class="size-3 shrink-0 rounded-full" :style="{ backgroundColor: tag.color }" />{{ tag.name }}</span>
              </td>
              <td class="px-3 py-1.5 text-muted-foreground">{{ tag.groupName || '–' }}</td>
              <td class="px-1 py-0.5" @click.stop>
                <div class="flex justify-end opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                  <Button v-if="workspaces.canEditShared" size="icon" variant="ghost" class="size-7" :title="$t('common.edit')" @click="open(tag)"><Pencil /></Button>
                  <Button v-if="workspaces.canEditShared" size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="askDelete(tag)"><Trash2 /></Button>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </section>
  </div>

  <TagDialog v-model:open="dialogOpen" :tag="editing" />
  <ConfirmDialog
    v-model:open="confirmOpen"
    :title="$t('tags.deleteConfirm', { name: deleting?.name ?? '' })"
    @confirm="confirmDelete"
  />
</template>
