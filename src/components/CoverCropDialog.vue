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
const headingId = useId()
const crop = ref({ ...props.image.region })
const short = Math.min(props.image.width, props.image.height)
const imageWidth = props.image.width / short
const imageHeight = props.image.height / short
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

function resizeCrop(event) {
  const size = event.target.valueAsNumber
  const offset = (crop.value.size - size) / 2
  placeCrop(crop.value.x + offset, crop.value.y + offset, size)
}

function startDrag(event) {
  if (props.saving || event.button !== 0 || drag) return
  const stage = event.currentTarget
  const bounds = stage.getBoundingClientRect()
  if (!bounds.width || !bounds.height) return
  const scaleX = imageWidth / bounds.width
  const scaleY = imageHeight / bounds.height
  const x = (event.clientX - bounds.left) * scaleX
  const y = (event.clientY - bounds.top) * scaleY
  const current = crop.value
  const inside = x >= current.x && x <= current.x + current.size
    && y >= current.y && y <= current.y + current.size
  if (!inside) placeCrop(x - current.size / 2, y - current.size / 2)
  drag = {
    id: event.pointerId, x: event.clientX, y: event.clientY,
    cropX: crop.value.x, cropY: crop.value.y, scaleX, scaleY,
  }
  stage.focus()
  stage.setPointerCapture(event.pointerId)
}

function moveDrag(event) {
  if (drag?.id !== event.pointerId || props.saving) return
  placeCrop(drag.cropX + (event.clientX - drag.x) * drag.scaleX,
    drag.cropY + (event.clientY - drag.y) * drag.scaleY)
}

function endDrag(event) {
  if (drag?.id === event.pointerId) drag = null
}

function moveWithKey(event) {
  const directions = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }
  const direction = directions[event.key]
  if (props.saving || !direction) return
  event.preventDefault()
  const step = event.shiftKey ? 0.05 : 0.01
  placeCrop(crop.value.x + direction[0] * step, crop.value.y + direction[1] * step)
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
        <div class="cover-crop-stage" :style="stageStyle" role="group" tabindex="0" aria-label="拖动或使用方向键移动裁切框"
          @pointerdown="startDrag" @pointermove="moveDrag" @pointerup="endDrag"
          @pointercancel="endDrag" @lostpointercapture="endDrag" @keydown="moveWithKey">
          <img :src="image.src" alt="待裁切的封面原图" draggable="false" />
          <div class="cover-crop-frame" :style="frameStyle" aria-hidden="true"></div>
        </div>
        <label class="cover-crop-size">
          <span>裁切范围</span>
          <input type="range" :min="1 / short" max="1" step="any" :value="crop.size" :disabled="saving || short === 1"
            @input="resizeCrop" />
          <span aria-label="固定比例">1:1</span>
        </label>
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
