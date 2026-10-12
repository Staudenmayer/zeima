<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { toast } from 'vue-sonner'
import { commands, type AutoPauseConfig, type AwayAction } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import TimeInput from '@/components/time/TimeInput.vue'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import PageHeader from '@/components/time/PageHeader.vue'
import SettingsNav from '@/components/time/SettingsNav.vue'
import { call, errorMessage } from '@/lib/ipc'
import { useWorkStore } from '@/stores/work'

definePage({ meta: { titleKey: 'nav.settings' } })

const { t } = useI18n()
const work = useWorkStore()

const form = ref<Required<AutoPauseConfig> | null>(null)
watch(
  () => work.settings?.autoPause,
  (cfg) => (form.value = cfg ? ({ ...cfg } as Required<AutoPauseConfig>) : null),
  { immediate: true },
)
const dirty = computed(() => JSON.stringify(form.value) !== JSON.stringify(work.settings?.autoPause))

const actions: AwayAction[] = ['prompt', 'exclude', 'keep']
const lockActions: AwayAction[] = ['exclude', 'prompt', 'keep']

const nightEnabled = computed({
  get: () => form.value?.nightStop != null,
  set: (on: boolean) => form.value && (form.value.nightStop = on ? '22:00' : null),
})
const nightTime = computed({
  get: () => form.value?.nightStop ?? '22:00',
  set: (v: string | number) => form.value && (form.value.nightStop = String(v)),
})

const source = ref('')
onMounted(async () => (source.value = await commands.activitySource()))

async function save() {
  if (!form.value || !work.settings) return
  try {
    await call(commands.workSettingsSet({ ...work.settings, autoPause: form.value }))
    toast.success(t('common.saved'))
  } catch (err) {
    toast.error(errorMessage(err))
  }
}
</script>

<template>
  <PageHeader :title="$t('nav.settings')">
    <Button :disabled="!dirty" @click="save">{{ $t('common.save') }}</Button>
  </PageHeader>
  <SettingsNav />

  <div v-if="form" class="grid max-w-3xl gap-6">
    <Card>
      <CardHeader>
        <div class="flex items-start justify-between gap-4">
          <div>
            <CardTitle>{{ $t('autopause.enabled') }}</CardTitle>
            <CardDescription>{{ $t('autopause.enabledHint') }}</CardDescription>
          </div>
          <Switch v-model="form.enabled" />
        </div>
      </CardHeader>
      <CardContent v-if="source" class="text-xs text-muted-foreground">
        {{ $t('autopause.source', { source }) }}
      </CardContent>
    </Card>

    <fieldset :disabled="!form.enabled" class="grid gap-6 disabled:opacity-60">
      <Card>
        <CardHeader>
          <CardTitle>{{ $t('autopause.idle') }}</CardTitle>
        </CardHeader>
        <CardContent class="grid gap-4">
          <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
            <Label for="idle-minutes">{{ $t('autopause.idleMinutes') }}</Label>
            <Input id="idle-minutes" v-model.number="form.idleMinutes" type="number" min="1" max="240" />
          </div>
          <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
            <Label>{{ $t('autopause.idleAction') }}</Label>
            <Select v-model="form.idleAction">
              <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem v-for="a in actions" :key="a" :value="a">{{ $t(`autopause.actions.${a}`) }}</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{{ $t('autopause.lock') }}</CardTitle>
        </CardHeader>
        <CardContent class="grid gap-4">
          <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
            <Label>{{ $t('autopause.lockAction') }}</Label>
            <Select v-model="form.lockAction">
              <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem v-for="a in lockActions" :key="a" :value="a">{{ $t(`autopause.actions.${a}`) }}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
            <Label for="min-away">{{ $t('autopause.minAway') }}</Label>
            <Input id="min-away" v-model.number="form.minAwayMinutes" type="number" min="0" max="120" />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div class="flex items-start justify-between gap-4">
            <div>
              <CardTitle>{{ $t('autopause.lunch') }}</CardTitle>
              <CardDescription>{{ $t('autopause.lunchHint') }}</CardDescription>
            </div>
            <Switch v-model="form.lunchDetection" />
          </div>
        </CardHeader>
        <CardContent v-if="form.lunchDetection" class="grid gap-4">
          <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
            <Label>{{ $t('autopause.lunchWindow') }}</Label>
            <div class="flex items-center gap-2">
              <TimeInput v-model="form.lunchStart" />
              <span>–</span>
              <TimeInput v-model="form.lunchEnd" />
            </div>
          </div>
          <div class="grid gap-2 sm:grid-cols-2 sm:items-center">
            <Label for="lunch-min">{{ $t('autopause.lunchMin') }}</Label>
            <Input id="lunch-min" v-model.number="form.lunchMinMinutes" type="number" min="1" max="180" />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div class="flex items-start justify-between gap-4">
            <div>
              <CardTitle>{{ $t('autopause.night') }}</CardTitle>
              <CardDescription>{{ $t('autopause.nightHint') }}</CardDescription>
            </div>
            <Switch v-model="nightEnabled" />
          </div>
        </CardHeader>
        <CardContent v-if="nightEnabled" class="grid gap-2 sm:grid-cols-2 sm:items-center">
          <Label>{{ $t('autopause.nightTime') }}</Label>
          <TimeInput v-model="nightTime" />
        </CardContent>
      </Card>

      <Card>
        <CardContent class="grid gap-4 pt-6">
          <label class="flex items-center justify-between gap-4 text-sm">
            {{ $t('autopause.warnDailyMax') }}
            <Switch v-model="form.warnDailyMax" />
          </label>
          <div class="flex items-center justify-between gap-4 text-sm">
            <div>
              <div>{{ $t('autopause.morning') }}</div>
              <p class="text-xs text-muted-foreground">{{ $t('autopause.morningHint') }}</p>
            </div>
            <Switch v-model="form.morningPrompt" />
          </div>
        </CardContent>
      </Card>
    </fieldset>
  </div>
</template>
