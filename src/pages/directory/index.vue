<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { toast } from 'vue-sonner'
import { Contact, Pencil, Search, X } from '@lucide/vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import EmptyState from '@/components/time/EmptyState.vue'
import MultiFilter, { type FilterOption } from '@/components/time/MultiFilter.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import SortHead from '@/components/time/SortHead.vue'
import { useSort } from '@/composables/useSort'
import { openUrl } from '@tauri-apps/plugin-opener'
import { errorMessage } from '@/lib/ipc'
import { CAN, type DirectoryEntry, useOrgStore } from '@/stores/org'
import { useWorkspaceStore } from '@/stores/workspace'

definePage({ meta: { titleKey: 'nav.directory' } })

// Everyone in the company: name, email, department and what they do. The
// dashboard's online list links here with `?user=<id>` for one person.
const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const org = useOrgStore()
const workspaces = useWorkspaceStore()

async function load() {
  const orgId = workspaces.active?.orgId
  if (!orgId) return
  try {
    org.activeId = orgId
    await Promise.all([org.loadDirectory(), org.loadJobs()])
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
onMounted(load)
watch(() => workspaces.active?.id, load)

// ---------------------------------------------------------------- filters
const search = ref('')
const departments = ref<(string | null)[]>([])
/** The person the page was opened for (`?user=`). */
const person = ref<string | null>(null)
watch(
  () => route.query.user,
  (u) => (person.value = typeof u === 'string' && u ? u : null),
  { immediate: true },
)
function clearPerson() {
  person.value = null
  void router.replace({ path: route.path })
}
const NONE = null
const departmentOptions = computed<FilterOption[]>(() => [
  ...[...new Set(org.directory.flatMap((e) => e.departments))].sort().map((d) => ({ value: d, label: d })),
  { value: NONE, label: t('directory.noDepartment') },
])
const hasFilter = computed(() => search.value !== '' || departments.value.length > 0 || person.value !== null)
function clearFilters() {
  search.value = ''
  departments.value = []
  clearPerson()
}

const nameOf = (e: DirectoryEntry) => e.name || e.email || t('team.someone')
const filtered = computed(() => {
  const q = search.value.trim().toLowerCase()
  return org.directory.filter(
    (e) =>
      (!person.value || e.id === person.value) &&
      (departments.value.length === 0 || (e.departments.length ? e.departments.some((d) => departments.value.includes(d)) : departments.value.includes(NONE))) &&
      (!q || [e.name, e.email, e.jobTitle, ...e.departments].some((x) => x?.toLowerCase().includes(q))),
  )
})
const { sort, sorted } = useSort(filtered, {
  name: (e) => nameOf(e),
  email: (e) => e.email,
  department: (e) => e.departments.join(', '),
  jobTitle: (e) => e.jobTitle,
})

/** Opens the person's mail program (the webview does not follow `mailto:` links itself). */
async function mail(address: string) {
  try {
    await openUrl(`mailto:${address}`)
  } catch (err) {
    toast.error(errorMessage(err))
  }
}

// ------------------------------------------------------------ job title
const canEditAll = computed(() => org.role !== null && CAN.manage(org.role))
const canEdit = (_e: DirectoryEntry) => canEditAll.value
const editing = ref<DirectoryEntry | null>(null)
const NO_JOB = '__none'
const draft = ref(NO_JOB)
const open = ref(false)
function edit(e: DirectoryEntry) {
  editing.value = e
  draft.value = e.jobId ?? NO_JOB
  open.value = true
}
async function save() {
  const e = editing.value
  if (!e) return
  try {
    await org.setJob(e.id, draft.value === NO_JOB ? null : draft.value)
    open.value = false
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
</script>

<template>
  <PageHeader :title="$t('nav.directory')" />

  <EmptyState v-if="!workspaces.inOrg" :icon="Contact" :text="$t('directory.personal')" />
  <template v-else>
    <div class="mb-3 flex flex-wrap items-center gap-2">
      <div class="relative">
        <Search class="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input v-model="search" class="h-8 w-64 pl-8" :placeholder="$t('directory.search')" />
      </div>
      <MultiFilter v-model="departments" :label="$t('team.department')" :options="departmentOptions" />
      <Badge v-if="person" variant="secondary" class="gap-1 py-1">
        {{ nameOf(org.directory.find((e) => e.id === person) ?? { id: person, name: null, email: null, departments: [], jobId: null, jobTitle: null, jobDescription: null }) }}
        <button type="button" class="rounded-sm hover:bg-background/60" :title="$t('reports.clearFilter')" @click="clearPerson"><X class="size-3" /></button>
      </Badge>
      <Button v-if="hasFilter" size="sm" variant="ghost" @click="clearFilters"><X /> {{ $t('reports.clearFilter') }}</Button>
    </div>

    <EmptyState v-if="org.directory.length === 0" :icon="Contact" :text="$t('directory.empty')" />
    <p v-else-if="sorted.length === 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('timesheet.noMatch') }}</p>
    <div v-else class="overflow-x-auto rounded-lg border">
      <table class="w-full text-sm">
        <thead class="text-left text-xs text-muted-foreground">
          <tr class="border-b">
            <SortHead :sort="sort" column="name" class="px-3 py-2 font-medium">{{ $t('org.col.member') }}</SortHead>
            <SortHead :sort="sort" column="email" class="px-3 py-2 font-medium">{{ $t('org.email') }}</SortHead>
            <SortHead :sort="sort" column="department" class="px-3 py-2 font-medium">{{ $t('team.department') }}</SortHead>
            <SortHead :sort="sort" column="jobTitle" class="px-3 py-2 font-medium">{{ $t('directory.jobTitle') }}</SortHead>
            <th class="w-12 px-1 py-2"><span class="sr-only">{{ $t('common.edit') }}</span></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="e in sorted" :key="e.id" class="group border-b last:border-b-0 hover:bg-muted/30" :class="canEdit(e) ? 'cursor-pointer' : ''" @click="canEdit(e) && edit(e)">
            <td class="whitespace-nowrap px-3 py-1.5 font-medium">{{ nameOf(e) }}</td>
            <td class="px-3 py-1.5">
              <a v-if="e.email" :href="`mailto:${e.email}`" class="text-muted-foreground hover:text-foreground hover:underline" @click.stop.prevent="mail(e.email)">{{ e.email }}</a>
              <span v-else class="text-muted-foreground">–</span>
            </td>
            <td class="px-3 py-1.5">
              <span v-if="e.departments.length" class="flex flex-wrap gap-1"><Badge v-for="d in e.departments" :key="d" variant="secondary">{{ d }}</Badge></span>
              <span v-else class="text-muted-foreground">–</span>
            </td>
            <td class="max-w-0 w-full truncate px-3 py-1.5" :title="e.jobDescription ?? undefined">{{ e.jobTitle || '–' }}</td>
            <td class="px-1 py-0.5" @click.stop>
              <div v-if="canEdit(e)" class="flex justify-end opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                <Button size="icon" variant="ghost" class="size-7" :title="$t('common.edit')" @click="edit(e)"><Pencil /></Button>
              </div>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
    <p class="mt-3 text-xs text-muted-foreground">{{ $t('directory.hint') }}</p>
  </template>

  <Dialog v-model:open="open">
    <DialogContent :aria-describedby="undefined" class="sm:max-w-md">
      <DialogHeader><DialogTitle>{{ editing ? nameOf(editing) : '' }}</DialogTitle></DialogHeader>
      <form class="grid gap-4" @submit.prevent="save">
        <div class="grid gap-2">
          <Label>{{ $t('directory.jobTitle') }}</Label>
          <Select v-model="draft">
            <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem :value="NO_JOB">{{ $t('common.none') }}</SelectItem>
              <SelectItem v-for="j in org.jobs" :key="j.id" :value="j.id">{{ j.name }}</SelectItem>
            </SelectContent>
          </Select>
          <p v-if="org.jobs.length === 0" class="text-xs text-muted-foreground">{{ $t('directory.noJobs') }}</p>
          <p v-else-if="org.jobs.find((j) => j.id === draft)?.description" class="text-xs text-muted-foreground">{{ org.jobs.find((j) => j.id === draft)?.description }}</p>
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" @click="open = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit">{{ $t('common.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
