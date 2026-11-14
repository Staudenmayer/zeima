<script setup lang="ts">
import { computed } from 'vue'
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { useOrgStore } from '@/stores/org'

// A person's profile picture, or their initial on a colour of their own when
// they have none. Give the `userId` (the picture comes from the organization's
// people), or `src` for one already at hand.
const props = withDefaults(defineProps<{ userId?: string | null; name?: string | null; src?: string | null; size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl' }>(), { size: 'sm' })
const org = useOrgStore()

const picture = computed(() => props.src ?? (props.userId ? org.avatarOf(props.userId) : null))
const label = computed(() => props.name || (props.userId ? org.nameOf(props.userId) : '') || '?')
const initial = computed(() => label.value.trim().charAt(0).toUpperCase() || '?')
const SIZES = { xs: 'size-4 text-[9px]', sm: 'size-6 text-[11px]', md: 'size-8 text-sm', lg: 'size-12 text-lg', xl: 'size-24 text-3xl' }
/** The same person always gets the same colour. */
const hue = computed(() => {
  let h = 0
  for (const c of props.userId || label.value) h = (h * 31 + c.charCodeAt(0)) % 360
  return h
})
</script>

<template>
  <Avatar :class="SIZES[size]" aria-hidden="true">
    <AvatarImage v-if="picture" :src="picture" :alt="label" class="object-cover" />
    <AvatarFallback class="font-medium text-white" :style="{ backgroundColor: `hsl(${hue} 45% 45%)` }">{{ initial }}</AvatarFallback>
  </Avatar>
</template>
