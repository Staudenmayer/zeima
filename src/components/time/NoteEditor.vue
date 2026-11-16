<script setup lang="ts">
import { nextTick, ref } from 'vue'
import { Button } from '@/components/ui/button'

// A note with a formatting toolbar: the buttons write Markdown around the selection.
defineProps<{ id: string; placeholder?: string; minHeight?: string; /** Stretch to the height the parent gives. */ fill?: boolean }>()
const model = defineModel<string>({ required: true })
const area = ref<HTMLTextAreaElement | null>(null)

const TOOLS = [
  { key: 'bold', label: 'B', cls: 'font-bold' },
  { key: 'italic', label: 'I', cls: 'italic' },
  { key: 'code', label: '</>', cls: 'font-mono text-xs' },
  { key: 'ul', label: '• List', cls: '' },
  { key: 'ol', label: '1. List', cls: '' },
  { key: 'quote', label: '“ Quote', cls: '' },
  { key: 'link', label: 'Link', cls: 'underline' },
] as const
type Tool = (typeof TOOLS)[number]['key']

async function format(kind: Tool) {
  const el = area.value
  const v = model.value ?? ''
  const a = el?.selectionStart ?? v.length
  const b = el?.selectionEnd ?? v.length
  let next = v
  let from = a
  let to = b
  const wrap = (m: string) => {
    next = v.slice(0, a) + m + v.slice(a, b) + m + v.slice(b)
    from = a + m.length
    to = b + m.length
  }
  const lines = (f: (line: string, i: number) => string) => {
    const start = v.lastIndexOf('\n', a - 1) + 1
    let end = v.indexOf('\n', b)
    if (end < 0) end = v.length
    const part = v.slice(start, end).split('\n').map(f).join('\n')
    next = v.slice(0, start) + part + v.slice(end)
    from = start
    to = start + part.length
  }
  if (kind === 'bold') wrap('**')
  else if (kind === 'italic') wrap('_')
  else if (kind === 'code') wrap('`')
  else if (kind === 'ul') lines((x) => `- ${x}`)
  else if (kind === 'ol') lines((x, i) => `${i + 1}. ${x}`)
  else if (kind === 'quote') lines((x) => `> ${x}`)
  else {
    const text = v.slice(a, b) || 'text'
    next = `${v.slice(0, a)}[${text}](https://)${v.slice(b)}`
    from = a
    to = a + text.length + 12
  }
  model.value = next
  await nextTick()
  el?.focus()
  el?.setSelectionRange(from, to)
}
</script>

<template>
  <div
    class="overflow-hidden rounded-md border border-input bg-background focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/50"
    :class="fill ? 'flex min-h-0 flex-1 flex-col' : ''"
  >
    <div role="toolbar" :aria-label="$t('note.toolbar')" class="flex flex-wrap items-center gap-0.5 border-b bg-muted/50 p-1">
      <Button v-for="tool in TOOLS" :key="tool.key" type="button" size="sm" variant="ghost" class="h-7 min-w-7 px-2" :class="tool.cls" :title="$t(`note.${tool.key}`)" :aria-label="$t(`note.${tool.key}`)" @click="format(tool.key)">{{ tool.label }}</Button>
    </div>
    <textarea
      :id="id"
      ref="area"
      v-model="model"
      :placeholder="placeholder"
      class="block w-full resize-none bg-transparent px-3 py-2 text-sm outline-none placeholder:text-muted-foreground"
      :class="fill ? 'min-h-0 flex-1' : ''"
      :style="{ minHeight: minHeight ?? '8.5rem' }"
    />
  </div>
</template>
