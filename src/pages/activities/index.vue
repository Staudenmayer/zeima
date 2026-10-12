<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Archive, ArchiveRestore, Pencil, Plus, Shapes, Trash2 } from '@lucide/vue'
import type { Activity } from '@/bindings'
import { Button } from '@/components/ui/button'
import ActivityDialog from '@/components/time/ActivityDialog.vue'
import ConfirmDialog from '@/components/time/ConfirmDialog.vue'
import EmptyState from '@/components/time/EmptyState.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import { nextColor } from '@/lib/colors'
import { errorMessage } from '@/lib/ipc'
import { useActivitiesStore } from '@/stores/activities'

definePage({ meta: { titleKey: 'nav.activities' } })

const { t } = useI18n()
const activities = useActivitiesStore()

const archived = computed(() => activities.all.filter((a) => a.archived))

/** Common activities not created yet, offered as one-click starters. */
const suggestions = computed(() => {
  const taken = new Set(activities.all.map((a) => a.name.toLowerCase()))
  return t('activities.suggestions')
    .split(',')
    .map((name) => name.trim())
    .filter((name) => name && !taken.has(name.toLowerCase()))
})

async function addSuggestion(name: string) {
  try {
    await activities.create({ name, color: nextColor(activities.all.map((a) => a.color)), archived: false })
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

const dialogOpen = ref(false)
const editing = ref<Activity | null>(null)
function open(activity: Activity | null) {
  editing.value = activity
  dialogOpen.value = true
}

async function setArchived(activity: Activity, value: boolean) {
  try {
    await activities.update(activity.id, { name: activity.name, color: activity.color, archived: value })
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

const deleting = ref<Activity | null>(null)
const confirmOpen = ref(false)
function askDelete(activity: Activity) {
  deleting.value = activity
  confirmOpen.value = true
}
async function confirmDelete() {
  if (!deleting.value) return
  try {
    await activities.remove(deleting.value.id)
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
</script>

<template>
  <PageHeader :title="$t('nav.activities')">
    <Button @click="open(null)"><Plus /> {{ $t('activities.new') }}</Button>
  </PageHeader>

  <EmptyState v-if="activities.active.length === 0" :icon="Shapes" :text="$t('activities.empty')">
    <Button variant="outline" @click="open(null)"><Plus /> {{ $t('activities.new') }}</Button>
  </EmptyState>

  <div class="grid gap-6">
    <ul v-if="activities.active.length" class="divide-y rounded-lg border">
      <li
        v-for="activity in activities.active"
        :key="activity.id"
        class="group flex items-center gap-3 px-3 py-2 text-sm hover:bg-muted/40"
      >
        <span class="size-3 rounded-sm" :style="{ backgroundColor: activity.color }" />
        <span class="flex-1">{{ activity.name }}</span>
        <div class="flex opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
          <Button size="icon" variant="ghost" class="size-7" :title="$t('common.edit')" @click="open(activity)"><Pencil /></Button>
          <Button size="icon" variant="ghost" class="size-7" :title="$t('activities.archive')" @click="setArchived(activity, true)"><Archive /></Button>
          <Button size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="askDelete(activity)"><Trash2 /></Button>
        </div>
      </li>
    </ul>

    <section v-if="suggestions.length">
      <h2 class="mb-2 text-sm font-medium">{{ $t('activities.suggested') }}</h2>
      <div class="flex flex-wrap gap-2">
        <Button v-for="name in suggestions" :key="name" size="sm" variant="outline" @click="addSuggestion(name)">
          <Plus /> {{ name }}
        </Button>
      </div>
    </section>

    <section v-if="archived.length">
      <h2 class="mb-2 text-sm font-medium text-muted-foreground">{{ $t('activities.archivedSection') }}</h2>
      <ul class="divide-y rounded-lg border">
        <li v-for="activity in archived" :key="activity.id" class="group flex items-center gap-3 px-3 py-2 text-sm text-muted-foreground hover:bg-muted/40">
          <span class="size-3 rounded-sm opacity-60" :style="{ backgroundColor: activity.color }" />
          <span class="flex-1">{{ activity.name }}</span>
          <div class="flex opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
            <Button size="icon" variant="ghost" class="size-7" :title="$t('activities.unarchive')" @click="setArchived(activity, false)"><ArchiveRestore /></Button>
            <Button size="icon" variant="ghost" class="size-7" :title="$t('common.delete')" @click="askDelete(activity)"><Trash2 /></Button>
          </div>
        </li>
      </ul>
    </section>
  </div>

  <ActivityDialog v-model:open="dialogOpen" :activity="editing" />
  <ConfirmDialog
    v-model:open="confirmOpen"
    :title="$t('activities.deleteConfirm', { name: deleting?.name ?? '' })"
    @confirm="confirmDelete"
  />
</template>
