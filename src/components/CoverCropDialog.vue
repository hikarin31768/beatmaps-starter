<script setup>
import { computed, onBeforeUnmount, onMounted, ref, useId } from 'vue'
import { LoaderCircle } from 'lucide-vue-next'

const props = defineProps({
  image: { type: Object, required: true },
  saving: Boolean,
  error: { type: String, default: '' },
})
const emit = defineEmits(['apply', 'cancel'])
const dialog = ref(null)
const stage = ref(null)
const headingId = useId()
const crop = ref({ ...props.image.region })
const short = Math.min(props.image.width, props.image.height)
const imageWidth = props.image.width / short
const imageHeight = props.image.height / short
const minSize = 1 / short
const maxSize = computed(() => maximumSize(crop.value))
const directions = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }
let drag = null

const stageStyle = {
  aspectRatio: `${props.image.width} / ${props.image.height}`,
  width: `min(100%, calc(min(52vh, 360px) * ${imageWidth / imageHeight}))`,
}
const frameStyle = computed(() => ({
  left: `${crop.value.x / imageWidth * 100}%`,
  top: `${crop.value.y / imageHeight * 100}%`,
  width: `${crop.value.size / imageWidth * 100}%`,
  height: `${crop.value.size / imageHeight * 100}%`,
}))

onMounted(() => dialog.value.showModal())
onBeforeUnmount(() => {
  if (dialog.value?.open) dialog.value.close()
})

function placeCrop(x, y, size = crop.value.size) {
  crop.value = {
    x: Math.max(0, Math.min(imageWidth - size, x)),
    y: Math.max(0, Math.min(imageHeight - size, y)),
    size,
  }
}

function maximumSize(frame, fromCenter = false) {
  const { x, y, size } = frame
  if (!fromCenter) return Math.min(1, imageWidth - x, imageHeight - y)
  const margin = Math.min(x, y, imageWidth - x - size, imageHeight - y - size)
  return Math.min(1, size + 2 * Math.max(0, margin))
}

function resizeCrop(size, frame = crop.value, fromCenter = false) {
  const nextSize = Math.max(minSize, Math.min(maximumSize(frame, fromCenter), size))
  const offset = fromCenter ? (frame.size - nextSize) / 2 : 0
  placeCrop(frame.x + offset, frame.y + offset, nextSize)
}

function startDrag(event, mode = 'move') {
  if (props.saving || event.button !== 0 || drag) return
  const bounds = stage.value.getBoundingClientRect()
  if (!bounds.width || !bounds.height) return
  event.preventDefault()
  const scaleX = imageWidth / bounds.width
  const scaleY = imageHeight / bounds.height
  if (mode === 'move') {
    const x = (event.clientX - bounds.left) * scaleX
    const y = (event.clientY - bounds.top) * scaleY
    const current = crop.value
    const inside = x >= current.x && x <= current.x + current.size
      && y >= current.y && y <= current.y + current.size
    if (!inside) placeCrop(x - current.size / 2, y - current.size / 2)
  }
  drag = {
    mode, id: event.pointerId, x: event.clientX, y: event.clientY,
    lastX: event.clientX, lastY: event.clientY, frame: crop.value, fromCenter: event.altKey, scaleX, scaleY,
  }
  const target = event.currentTarget
  target.focus({ preventScroll: true })
  target.setPointerCapture(event.pointerId)
}

function moveDrag(event) {
  if (drag?.id !== event.pointerId || props.saving) return
  if (drag.mode === 'resize' && drag.fromCenter !== event.altKey) {
    drag.frame = crop.value
    drag.x = drag.lastX
    drag.y = drag.lastY
    drag.fromCenter = event.altKey
  }
  const dx = (event.clientX - drag.x) * drag.scaleX
  const dy = (event.clientY - drag.y) * drag.scaleY
  if (drag.mode === 'resize') {
    const delta = (dx + dy) * (drag.fromCenter ? 1 : 0.5)
    resizeCrop(drag.frame.size + delta, drag.frame, drag.fromCenter)
  } else {
    placeCrop(drag.frame.x + dx, drag.frame.y + dy)
  }
  drag.lastX = event.clientX
  drag.lastY = event.clientY
}

function endDrag(event) {
  if (drag?.id === event.pointerId) drag = null
}

function moveWithKey(event) {
  const direction = directions[event.key]
  if (props.saving || !direction) return
  event.preventDefault()
  const step = event.shiftKey ? 0.05 : 0.01
  placeCrop(crop.value.x + direction[0] * step, crop.value.y + direction[1] * step)
}

function resizeWithKey(event) {
  const direction = directions[event.key]
  if (props.saving || (!direction && event.key !== 'Home' && event.key !== 'End')) return
  event.preventDefault()
  if (event.key === 'Home') resizeCrop(minSize)
  else if (event.key === 'End') resizeCrop(maxSize.value)
  else {
    const step = event.shiftKey ? 0.05 : 0.01
    resizeCrop(crop.value.size + (direction[0] - direction[1]) * step)
  }
}

function cancel() {
  if (!props.saving) emit('cancel')
}
</script>

<template>
  <Teleport to="body">
    <dialog ref="dialog" class="cover-crop-dialog" :aria-labelledby="headingId" @cancel.prevent="cancel">
      <form @submit.prevent="!saving && emit('apply', crop)">
        <h2 :id="headingId">裁切封面</h2>
        <div ref="stage" class="cover-crop-stage" :style="stageStyle" role="group" tabindex="0" aria-label="拖动或使用方向键移动裁切框"
          @pointerdown="startDrag" @pointermove="moveDrag" @pointerup="endDrag"
          @pointercancel="endDrag" @lostpointercapture="endDrag" @keydown="moveWithKey"
          @selectstart.prevent @dragstart.prevent>
          <img :src="image.src" alt="待裁切的封面原图" draggable="false" />
          <div class="cover-crop-frame" :style="frameStyle">
            <button type="button" class="cover-crop-handle" role="slider"
              aria-label="调整裁切框大小" title="拖动缩放，按住 Alt 围绕中心缩放"
              :aria-valuemin="1" :aria-valuemax="Math.round(maxSize * short)" :aria-valuenow="Math.round(crop.size * short)"
              :disabled="saving || short === 1" @pointerdown.stop="startDrag($event, 'resize')" @keydown.stop="resizeWithKey"></button>
          </div>
        </div>
        <p v-if="error" class="inline-error" role="alert">{{ error }}</p>
        <div class="cover-crop-actions">
          <button class="button secondary" type="button" :disabled="saving" @click="cancel">取消</button>
          <button class="button primary" type="submit" :disabled="saving">
            <LoaderCircle v-if="saving" :size="16" class="spin" />应用
          </button>
        </div>
      </form>
    </dialog>
  </Teleport>
</template>
