import type { LucideIcon } from '@lucide/vue'
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute } from 'vue-router'
import {
  BarChart3,
  Clock,
  FolderKanban,
  Landmark,
  LayoutDashboard,
  MapPinned,
  Palmtree,
  Shapes,
  Tags,
  Users,
} from '@lucide/vue'
import { useApprovalsStore } from '@/stores/approvals'
import { useWorkspaceStore } from '@/stores/workspace'

export interface NavLeaf {
  title: string
  url: string
  icon: LucideIcon
}
/** One sidebar entry; with `members` it stands for several pages shown as tabs. */
export interface NavItem extends NavLeaf {
  members?: NavLeaf[]
}

// Pages that are the same kind of list share one entry in the sidebar and a tab strip on top.
const GROUPS = [
  { titleKey: 'nav.catalog', urls: ['/activities', '/projects', '/tags', '/cost-centers'] },
  { titleKey: 'nav.team', urls: ['/locations', '/team'] },
]

/** The tracking entries of the sidebar, grouped, and the tabs of the page that is open. */
export function useNav() {
  const { t } = useI18n()
  const route = useRoute()
  const workspaces = useWorkspaceStore()
  const approvals = useApprovalsStore()

  const flat = computed<NavLeaf[]>(() => [
    { title: t('nav.dashboard'), url: '/', icon: LayoutDashboard },
    { title: t('nav.timesheet'), url: '/timesheet', icon: Clock },
    { title: t('nav.activities'), url: '/activities', icon: Shapes },
    // What a role may not view is not shown (the pages say so, should it be opened anyway).
    ...(workspaces.can('project.read') ? [{ title: t('nav.projects'), url: '/projects', icon: FolderKanban }] : []),
    { title: t('nav.tags'), url: '/tags', icon: Tags },
    ...(workspaces.can('cost_center.read') ? [{ title: t('nav.costCenters'), url: '/cost-centers', icon: Landmark }] : []),
    { title: t('nav.reports'), url: '/reports', icon: BarChart3 },
    {
      title:
        approvals.waiting && workspaces.inOrg && workspaces.can('absence.approve')
          ? `${t('nav.absences')} (${approvals.waiting})`
          : t('nav.absences'),
      url: '/absences',
      icon: Palmtree,
    },
    ...(workspaces.inOrg ? [{ title: t('nav.locations'), url: '/locations', icon: MapPinned }] : []),
    // Those who may see other people's time (their department's or all); for the rest the page would show only themselves.
    ...(workspaces.inOrg && ['group', 'org'].includes(workspaces.scopeOf('entry.read') ?? '')
      ? [{ title: t('nav.team'), url: '/team', icon: Users }]
      : []),
  ])

  const items = computed<NavItem[]>(() => {
    const out: NavItem[] = []
    const seen = new Set<string>()
    for (const leaf of flat.value) {
      const g = GROUPS.find((x) => x.urls.includes(leaf.url))
      const members = g ? flat.value.filter((l) => g.urls.includes(l.url)) : []
      if (!g || members.length < 2) {
        out.push(leaf)
      } else if (!seen.has(g.titleKey)) {
        seen.add(g.titleKey)
        out.push({ title: t(g.titleKey), url: leaf.url, icon: leaf.icon, members })
      }
    }
    return out
  })

  /** The pages next to the open one, when it belongs to a group. */
  const tabs = computed<NavLeaf[]>(() => items.value.find((i) => i.members?.some((m) => route.path.startsWith(m.url)))?.members ?? [])

  return { items, tabs }
}
