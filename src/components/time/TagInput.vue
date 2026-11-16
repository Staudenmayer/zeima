<script setup lang="ts">
import { onClickOutside } from '@vueuse/core'
import { Check, Plus, X } from '@lucide/vue'
import { computed, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { errorMessage } from '@/lib/ipc'
import { nextColor } from '@/lib/colors'
import { useTagsStore } from '@/stores/tags'
import { useWorkspaceStore } from '@/stores/workspace'

// Tags as a field you type into: the usable tags are suggested as you type, a pick becomes a chip
// in the field. Only the first few chips are shown, the rest fold into "+N" (their names in its tooltip).
const props = defineProps<{ projectId: string | null }>()
const selected = defineModel<string[]>({ required: true })
const SHOW = 2

const tags = useTagsStore()
const workspaces = useWorkspaceStore()
const usable = computed(() => tags.usableFor(props.projectId))
const picked = computed(() => selected.value.map((id) => tags.byId.get(id)).filter((t) => t !== undefined))
const rest = computed(() => picked.value.slice(SHOW))

// Drop project tags that no longer apply after switching the project.
watch(usable, (list) => {
  const ids = new Set(list.map((t) => t.id))
  if (selected.value.some((id) => !ids.has(id))) selected.value = selected.value.filter((id) => ids.has(id))
})

const query = ref('')
const open = ref(false)
const root = ref<HTMLElement | null>(null)
onClickOutside(root, () => (open.value = false))

const options = computed(() => {
  const q = query.value.trim().toLowerCase()
  return usable.value.filter((t) => !q || t.name.toLowerCase().includes(q))
})
const typed = computed(() => query.value.trim())
const canCreate = computed(
  () => typed.value !== '' && workspaces.can('tag.write') && !usable.value.some((t) => t.name.toLowerCase() === typed.value.toLowerCase()),
)

function toggle(id: string) {
  selected.value = selected.value.includes(id) ? selected.value.filter((x) => x !== id) : [...selected.value, id]
  query.value = ''
}
async function create() {
  try {
    const tag = await tags.create({ name: typed.value, color: nextColor(tags.all.map((t) => t.color)), projectId: null, groupName: null })
    await tags.load()
    selected.value = [...selected.value, tag.id]
    query.value = ''
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
function onKey(e: KeyboardEvent) {
  if (e.key === 'Enter') {
    e.preventDefault()
    const first = options.value.find((t) => !selected.value.includes(t.id)) ?? options.value[0]
    if (first) toggle(first.id)
    else if (canCreate.value) void create()
  } else if (e.key === 'Backspace' && !query.value && selected.value.length) {
    selected.value = selected.value.slice(0, -1)
  } else if (e.key === 'Escape') {
    open.value = false
    query.value = ''
  }
}
</script>

<template>
  <div ref="root" class="relative">
    <div class="flex h-9 items-center gap-1 overflow-hidden rounded-md border border-input bg-transparent px-1.5 shadow-xs focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/50 dark:bg-input/30">
      <span
        v-for="tag in picked.slice(0, SHOW)"
        :key="tag.id"
        class="inline-flex shrink-0 items-center gap-1 rounded-full py-px pr-1 pl-2 text-[11px] leading-4 text-white"
        :style="{ backgroundColor: tag.color }"
      >{{ tag.name }}<button type="button" class="rounded-full p-0.5 hover:bg-black/20" :aria-label="$t('common.delete')" @click="toggle(tag.id)"><X class="size-3" /></button></span>
      <span v-if="rest.length" tabindex="0" class="shrink-0 cursor-default rounded-full bg-secondary px-2 py-px text-[11px] leading-4" :title="rest.map((t) => t.name).join(', ')" :aria-label="rest.map((t) => t.name).join(', ')">+{{ rest.length }}</span>
      <input
        v-model="query"
        type="text"
        role="combobox"
        autocomplete="off"
        :aria-expanded="open"
        :aria-label="$t('entry.tags')"
        :placeholder="picked.length ? '' : $t('entry.searchTags')"
        class="h-full min-w-14 flex-1 bg-transparent px-1 text-sm outline-none placeholder:text-muted-foreground"
        @focus="open = true"
        @input="open = true"
        @keydown="onKey"
      />
    </div>
    <ul v-if="open && (options.length || canCreate)" class="absolute top-full right-0 left-0 z-50 mt-1 max-h-56 overflow-y-auto rounded-md border bg-popover p-1 text-sm text-popover-foreground shadow-md">
      <li v-for="tag in options" :key="tag.id">
        <button type="button" class="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left hover:bg-accent" @click="toggle(tag.id)">
          <span class="size-2.5 shrink-0 rounded-full" :style="{ backgroundColor: tag.color }" />
          <span class="flex-1 truncate">{{ tag.name }}</span>
          <Check v-if="selected.includes(tag.id)" class="size-4" />
        </button>
      </li>
      <li v-if="canCreate">
        <button type="button" class="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left hover:bg-accent" @click="create"><Plus class="size-4" />{{ $t('entry.createTag', { name: typed }) }}</button>
      </li>
    </ul>
  </div>
</template>
