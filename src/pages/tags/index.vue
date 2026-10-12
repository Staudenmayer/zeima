<script setup lang="ts">
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

definePage({ meta: { titleKey: 'nav.tags' } })

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
</script>

<template>
  <PageHeader :title="$t('nav.tags')">
    <Button @click="open(null)"><Plus /> {{ $t('tags.new') }}</Button>
  </PageHeader>

  <EmptyState v-if="tags.all.length === 0" :icon="Tags" :text="$t('tags.empty')">
    <Button variant="outline" @click="open(null)"><Plus /> {{ $t('tags.new') }}</Button>
  </EmptyState>

  <div v-else class="grid gap-6">
    <section v-for="section in sections" :key="section.projectId ?? 'global'">
      <h2 class="mb-2 text-sm font-medium">
        <span v-if="section.projectId === null">{{ $t('tags.global') }}</span>
        <ProjectDot v-else :project="section.project" show-alias />
      </h2>
      <ul class="divide-y rounded-lg border">
        <li v-for="tag in section.list" :key="tag.id" class="group flex items-center gap-3 px-3 py-2 text-sm hover:bg-muted/40">
          <span class="size-3 rounded-full" :style="{ backgroundColor: tag.color }" />
          <span class="flex-1">{{ tag.name }}</span>
          <span v-if="tag.groupName" class="text-xs text-muted-foreground">{{ $t('tags.group') }}: {{ tag.groupName }}</span>
          <div class="flex opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
            <Button size="icon" variant="ghost" class="size-7" :title="$t('common.edit')" @click="open(tag)"><Pencil /></Button>
            <Button size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="askDelete(tag)"><Trash2 /></Button>
          </div>
        </li>
      </ul>
    </section>
  </div>

  <TagDialog v-model:open="dialogOpen" :tag="editing" />
  <ConfirmDialog
    v-model:open="confirmOpen"
    :title="$t('tags.deleteConfirm', { name: deleting?.name ?? '' })"
    @confirm="confirmDelete"
  />
</template>
