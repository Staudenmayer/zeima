<script setup lang="ts">
import UserAvatar from '@/components/time/UserAvatar.vue'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { toast } from 'vue-sonner'
import { Contact, X } from '@lucide/vue'
import { Badge } from '@/components/ui/badge'
import EmptyState from '@/components/time/EmptyState.vue'
import MultiFilter, { type FilterOption } from '@/components/time/MultiFilter.vue'
import TableToolbar from '@/components/time/TableToolbar.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import PresenceDot from '@/components/time/PresenceDot.vue'
import SortHead from '@/components/time/SortHead.vue'
import { useSort } from '@/composables/useSort'
import { openUrl } from '@tauri-apps/plugin-opener'
import { events } from '@/bindings'
import { errorMessage } from '@/lib/ipc'
import { type DirectoryEntry, useOrgStore } from '@/stores/org'
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
    await Promise.all([org.loadDirectory(), org.loadPresence().catch(() => {})])
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
// Who is online: updates arrive as events; a quiet minute still ages out stale reports.
let unlisten: (() => void) | undefined
let refresh: ReturnType<typeof setInterval> | undefined
onMounted(async () => {
  await load()
  unlisten = await events.presenceChanged.listen((e) => {
    if (e.payload.orgId === org.activeId) void org.loadPresence().catch(() => {})
  })
  refresh = setInterval(() => void org.loadPresence().catch(() => {}), 60_000)
})
onUnmounted(() => {
  unlisten?.()
  clearInterval(refresh)
})
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
</script>

<template>
  <PageHeader :title="$t('nav.directory')" />

  <EmptyState v-if="!workspaces.inOrg" :icon="Contact" :text="$t('directory.personal')" />
  <template v-else>
    <TableToolbar v-model="search" :active="hasFilter" :placeholder="$t('directory.search')" @clear="clearFilters">
      <MultiFilter v-model="departments" :label="$t('team.department')" :options="departmentOptions" />
      <Badge v-if="person" variant="secondary" class="gap-1 py-1">
        <UserAvatar :user-id="person" size="xs" />
        {{ nameOf(org.directory.find((e) => e.id === person) ?? { id: person, name: null, email: null, departments: [], jobId: null, jobTitle: null, jobDescription: null }) }}
        <button type="button" class="rounded-sm hover:bg-background/60" :title="$t('reports.clearFilter')" @click="clearPerson"><X class="size-3" /></button>
      </Badge>
    </TableToolbar>

    <EmptyState v-if="org.directory.length === 0" :icon="Contact" :text="$t('directory.empty')" />
    <p v-else-if="sorted.length === 0" class="py-6 text-center text-sm text-muted-foreground">{{ $t('table.noMatch') }}</p>
    <div v-else class="overflow-x-auto rounded-lg border">
      <!-- Fixed column widths: searching or filtering must not make the columns jump or the cells wrap. -->
      <table class="w-full min-w-[56rem] table-fixed text-sm">
        <colgroup>
          <col class="w-[24%]" /><col class="w-[26%]" /><col class="w-[22%]" /><col class="w-[28%]" />
        </colgroup>
        <thead class="text-left text-xs text-muted-foreground">
          <tr class="border-b">
            <SortHead :sort="sort" column="name" class="px-3 py-2 font-medium">{{ $t('org.col.member') }}</SortHead>
            <SortHead :sort="sort" column="email" class="px-3 py-2 font-medium">{{ $t('org.email') }}</SortHead>
            <SortHead :sort="sort" column="department" class="px-3 py-2 font-medium">{{ $t('team.department') }}</SortHead>
            <SortHead :sort="sort" column="jobTitle" class="px-3 py-2 font-medium">{{ $t('directory.jobTitle') }}</SortHead>
          </tr>
        </thead>
        <tbody>
          <tr v-for="e in sorted" :key="e.id" class="border-b last:border-b-0 hover:bg-muted/30">
            <td class="px-3 py-1.5 font-medium"><span class="flex min-w-0 items-center gap-2.5"><span class="relative inline-flex shrink-0"><UserAvatar :user-id="e.id" :name="nameOf(e)" size="md" /><PresenceDot :user-id="e.id" class="absolute -right-0.5 -bottom-0.5 ring-2 ring-background" /></span><span class="truncate">{{ nameOf(e) }}</span></span></td>
            <td class="truncate px-3 py-1.5">
              <a v-if="e.email" :href="`mailto:${e.email}`" :title="e.email" class="text-muted-foreground hover:text-foreground hover:underline" @click.prevent="mail(e.email)">{{ e.email }}</a>
              <span v-else class="text-muted-foreground">–</span>
            </td>
            <td class="px-3 py-1.5">
              <span v-if="e.departments.length" class="flex gap-1 overflow-hidden" :title="e.departments.join(', ')"><Badge v-for="d in e.departments" :key="d" variant="secondary" class="shrink-0">{{ d }}</Badge></span>
              <span v-else class="text-muted-foreground">–</span>
            </td>
            <td class="truncate px-3 py-1.5" :title="e.jobDescription ?? undefined">{{ e.jobTitle || '–' }}</td>
          </tr>
        </tbody>
      </table>
    </div>
    <p class="mt-3 text-xs text-muted-foreground">{{ $t('directory.hint') }}</p>
  </template>
</template>
