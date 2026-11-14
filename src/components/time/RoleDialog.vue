<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { errorMessage } from '@/lib/ipc'
import { type PermissionDef, type Permissions, type Role, type Scope, useOrgStore } from '@/stores/org'

// A role is a set of permissions: for each thing an organization has (time
// entries, projects, people, …) whether the role may view, edit and delete it.
// Some views reach as far as the person's own, their departments' or
// everyone's. Only what the person holds themselves can be given.
const props = defineProps<{ role: Role | null }>()
const open = defineModel<boolean>('open', { required: true })

const org = useOrgStore()
const RANK: Record<Scope, number> = { own: 0, group: 1, org: 2 }
const KINDS = ['view', 'edit', 'delete'] as const

const name = ref('')
const description = ref('')
const draft = ref<Permissions>({})
const saving = ref(false)

watch(open, (isOpen) => {
  if (!isOpen) return
  name.value = props.role?.name ?? ''
  description.value = props.role?.description ?? ''
  draft.value = { ...(props.role?.permissions ?? {}) }
})

/** Nothing can change the owner role, nor the role the person has themselves. */
const readOnly = computed(() => !!props.role && (props.role.locked || props.role.key === org.role))
const nameLocked = computed(() => !!props.role?.builtin)

const resources = computed(() => [...new Set(org.catalog.map((d) => d.resource))])
const defsOf = (resource: string, kind: (typeof KINDS)[number]) => org.catalog.filter((d) => d.resource === resource && d.kind === kind)

/** The scopes the person may give for an action: up to what they hold. */
function givable(d: PermissionDef): Scope[] {
  const mine = org.permissions[d.action]
  return mine === undefined ? [] : d.scopes.filter((s) => RANK[s] <= RANK[mine])
}
const set = (action: string, scope: Scope | null) => {
  const next = { ...draft.value }
  if (scope === null) delete next[action]
  else next[action] = scope
  draft.value = next
}

async function save() {
  saving.value = true
  try {
    if (props.role) await org.updateRole(props.role.key, { ...(nameLocked.value ? {} : { name: name.value.trim() }), description: description.value.trim(), permissions: draft.value })
    else await org.createRole({ name: name.value.trim(), description: description.value.trim(), permissions: draft.value })
    open.value = false
  } catch (err) {
    toast.error(errorMessage(err))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent :aria-describedby="undefined" class="max-h-[90vh] overflow-y-auto sm:max-w-3xl">
      <DialogHeader>
        <DialogTitle>{{ role ? $t('org.roleEdit') : $t('org.roleNew') }}</DialogTitle>
        <DialogDescription>{{ $t('org.roleHint') }}</DialogDescription>
      </DialogHeader>
      <form class="grid gap-4" @submit.prevent="save">
        <div class="grid gap-3 sm:grid-cols-2">
          <div class="grid gap-2">
            <Label for="role-name">{{ $t('org.jobs.name') }}</Label>
            <Input id="role-name" :model-value="nameLocked && role ? org.roleName(role.key) : name" required maxlength="60" :disabled="nameLocked || readOnly" @update:model-value="name = String($event)" />
          </div>
          <div class="grid gap-2">
            <Label for="role-desc">{{ $t('org.jobs.description') }} <span class="font-normal text-muted-foreground">{{ $t('common.optional') }}</span></Label>
            <Input id="role-desc" v-model="description" maxlength="300" :disabled="readOnly" />
          </div>
        </div>
        <p v-if="readOnly" class="rounded-md border p-2 text-sm text-muted-foreground">{{ role?.locked ? $t('org.roleLocked') : $t('org.roleOwn') }}</p>

        <div class="overflow-x-auto rounded-lg border">
          <table class="w-full text-sm">
            <thead class="text-left text-xs text-muted-foreground">
              <tr class="border-b">
                <th class="px-3 py-2 font-medium">{{ $t('org.permWhat') }}</th>
                <th v-for="k in KINDS" :key="k" class="px-3 py-2 font-medium">{{ $t(`org.permKind.${k}`) }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="r in resources" :key="r" class="border-b align-top last:border-b-0">
                <td class="whitespace-nowrap px-3 py-2 font-medium">{{ $t(`org.permResource.${r}`) }}</td>
                <td v-for="k in KINDS" :key="k" class="px-3 py-2">
                  <div class="grid gap-1.5">
                    <template v-for="d in defsOf(r, k)" :key="d.action">
                      <label v-if="d.scopes.length === 1" class="flex items-start gap-2" :class="givable(d).length === 0 ? 'opacity-50' : ''">
                        <Checkbox
                          class="mt-0.5"
                          :model-value="draft[d.action] !== undefined"
                          :disabled="readOnly || givable(d).length === 0"
                          @update:model-value="(on) => set(d.action, on === true ? 'org' : null)"
                        />
                        <span>{{ $t(`org.perm.${d.action}`) }}</span>
                      </label>
                      <div v-else class="grid gap-1" :class="givable(d).length === 0 ? 'opacity-50' : ''">
                        <span>{{ $t(`org.perm.${d.action}`) }}</span>
                        <Select :model-value="draft[d.action] ?? 'none'" :disabled="readOnly || givable(d).length === 0" @update:model-value="(v) => set(d.action, v === 'none' ? null : (v as Scope))">
                          <SelectTrigger class="h-8 w-44"><SelectValue /></SelectTrigger>
                          <SelectContent>
                            <SelectItem value="none">{{ $t('org.scope.none') }}</SelectItem>
                            <SelectItem v-for="s in givable(d)" :key="s" :value="s">{{ $t(`org.scope.${s}`) }}</SelectItem>
                          </SelectContent>
                        </Select>
                      </div>
                    </template>
                    <span v-if="defsOf(r, k).length === 0" class="text-muted-foreground">–</span>
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        </div>

        <DialogFooter>
          <Button type="button" variant="outline" @click="open = false">{{ $t('common.cancel') }}</Button>
          <Button type="submit" :disabled="readOnly || saving">{{ $t('common.save') }}</Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
