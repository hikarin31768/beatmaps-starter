<script setup>
import { computed, onBeforeUnmount, onMounted, ref, useId } from 'vue'

const props = defineProps({
  update: { type: Object, required: true },
  download: { type: Object, default: null },
  installing: Boolean,
  error: { type: String, default: '' },
})
const emit = defineEmits(['install', 'dismiss'])
const dialog = ref(null)
const titleId = useId()
const bytes = new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 1 })

function noteText(value) {
  return value.replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
    .replace(/\*\*([^*]+)\*\*/g, '$1')
    .replace(/`([^`]+)`/g, '$1')
}

const noteBlocks = computed(() => {
  const blocks = []
  for (const raw of (props.update.notes || '').split(/\r?\n/)) {
    const line = raw.trim()
    if (!line) continue
    const heading = line.match(/^#{1,6}\s+(.+)$/)
    const bullet = line.match(/^[-*+]\s+(.+)$/)
    if (heading) {
      blocks.push({ kind: 'heading', text: noteText(heading[1]) })
    } else if (bullet) {
      const last = blocks.at(-1)
      const text = noteText(bullet[1])
      if (last?.kind === 'list') last.items.push(text)
      else blocks.push({ kind: 'list', items: [text] })
    } else {
      blocks.push({ kind: 'paragraph', text: noteText(line) })
    }
  }
  return blocks
})

const percentage = computed(() => {
  if (props.download?.phase === 'installing') return 100
  const { received, total } = props.download ?? {}
  return total > 0 ? Math.min(100, Math.floor(received / total * 100)) : null
})
const transferSize = computed(() => {
  if (props.download?.phase !== 'downloading' || !props.download.received) return ''
  const { received, total } = props.download
  const format = value => `${bytes.format(value / 1_000_000)} MB`
  return total > 0 ? `${format(received)} / ${format(total)}` : format(received)
})

function dismiss() {
  if (!props.installing) emit('dismiss')
}

onMounted(() => dialog.value.showModal())
onBeforeUnmount(() => {
  if (dialog.value?.open) dialog.value.close()
})
</script>

<template>
  <Teleport to="body">
    <dialog ref="dialog" class="update-dialog" :aria-labelledby="titleId" @cancel.prevent="dismiss">
      <div class="update-dialog-content">
        <h2 :id="titleId">发现新版本 {{ update.version }}</h2>
        <section class="update-notes" aria-label="更新日志" tabindex="0">
          <h3>更新日志</h3>
          <template v-for="(block, index) in noteBlocks" :key="index">
            <h4 v-if="block.kind === 'heading'">{{ block.text }}</h4>
            <ul v-else-if="block.kind === 'list'">
              <li v-for="(item, itemIndex) in block.items" :key="itemIndex">{{ item }}</li>
            </ul>
            <p v-else>{{ block.text }}</p>
          </template>
          <p v-if="!noteBlocks.length" class="muted">此版本暂无更新日志。</p>
        </section>
        <div v-if="download" class="update-progress">
          <div class="update-progress-label">
            <span role="status">{{ download.phase === 'installing' ? '正在验证并安装更新' : '正在下载更新' }}</span>
            <span v-if="transferSize" class="muted">{{ transferSize }}</span>
            <strong v-if="percentage != null">{{ percentage }}%</strong>
          </div>
          <progress v-if="percentage != null" :value="percentage" max="100" aria-label="安装包下载进度"></progress>
          <progress v-else aria-label="安装包下载进度"></progress>
        </div>
        <p v-if="error" class="inline-error" role="alert">{{ error }}</p>
        <div class="update-dialog-actions">
          <button class="button secondary" type="button" :disabled="installing" @click="dismiss">稍后</button>
          <button class="button primary" type="button" :disabled="installing" @click="emit('install')">
            {{ error ? '重试下载' : '下载并安装' }}
          </button>
        </div>
      </div>
    </dialog>
  </Teleport>
</template>
