<script setup>
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { LoaderCircle, Pause, Play } from 'lucide-vue-next'
import { durationText } from '../platforms'

const props = defineProps({
  range: { type: Object, required: true },
  trackDuration: { type: Number, required: true },
  src: { type: String, default: '' },
  loading: Boolean,
  disabled: Boolean,
  canPlay: Boolean,
})
const emit = defineEmits(['load', 'update:range'])
const markers = { start: '预览入点', end: '预览出点' }
const seconds = new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 3 })
const markersRail = ref(null)
const audio = ref(null)
const playing = ref(false)
const position = ref(props.range.start)
const decodedDuration = ref(0)
let drag = null
let seeked = false
const duration = computed(() => decodedDuration.value || props.trackDuration)
const buttonLabel = computed(() => {
  if (props.loading) return '正在准备预览'
  return playing.value ? '暂停预览' : '播放预览'
})

watch(() => props.src, () => {
  audio.value?.pause()
  decodedDuration.value = 0
  playing.value = false
})
watch(() => props.range.start, start => {
  if (!props.src && !seeked) position.value = start
})
onBeforeUnmount(() => audio.value?.pause())

function percent(seconds) {
  return `${Math.min(seconds / duration.value, 1) * 100}%`
}

const markerBounds = computed(() => {
  const limit = Math.max(duration.value, props.range.end)
  const gap = Math.min(0.001, props.range.end - props.range.start)
  return { start: [0, props.range.end - gap], end: [props.range.start + gap, limit] }
})

function setMarker(point, seconds) {
  const [min, max] = markerBounds.value[point]
  const time = Math.max(min, Math.min(max, Math.round(seconds * 1000) / 1000))
  if (!props.disabled && time !== props.range[point]) emit('update:range', { ...props.range, [point]: time })
}

function startDrag(point, event) {
  if (event.button !== 0 || drag) return
  const width = markersRail.value.clientWidth
  if (!width) return
  event.preventDefault()
  event.currentTarget.focus()
  event.currentTarget.setPointerCapture(event.pointerId)
  drag = { point, id: event.pointerId, x: event.clientX, time: props.range[point], secondsPerPixel: duration.value / width }
}

function moveDrag(event) {
  if (drag?.id === event.pointerId) setMarker(drag.point, drag.time + (event.clientX - drag.x) * drag.secondsPerPixel)
}

function endDrag(event) {
  if (drag?.id === event.pointerId) drag = null
}

function moveMarkerWithKey(point, event) {
  const directions = { ArrowLeft: -1, ArrowDown: -1, ArrowRight: 1, ArrowUp: 1 }
  const [min, max] = markerBounds.value[point]
  const offset = directions[event.key]
  if (!offset && event.key !== 'Home' && event.key !== 'End') return
  event.preventDefault()
  const time = offset ? props.range[point] + offset * (event.shiftKey ? 1 : 0.1) : event.key === 'Home' ? min : max
  setMarker(point, time)
}

function toggle() {
  if (!props.src) {
    emit('load')
    return
  }
  if (audio.value.paused) {
    if (audio.value.ended) audio.value.currentTime = Math.min(props.range.start, duration.value)
    void audio.value.play().catch(() => {})
  } else {
    audio.value.pause()
  }
}

function seek(event) {
  seeked = true
  position.value = Number(event.target.value)
  if (audio.value?.readyState) audio.value.currentTime = position.value
}

function startPlayback(event) {
  const player = event.target
  if (Number.isFinite(player.duration) && player.duration > 0) decodedDuration.value = player.duration
  player.currentTime = Math.min(position.value, duration.value)
  void player.play().catch(() => {})
}
</script>

<template>
  <div class="preview-player">
    <h3 class="preview-heading">试听与谱面预览区间</h3>
    <div class="preview-transport">
      <button class="button secondary preview-toggle" type="button"
        :disabled="loading || (!src && (disabled || !canPlay))" :aria-label="buttonLabel" :title="buttonLabel" @click="toggle">
        <LoaderCircle v-if="loading" class="spin" :size="18" />
        <Pause v-else-if="playing" :size="18" />
        <Play v-else :size="18" />
      </button>
      <div class="preview-timeline" :style="{ '--preview-start': percent(range.start), '--preview-end': percent(range.end) }">
        <div ref="markersRail" class="preview-markers">
          <button v-for="(label, point) in markers" :key="point" type="button" class="preview-marker"
            :class="`preview-marker-${point}`" :style="{ left: percent(range[point]) }" role="slider"
            :aria-label="label" :aria-valuemin="markerBounds[point][0]" :aria-valuemax="markerBounds[point][1]"
            :aria-valuenow="range[point]" :disabled="disabled"
            @pointerdown="startDrag(point, $event)" @pointermove="moveDrag" @lostpointercapture="endDrag"
            @keydown="moveMarkerWithKey(point, $event)" />
        </div>
        <input class="preview-seek" type="range" min="0" :max="duration" step="0.01"
          :value="Math.min(position, duration)" :disabled="!canPlay || !duration"
          aria-label="全曲播放进度" @input="seek" />
        <output class="preview-range-length" aria-label="预览片段时长" aria-live="off">
          <span>{{ seconds.format(range.end - range.start) }} 秒</span>
        </output>
      </div>
      <span class="preview-counter">{{ durationText(position) || '0:00' }} / {{ durationText(duration) || '0:00' }}</span>
    </div>
    <audio v-if="src" ref="audio" :src="src" preload="auto" hidden
      @loadedmetadata="startPlayback" @timeupdate="position = $event.target.currentTime"
      @play="playing = true" @pause="playing = false" @ended="playing = false" />
  </div>
</template>
