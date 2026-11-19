<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { commands } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import CountryPicker from '@/components/time/CountryPicker.vue'
import PageHeader from '@/components/time/PageHeader.vue'
import WorkRecords from '@/components/time/WorkRecords.vue'
import SettingsNav from '@/components/time/SettingsNav.vue'
import { useFormat } from '@/composables/useFormat'
import { call, errorMessage } from '@/lib/ipc'
import { MINUTE } from '@/lib/time'
import { useSettingsStore } from '@/stores/settings'
import { useWorkStore } from '@/stores/work'
import { useWorkspaceStore } from '@/stores/workspace'

definePage({ meta: { titleKey: 'nav.settings' } })

const { t } = useI18n()
const work = useWorkStore()
const settings = useSettingsStore()
const format = useFormat()
const workspace = useWorkspaceStore()

// In an organization the schedule, vacation entitlement and overtime corrections are the organization's records: set
// under Organization → Work hours by someone who manages members, read-only here. In the personal workspace the
// person sets them (WorkRecords).
const managing = computed(() => workspace.inOrg && workspace.scopeOf('member.manage') === 'org')
const readOnly = computed(() => workspace.inOrg && !managing.value)

async function run<T>(promise: Promise<T>, success?: string): Promise<T | undefined> {
  try {
    const result = await promise
    if (success) toast.success(success)
    return result
  } catch (err) {
    toast.error(errorMessage(err))
    return undefined
  }
}

// ----- Country -----
const profileId = ref('AT')
const region = ref<string | null>(null)
watch(
  () => work.jurisdiction,
  (j) => {
    profileId.value = j?.profileId ?? 'AT'
    region.value = j?.region ?? null
  },
  { immediate: true },
)
const countryDirty = computed(
  () => profileId.value !== (work.jurisdiction?.profileId ?? '') || region.value !== (work.jurisdiction?.region ?? null),
)
const saveCountry = () => run(call(commands.jurisdictionSet(profileId.value, region.value)), t('common.saved'))

const hours = (minutes: number | null) => (minutes === null ? t('work.none') : `${settings.formatDuration(minutes * MINUTE)} h`)

</script>

<template>
  <PageHeader :title="$t('nav.settings')" />
  <SettingsNav />

  <div class="grid max-w-4xl gap-6">
    <p v-if="readOnly" class="rounded-lg border bg-muted/50 p-3 text-sm text-muted-foreground">{{ $t('work.managedBy') }}</p>
    <Card>
      <CardHeader>
        <CardTitle>{{ $t('work.country') }}</CardTitle>
        <CardDescription>{{ $t('work.countryHint') }}</CardDescription>
      </CardHeader>
      <CardContent class="grid gap-4">
        <CountryPicker v-model:profile-id="profileId" v-model:region="region" />
        <div class="flex justify-end">
          <Button :disabled="!countryDirty" @click="saveCountry">{{ $t('common.save') }}</Button>
        </div>
        <dl v-if="work.summary" class="grid gap-x-6 gap-y-2 rounded-lg bg-muted/50 p-4 text-sm sm:grid-cols-2">
          <div class="flex justify-between gap-2">
            <dt class="text-muted-foreground">{{ $t('work.maxDaily') }}</dt>
            <dd>{{ hours(work.summary.maxDailyMinutes) }}</dd>
          </div>
          <div class="flex justify-between gap-2">
            <dt class="text-muted-foreground">{{ $t('work.maxWeekly') }}</dt>
            <dd>{{ hours(work.summary.maxWeeklyMinutes) }}</dd>
          </div>
          <div class="flex justify-between gap-2">
            <dt class="text-muted-foreground">{{ $t('work.minRest') }}</dt>
            <dd>{{ hours(work.summary.minDailyRestMinutes) }}</dd>
          </div>
          <div class="flex justify-between gap-2">
            <dt class="text-muted-foreground">{{ $t('work.statutoryVacation') }}</dt>
            <dd>{{ $t('work.days', { n: work.summary.vacationDays }) }}</dd>
          </div>
          <div class="flex justify-between gap-2 sm:col-span-2">
            <dt class="text-muted-foreground">{{ $t('work.breaks') }}</dt>
            <dd class="text-right">
              <span v-for="([after, brk], i) in work.summary.breaks" :key="after">
                {{ i ? ' · ' : '' }}{{ $t('work.breakRule', { break: brk, after: format.number(after / 60, 1) }) }}
              </span>
              <span v-if="!work.summary.breaks.length">{{ $t('work.none') }}</span>
            </dd>
          </div>
        </dl>
      </CardContent>
    </Card>

    <p v-if="managing" class="rounded-lg border bg-muted/50 p-3 text-sm text-muted-foreground">
      {{ $t('work.managedUnderOrg') }}
      <RouterLink to="/organization" class="font-medium text-foreground underline">{{ $t('nav.organization') }}</RouterLink>
    </p>
    <!-- Everybody sees their own here; in an organization it is read-only (set under Organization → Work hours). -->
    <WorkRecords :read-only="workspace.inOrg" />
  </div>
</template>
