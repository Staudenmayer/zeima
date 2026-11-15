<script setup lang="ts">
import { openUrl } from '@tauri-apps/plugin-opener'
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { Badge } from '@/components/ui/badge'
import UserAvatar from '@/components/time/UserAvatar.vue'
import { errorMessage } from '@/lib/ipc'
import { PRESENCE_COLORS } from '@/lib/presence'
import { useOrgStore } from '@/stores/org'
import { useProjectsStore } from '@/stores/projects'

// A person's profile: picture, name, email (opens the mail program), status, departments and job
// description. Shown from the online sidebar and from a name in a meeting.
const props = defineProps<{ userId: string }>()
const { t } = useI18n()
const org = useOrgStore()
const projects = useProjectsStore()

const entry = computed(() => org.directory.find((e) => e.id === props.userId) ?? null)
const name = computed(() => entry.value?.name || org.nameOf(props.userId) || entry.value?.email || t('team.someone'))
const presence = computed(() => org.presence.find((p) => p.userId === props.userId) ?? null)
const state = computed(() => presence.value?.state ?? 'offline')
const statusText = computed(() => (presence.value ? t(`org.presenceStates.${presence.value.state}`) : t('org.offline')))
const project = computed(() => (presence.value?.projectId ? (projects.byId.get(presence.value.projectId)?.name ?? null) : null))
/** The role is known only for those who may read the members (an administrator, a manager). */
const role = computed(() => org.members.find((m) => m.id === props.userId)?.role ?? null)

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
  <div class="overflow-hidden rounded-[inherit] text-sm">
    <div class="h-14 bg-gradient-to-br from-primary to-primary/55" />
    <div class="grid gap-3 px-4 pb-4">
      <span class="relative -mt-7 inline-flex w-fit">
        <UserAvatar :user-id="userId" :name="name" size="lg" class="size-16 border-4 border-popover text-2xl" />
        <span
          class="absolute right-0.5 bottom-0.5 aspect-square size-4 rounded-full ring-[3px] ring-popover"
          :style="{ backgroundColor: PRESENCE_COLORS[state] }"
          :title="statusText"
        />
      </span>
      <div class="grid gap-1">
        <div class="flex flex-wrap items-center gap-2">
          <span class="text-lg font-semibold">{{ name }}</span>
          <Badge v-if="role" variant="secondary">{{ org.roleName(role) }}</Badge>
        </div>
        <a
          v-if="entry?.email"
          :href="`mailto:${entry.email}`"
          class="w-fit text-muted-foreground underline underline-offset-2 hover:text-foreground"
          :title="$t('directory.writeEmail')"
          @click.prevent="mail(entry.email)"
        >{{ entry.email }}</a>
      </div>
      <div class="h-px bg-border" />
      <dl class="grid gap-2.5">
        <div class="grid gap-0.5">
          <dt class="text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">{{ $t('directory.status') }}</dt>
          <dd class="flex items-center gap-2">
            <span class="size-2.5 rounded-full" :style="{ backgroundColor: PRESENCE_COLORS[state] }" />{{ statusText }}
          </dd>
          <dd v-if="project" class="text-muted-foreground">{{ $t('directory.workingOn', { project }) }}</dd>
        </div>
        <div class="grid gap-0.5">
          <dt class="text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">{{ $t('team.department') }}</dt>
          <dd>{{ entry?.departments.length ? entry.departments.join(', ') : $t('directory.noDepartment') }}</dd>
        </div>
        <div class="grid gap-0.5">
          <dt class="text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">{{ $t('directory.jobDescription') }}</dt>
          <dd class="font-medium">{{ entry?.jobTitle || '–' }}</dd>
          <dd v-if="entry?.jobDescription" class="text-muted-foreground">{{ entry.jobDescription }}</dd>
        </div>
      </dl>
    </div>
  </div>
</template>
