<script setup>
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { LoaderCircle, Pause, Play, SkipBack, SkipForward } from 'lucide-vue-next'
import { durationText } from '../platforms'

const props = defineProps({
  range: { type: Object, required: true },
  trackDuration: { type: Number, required: true },
  src: { type: String, default: '' },
  loading: Boolean,
  disabled: Boolean,
  canPlay: Boolean,
  active: { type: Boolean, default: true },
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
let previewEnd = null
let playbackFrame = 0
const duration = computed(() => decodedDuration.value || props.trackDuration)
const canSeek = computed(() => props.canPlay && duration.value > 0)
const buttonLabel = computed(() => {
  if (props.loading) return '正在准备预览'
  return playing.value ? '暂停预览' : '播放预览'
})

watch(() => props.src, () => {
  audio.value?.pause()
  stopTracking()
  previewEnd = null
  decodedDuration.value = 0
  playing.value = false
})
watch(() => props.range.start, start => {
  if (!props.src && !seeked) position.value = start
})
watch(() => props.active, active => {
  if (active) return
  releaseDrag()
  audio.value?.pause()
  stopTracking()
})
onMounted(() => {
  window.addEventListener('keydown', snapWithCtrl)
  window.addEventListener('keyup', releaseCtrl)
  window.addEventListener('blur', releaseDrag)
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', snapWithCtrl)
  window.removeEventListener('keyup', releaseCtrl)
  window.removeEventListener('blur', releaseDrag)
  releaseDrag()
  stopTracking()
  audio.value?.pause()
})

function percent(seconds) {
  return duration.value ? `${Math.min(seconds / duration.value, 1) * 100}%` : '0%'
}

const markerBounds = computed(() => {
  const limit = Math.max(duration.value, props.range.end)
  const gap = Math.min(0.001, props.range.end - props.range.start)
  return { start: [0, props.range.end - gap], end: [props.range.start + gap, limit] }
})

function setMarker(point, seconds) {
  const [min, max] = markerBounds.value[point]
  const time = Math.max(min, Math.min(max, seconds))
  if (!props.disabled && time !== props.range[point]) emit('update:range', { ...props.range, [point]: time })
}

function startDrag(point, event) {
  if (event.button !== 0 || drag || !duration.value) return
  const width = markersRail.value.clientWidth
  if (!width) return
  event.preventDefault()
  event.currentTarget.focus()
  const target = event.currentTarget
  target.setPointerCapture(event.pointerId)
  drag = { point, target, id: event.pointerId, x: event.clientX, time: props.range[point], secondsPerPixel: duration.value / width, snapped: false }
  previewEnd = null
  if (event.ctrlKey) snapDrag()
}

function moveDrag(event) {
  if (drag?.id !== event.pointerId) return
  if (!(event.buttons & 1)) {
    releaseDrag()
  } else if (event.ctrlKey) {
    snapDrag()
  } else if (drag.snapped) {
    releaseDrag()
  } else {
    const time = drag.time + (event.clientX - drag.x) * drag.secondsPerPixel
    setMarker(drag.point, Math.round(time * 1000) / 1000)
  }
}

function snapDrag() {
  if (!drag) return
  drag.snapped = true
  setMarker(drag.point, position.value)
}

function snapWithCtrl(event) {
  if (event.key === 'Control') snapDrag()
}

function releaseCtrl(event) {
  if (event.key === 'Control' && drag?.snapped) releaseDrag()
}

function releaseDrag() {
  if (!drag) return
  const { target, id } = drag
  drag = null
  if (target.hasPointerCapture(id)) target.releasePointerCapture(id)
}

function endDrag(event) {
  if (drag?.id === event.pointerId) releaseDrag()
}

function moveMarkerWithKey(point, event) {
  const directions = { ArrowLeft: -1, ArrowDown: -1, ArrowRight: 1, ArrowUp: 1 }
  const [min, max] = markerBounds.value[point]
  const offset = directions[event.key]
  if (!offset && event.key !== 'Home' && event.key !== 'End') return
  event.preventDefault()
  const time = offset ? props.range[point] + offset * (event.shiftKey ? 1 : 0.1) : event.key === 'Home' ? min : max
  setMarker(point, offset ? Math.round(time * 1000) / 1000 : time)
}

function playFrom(player, time) {
  const start = Math.min(time, duration.value)
  previewEnd = endForPosition(start)
  player.currentTime = start >= duration.value ? 0 : start
  void player.play().catch(() => {})
}

function endForPosition(time) {
  const end = Math.min(props.range.end, duration.value)
  return !drag && props.range.start <= time && time < end ? end : null
}

function toggle() {
  if (!props.src) {
    emit('load')
    return
  }
  if (audio.value.paused) {
    if (audio.value.readyState) {
      playFrom(audio.value, position.value)
    }
  } else {
    audio.value.pause()
  }
}

function seek(time) {
  seeked = true
  position.value = Math.min(time, duration.value)
  if (audio.value?.readyState) {
    if (!audio.value.paused) previewEnd = endForPosition(position.value)
    audio.value.currentTime = position.value
  }
}

function startPlayback(event) {
  const player = event.target
  if (Number.isFinite(player.duration) && player.duration > 0) decodedDuration.value = player.duration
  if (!props.active) {
    player.currentTime = Math.min(position.value, duration.value)
    return
  }
  playFrom(player, position.value)
}

function updatePosition() {
  const player = audio.value
  if (!player) return
  if (previewEnd !== null && player.currentTime >= previewEnd) {
    const end = previewEnd
    previewEnd = null
    player.pause()
    if (player.currentTime !== end) player.currentTime = end
    position.value = end
  } else if (!player.paused || Math.abs(player.currentTime - position.value) > 0.000001) {
    position.value = player.currentTime
  }
  if (drag?.snapped) snapDrag()
}

function trackPlayback() {
  updatePosition()
  if (audio.value && !audio.value.paused) playbackFrame = requestAnimationFrame(trackPlayback)
}

function startTracking() {
  playing.value = true
  cancelAnimationFrame(playbackFrame)
  playbackFrame = requestAnimationFrame(trackPlayback)
}

function stopTracking() {
  cancelAnimationFrame(playbackFrame)
  playing.value = false
}
</script>

<template>
  <div class="preview-player">
    <h3 class="preview-heading">试听与谱面预览区间</h3>
    <div class="preview-transport">
      <div class="preview-buttons">
        <button class="button secondary preview-control" type="button" :disabled="!canSeek"
          aria-label="转到入点" title="转到入点" @click="seek(range.start)">
          <SkipBack :size="18" />
        </button>
        <button class="button secondary preview-control" type="button"
          :disabled="loading || (!src && (disabled || !canPlay))" :aria-label="buttonLabel" :title="buttonLabel" @click="toggle">
          <LoaderCircle v-if="loading" class="spin" :size="18" />
          <Pause v-else-if="playing" :size="18" />
          <Play v-else :size="18" />
        </button>
        <button class="button secondary preview-control" type="button" :disabled="!canSeek"
          aria-label="转到出点" title="转到出点" @click="seek(range.end)">
          <SkipForward :size="18" />
        </button>
      </div>
      <div class="preview-timeline" :style="{ '--preview-start': percent(range.start), '--preview-end': percent(range.end) }">
        <div ref="markersRail" class="preview-markers">
          <button v-for="(label, point) in markers" :key="point" type="button" class="preview-marker"
            :class="`preview-marker-${point}`" :style="{ left: percent(range[point]) }" role="slider"
            :aria-label="label" :aria-valuemin="markerBounds[point][0]" :aria-valuemax="markerBounds[point][1]"
            :aria-valuenow="range[point]" :disabled="disabled" title="按住左键+Ctrl吸附"
            @pointerdown="startDrag(point, $event)" @pointermove="moveDrag"
            @pointerup="endDrag" @pointercancel="endDrag" @lostpointercapture="endDrag"
            @keydown="moveMarkerWithKey(point, $event)" />
        </div>
        <input class="preview-seek" type="range" min="0" :max="duration" step="any"
          :value="Math.min(position, duration)" :disabled="!canSeek"
          aria-label="全曲播放进度" @input="seek($event.target.valueAsNumber)" />
        <output class="preview-range-length" aria-label="预览片段时长" aria-live="off">
          <span>{{ seconds.format(range.end - range.start) }} 秒</span>
        </output>
      </div>
      <span class="preview-counter">{{ durationText(position) || '0:00' }} / {{ durationText(duration) || '0:00' }}</span>
    </div>
    <audio v-if="src" ref="audio" :src="src" preload="auto" hidden
      @loadedmetadata="startPlayback" @timeupdate="updatePosition"
      @play="startTracking" @pause="stopTracking" @ended="stopTracking" />
  </div>
</template>
