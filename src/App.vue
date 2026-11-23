<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { Channel, convertFileSrc, invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { open } from '@tauri-apps/plugin-dialog'
import {
  ArrowDownToLine, ArrowLeft, ArrowRight, AudioLines, Check, ChevronDown, ChevronRight, ChevronUp, Crop, FileImage, Folder,
  ExternalLink, Eye, FolderOpen, History, House, Link2, LoaderCircle, Minus, Pencil, Settings2, Square, Trash2, UserRound, X,
} from 'lucide-vue-next'
import LoginPage from './components/LoginPage.vue'
import CoverCropDialog from './components/CoverCropDialog.vue'
import OffsetDefaults from './components/OffsetDefaults.vue'
import PlatformIcon from './components/PlatformIcon.vue'
import PreviewPlayer from './components/PreviewPlayer.vue'
import UpdateDialog from './components/UpdateDialog.vue'
import { durationText, pickQuality, platforms } from './platforms'

const navigation = [
  { id: 'home', name: '首页', icon: House },
  { id: 'history', name: '历史', icon: History },
  { id: 'accounts', name: '账号', icon: UserRound },
  { id: 'settings', name: '设置', icon: Settings2 },
]
const steps = ['游戏目录', '流媒体账号', '谱面偏好']
const settings = reactive({
  initialized: false, gameDir: '', theme: 'dark', version: 'legacy', mapper: '', leadIn: 2,
  leadInUnit: 'seconds', tailOffset: 2, tailOffsetUnit: 'seconds',
  ...Object.fromEntries(platforms.map(({ qualityKey, qualities }) => [qualityKey, qualities[0][0]])),
})
const changes = reactive({})
const accounts = reactive({})
const ready = ref(false)
const startupError = ref('')
const accountsError = ref('')
const page = ref('home')
const settingsFromResult = ref(false)
const step = ref(0)
const gameDir = ref('')
const scanning = ref(false)
const login = ref(null)
const sourceUrl = ref('')
const media = ref(null)
const map = reactive({})
const draftUrl = ref('')
const historyId = ref('')
const customCover = ref(false)
const coverCrop = ref(null)
const historyEntries = ref([])
const loadingHistory = ref(false)
const historyError = ref('')
const historySaveError = ref('')
const selectingHistory = ref(false)
const selectedHistoryIds = ref([])
const editing = ref(false)
const preview = ref('')
const previewSource = ref(null)
const decodedSourceDuration = ref(0)
const analysis = ref(null)
const analyzing = ref(false)
const analysisError = ref('')
const alignment = ref('')
const fixedLeadOverride = ref(null)
const editingFixedLead = ref(false)
const fixedLeadInput = ref('')
const fixedLeadField = ref(null)
const fixedLeadButton = ref(null)
const exportPath = ref('')
const availableUpdate = ref(null)
const updateStatus = ref('')
const updateError = ref('')
const updateDownload = ref(null)
const busy = ref('')
const progress = ref('')
const error = ref('')
const notification = ref(null)
const scroll = ref(null)
const titleField = ref(null)
const desktop = '__TAURI_INTERNALS__' in window ? getCurrentWindow() : null
const osDark = window.matchMedia('(prefers-color-scheme: dark)')
const systemDark = ref(osDark.matches)
const effectiveTheme = computed(() => settings.theme === 'system' ? (systemDark.value ? 'dark' : 'light') : settings.theme)
const initialized = computed(() => settings.initialized)
const resultVisible = computed(() => page.value === 'home' && !login.value && !!media.value)
const sourcePlatform = computed(() => platforms.find(({ id }) => id === media.value?.platform))
const hasBpm = computed(() => Number.isFinite(map.bpm) && map.bpm >= 1 && map.bpm <= 1000)
const canAlign = computed(() => analysis.value?.firstBeat != null && hasBpm.value)
const validTail = computed(() => Number.isFinite(map.tailOffset) && map.tailOffset >= 0)
const tailSeconds = computed(() => validTail.value ? offsetInSeconds(map.tailOffset, map.tailOffsetUnit) : 0)
const tailSecondsInput = computed({
  get: () => {
    if (map.tailOffset == null) return ''
    if (map.tailOffsetUnit === 'seconds') return map.tailOffset
    return hasBpm.value ? Number(tailSeconds.value.toFixed(3)) : ''
  },
  set: value => {
    map.tailOffset = value === '' ? null : value
    map.tailOffsetUnit = 'seconds'
  },
})
const trackDuration = computed(() => {
  if (!media.value) return 0
  const source = decodedSourceDuration.value || media.value.duration || media.value.preview.end
  return source > 0 ? Math.max(0, source + map.shift + tailSeconds.value) : 0
})
const canDoubleBpm = computed(() => Number.isFinite(Number(map.bpm)) && Number(map.bpm) >= 1 && Number(map.bpm) <= 500)
const canHalveBpm = computed(() => Number.isFinite(Number(map.bpm)) && Number(map.bpm) >= 2 && Number(map.bpm) <= 1000)
const currentLeadIn = computed(() => analysis.value ? Math.max(0, analysis.value.leading + map.shift) : null)
const currentLeadBeats = computed(() => {
  if (!canAlign.value) return null
  const target = firstBeatAlignment(alignment.value || 'minimum')
  return target.shift === map.shift
    ? target.beatIndex
    : Math.max(0, beatsFor(analysis.value.firstBeat + map.shift))
})
const fixedLeadTarget = computed(() => fixedLeadOverride.value ?? { value: settings.leadIn, unit: settings.leadInUnit })
const validFixedLead = computed(() => Number.isFinite(fixedLeadInput.value) && fixedLeadInput.value >= 0)
const beatValue = new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 3 })
const leadBeatLabel = computed(() => {
  const value = currentLeadBeats.value
  if (value == null) return ''
  const roundsToInteger = !Number.isInteger(value) && Number.isInteger(Number(value.toFixed(3)))
  return `${roundsToInteger ? value : beatValue.format(value)} 拍`
})
const offsetSeconds = new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 3, signDisplay: 'always' })
const historyDate = new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: 'short', day: 'numeric' })
const allHistorySelected = computed(() => historyEntries.value.length > 0
  && selectedHistoryIds.value.length === historyEntries.value.length)
const historyDraft = computed(() => {
  if (!historyId.value || !media.value) return null
  const { title, subtitle, artist, mapper, bpm, version, quality, preview, shift, tailOffset, tailOffsetUnit } = map
  return {
    id: historyId.value,
    url: draftUrl.value,
    cover: media.value.thumbnail,
    customCover: customCover.value,
    form: {
      title, subtitle, artist, mapper, bpm: Number.isFinite(bpm) ? bpm : null, version, quality,
      preview, previewToTrackEnd: map.previewToTrackEnd, previewSource: previewSource.value, shift, alignment: alignment.value,
      fixedLead: fixedLeadOverride.value,
      editingFixedLead: editingFixedLead.value,
      fixedLeadInput: editingFixedLead.value && Number.isFinite(fixedLeadInput.value) ? fixedLeadInput.value : null,
      tailOffset: Number.isFinite(tailOffset) ? tailOffset : null, tailOffsetUnit,
    },
  }
})
const heading = computed(() => navigation.find(({ id }) => id === page.value)?.name)
const pendingHistory = new Map()
let historyWriter = null
let resultScrollTop = 0
let releaseProgress
let toastTimer

function syncTheme(event) { systemDark.value = event.matches }
function beatsFor(time) { return time * map.bpm / 60 }
function offsetInSeconds(value, unit) {
  return unit === 'beats' ? (hasBpm.value ? value * 60 / map.bpm : 0) : value
}

function adjustBpm(multiplier) {
  const bpm = Number(map.bpm)
  if (!Number.isFinite(bpm) || bpm < 1 || bpm > 1000) return
  const adjusted = bpm * multiplier
  if (adjusted < 1 || adjusted > 1000) return
  map.bpm = Number(adjusted.toPrecision(12))
}
watch(effectiveTheme, color => { document.documentElement.dataset.theme = color }, { immediate: true })
watch([page, step, login, resultVisible], async ([, , , visible], [, , , wasVisible]) => {
  if (wasVisible && !visible) resultScrollTop = scroll.value?.scrollTop ?? 0
  await nextTick()
  scroll.value?.scrollTo(0, resultVisible.value ? resultScrollTop : 0)
})
watch([() => map.shift, tailSeconds], () => { preview.value = '' })
watch(trackDuration, duration => {
  if (media.value && (map.previewToTrackEnd || map.preview.end > duration)) alignPreviewRange()
})
watch(() => map.bpm, () => {
  if (analysis.value && alignment.value) applyAlignment(canAlign.value ? alignment.value : '')
})
watch(historyDraft, draft => {
  if (!draft) return
  pendingHistory.set(draft.id, draft)
  void flushHistory()
})
watch(page, destination => {
  if (destination !== 'history') setHistorySelection(false)
})

onMounted(async () => {
  osDark.addEventListener('change', syncTheme)
  if (desktop) {
    releaseProgress = await listen('media-progress', ({ payload }) => {
      if (payload.operation === busy.value) progress.value = payload.message
    }).catch(() => undefined)
  }
  await load()
})
onBeforeUnmount(() => {
  osDark.removeEventListener('change', syncTheme)
  releaseProgress?.()
  clearTimeout(toastTimer)
})

async function load() {
  startupError.value = ''
  try {
    Object.assign(settings, await invoke('load_settings'))
    gameDir.value = settings.gameDir
    Object.assign(changes, settings)
    ready.value = true
    if (!initialized.value && !gameDir.value) scanGame()
    await loadAccounts()
  } catch (reason) {
    startupError.value = desktop ? String(reason) : '请通过 Beatmaps Starter 桌面应用访问。'
  }
}

async function loadAccounts() {
  accountsError.value = ''
  try {
    const result = await invoke('account_names')
    for (const { id } of platforms) accounts[id] = result[id] ?? ''
  } catch (reason) {
    accountsError.value = String(reason)
  }
}

async function run(operation, message, action) {
  if (busy.value) return
  busy.value = operation
  progress.value = message
  error.value = ''
  try {
    await action()
  } catch (reason) {
    error.value = String(reason)
  } finally {
    busy.value = ''
    progress.value = ''
  }
}

async function scanGame() {
  scanning.value = true
  const previous = gameDir.value
  try {
    const dir = await invoke('discover_game')
    if (gameDir.value === previous && dir) gameDir.value = dir
  } catch (reason) {
    error.value = String(reason)
  } finally {
    scanning.value = false
  }
}

function browseDirectory(target) {
  run('directory', '', async () => {
    const current = target === 'setup' ? gameDir.value : changes.gameDir
    const defaultPath = await invoke('game_dialog_directory', { path: current })
    const path = await open({ directory: true, multiple: false, defaultPath: defaultPath || undefined, title: '选择 Beat Saber 游戏目录' })
    if (!path) return
    const validated = await invoke('validate_game_dir', { path })
    if (target === 'setup') gameDir.value = validated
    else changes.gameDir = validated
  })
}

function checkForUpdates() {
  run('update-check', '正在检查更新', async () => {
    availableUpdate.value = null
    updateStatus.value = ''
    updateError.value = ''
    try {
      availableUpdate.value = await invoke('check_update')
      if (!availableUpdate.value) updateStatus.value = '没有可用的正式版更新'
    } catch (reason) {
      updateStatus.value = String(reason)
    }
  })
}

function installUpdate() {
  if (!availableUpdate.value) return
  const { version } = availableUpdate.value
  run('update', '', async () => {
    updateError.value = ''
    updateDownload.value = { phase: 'downloading', received: 0, total: null }
    let receivingProgress = true
    const onEvent = new Channel(event => {
      if (receivingProgress) updateDownload.value = event
    })
    try {
      await flushHistory()
      await invoke('install_update', { version, onEvent })
    } catch (reason) {
      updateError.value = String(reason)
      updateDownload.value = null
    } finally {
      receivingProgress = false
    }
  })
}

function dismissUpdate() {
  if (busy.value === 'update') return
  availableUpdate.value = null
  updateDownload.value = null
}

function openProjectSource() {
  run('project-source', '', () => invoke('open_project_source'))
}

function setupNext() {
  run('setup', '', async () => {
    if (step.value === 0 && gameDir.value.trim()) {
      gameDir.value = await invoke('validate_game_dir', { path: gameDir.value })
    }
    if (step.value < 2) {
      step.value += 1
      return
    }
    if ([settings.leadIn, settings.tailOffset].some(value => !Number.isFinite(value) || value < 0)) {
      throw new Error('默认前后偏移必须是大于或等于 0 的数字')
    }
    const saved = await invoke('save_settings', { settings: { ...settings, gameDir: gameDir.value, initialized: true } })
    Object.assign(settings, saved)
    Object.assign(changes, saved)
  })
}

function setupBack() {
  step.value -= 1
  error.value = ''
}

function navigate(destination) {
  if (busy.value) return
  error.value = ''
  login.value = null
  settingsFromResult.value = false
  page.value = destination
  if (destination === 'accounts') loadAccounts()
  if (destination === 'history') void loadHistory()
  if (destination === 'settings') Object.assign(changes, settings)
}

function closeDraft() {
  historyId.value = ''
  media.value = null
  map.id = ''
  preview.value = ''
  error.value = ''
}

async function writeHistory() {
  while (pendingHistory.size) {
    const [id, payload] = pendingHistory.entries().next().value
    pendingHistory.delete(id)
    try {
      await invoke('save_history', { payload })
      historySaveError.value = ''
    } catch (reason) {
      historySaveError.value = String(reason)
    }
  }
}

function flushHistory() {
  if (!historyWriter && pendingHistory.size) {
    historyWriter = writeHistory().finally(() => {
      historyWriter = null
      return flushHistory()
    })
  }
  return historyWriter
}

async function closeWindow() {
  await nextTick()
  await flushHistory()
  await desktop?.close()
}

async function loadHistory() {
  if (loadingHistory.value) return
  loadingHistory.value = true
  historyError.value = ''
  try {
    await nextTick()
    await flushHistory()
    historyEntries.value = await invoke('list_history')
    const existing = new Set(historyEntries.value.map(entry => entry.id))
    selectedHistoryIds.value = selectedHistoryIds.value.filter(id => existing.has(id))
  } catch (reason) {
    historyError.value = String(reason)
  } finally {
    loadingHistory.value = false
  }
}

function openHistory(entry) {
  if (media.value && historyId.value === entry.id) {
    navigate('home')
    return
  }
  run('history-open', '正在恢复编辑', async () => {
    await flushHistory()
    await openDraft(entry.url, entry)
  })
}

function setHistorySelection(enabled) {
  selectingHistory.value = enabled
  selectedHistoryIds.value = []
}

function toggleAllHistory(event) {
  selectedHistoryIds.value = event.target.checked ? historyEntries.value.map(entry => entry.id) : []
}

function deleteHistory(ids) {
  if (!ids.length) return
  run('history-delete', '', async () => {
    const deleted = new Set(ids)
    const activeId = deleted.has(historyId.value) ? historyId.value : ''
    if (activeId) historyId.value = ''
    try {
      await flushHistory()
      await invoke('delete_history', { ids })
      historyEntries.value = historyEntries.value.filter(entry => !deleted.has(entry.id))
      selectedHistoryIds.value = selectedHistoryIds.value.filter(id => !deleted.has(id))
      if (!historyEntries.value.length) setHistorySelection(false)
    } catch (reason) {
      if (activeId) historyId.value = activeId
      throw reason
    }
  })
}

function openLogin(platform) {
  if (busy.value) return
  error.value = ''
  login.value = platform
}

function completeLogin(name) {
  if (!login.value) return
  accounts[login.value.id] = name
  login.value = null
  showToast('登录成功')
}

function logout(platform) {
  run('account', '', async () => {
    await invoke('logout', { platform })
    accounts[platform] = ''
    if (media.value?.platform === platform) closeDraft()
    showToast('已退出登录')
  })
}

function saveSettings() {
  run('settings', '正在保存', async () => {
    const saved = await invoke('save_settings', { settings: { ...changes } })
    Object.assign(settings, saved)
    Object.assign(changes, saved)
    if (settingsFromResult.value && saved.gameDir) {
      settingsFromResult.value = false
      page.value = 'home'
    }
    showToast('设置已保存')
  })
}

function configureForExport() {
  navigate('settings')
  settingsFromResult.value = true
}

function showToast(message, path = '') {
  clearTimeout(toastTimer)
  notification.value = { message, path }
  toastTimer = setTimeout(() => { notification.value = null }, path ? 12000 : 4500)
}

function parse() {
  run('resolve', '正在解析链接', () => openDraft(sourceUrl.value.trim()))
}

async function openDraft(url, entry = null) {
  const result = await invoke('resolve_media', { url, historyId: entry?.id ?? null })
  const saved = entry?.form
  resultScrollTop = 0
  media.value = result
  historyId.value = entry?.id ?? result.id
  draftUrl.value = url
  sourceUrl.value = url
  customCover.value = entry?.customCover ?? false
  decodedSourceDuration.value = 0
  preview.value = ''
  exportPath.value = ''
  editing.value = false
  analysis.value = null
  analysisError.value = ''
  analyzing.value = false
  alignment.value = saved?.alignment ?? ''
  fixedLeadOverride.value = saved?.fixedLead ?? null
  editingFixedLead.value = saved?.editingFixedLead ?? false
  fixedLeadInput.value = saved?.fixedLeadInput ?? ''
  previewSource.value = saved?.previewSource ?? result.preview
  Object.assign(map, {
    id: result.id, title: saved?.title ?? result.title, subtitle: saved?.subtitle ?? '', artist: saved?.artist ?? result.artist,
    quality: result.qualities.some(item => item.id === saved?.quality) ? saved.quality : pickQuality(result, settings),
    version: saved?.version ?? settings.version, mapper: saved?.mapper ?? settings.mapper, bpm: saved?.bpm ?? null,
    preview: saved?.preview ?? result.preview, shift: saved?.shift ?? 0,
    previewToTrackEnd: saved?.previewToTrackEnd ?? result.previewToTrackEnd,
    tailOffset: saved ? saved.tailOffset : settings.tailOffset,
    tailOffsetUnit: saved?.tailOffsetUnit ?? settings.tailOffsetUnit,
  })
  if (map.previewToTrackEnd || map.preview.end > trackDuration.value) alignPreviewRange()
  page.value = 'home'
  if (result.qualities.length) void analyzeAudio(result.id, saved?.bpm == null)
}

async function analyzeAudio(id, fillBpm = true) {
  analyzing.value = true
  analysisError.value = ''
  try {
    const result = await invoke('analyze_audio', { id })
    if (id === map.id) {
      analysis.value = result
      if (fillBpm && !map.bpm) map.bpm = result.bpm
    }
  } catch (reason) {
    if (id === map.id) analysisError.value = String(reason)
  } finally {
    if (id === map.id) analyzing.value = false
  }
}

function firstBeatAlignment(mode) {
  if (!mode || !canAlign.value) return null
  const { leading, firstBeat } = analysis.value
  const period = 60 / map.bpm
  const target = fixedLeadTarget.value
  const beatIndex = Math.max(0, mode === 'minimum'
    ? Math.ceil(firstBeat / period - 1e-9)
    : Math.max(Math.ceil((firstBeat - leading) / period - 1e-9),
      Math.round((offsetInSeconds(target.value, target.unit) + firstBeat - leading) / period)))
  const shift = beatIndex * period - firstBeat
  return { beatIndex, shift: Math.abs(shift) < 1e-8 ? 0 : shift }
}

function applyAlignment(mode) {
  if (mode === 'fixed' && canAlign.value && !fixedLeadOverride.value) fixedLeadOverride.value = fixedLeadTarget.value
  const shift = firstBeatAlignment(mode)?.shift ?? 0
  alignment.value = mode
  if (mode !== 'fixed') editingFixedLead.value = false
  if (shift === map.shift) return
  map.shift = shift
  alignPreviewRange()
}

function alignPreviewRange() {
  const duration = trackDuration.value
  const end = map.previewToTrackEnd ? duration : Math.min(duration, previewSource.value.end + map.shift)
  const start = map.previewToTrackEnd && previewSource.value.start === 0
    ? 0 : Math.max(0, previewSource.value.start + map.shift)
  const range = end > start ? { start, end } : { start: 0, end: duration }
  if (range.start !== map.preview.start || range.end !== map.preview.end) map.preview = range
}

async function editFixedLead() {
  if (!editingFixedLead.value) {
    const { value, unit } = fixedLeadTarget.value
    fixedLeadInput.value = Number(beatsFor(offsetInSeconds(value, unit)).toFixed(3))
  }
  editingFixedLead.value = true
  await nextTick()
  fixedLeadField.value?.focus()
}

async function closeFixedLeadEditor() {
  editingFixedLead.value = false
  await nextTick()
  fixedLeadButton.value?.focus()
}

function saveFixedLead() {
  if (!validFixedLead.value || !canAlign.value || busy.value) return
  fixedLeadOverride.value = { value: fixedLeadInput.value, unit: 'beats' }
  applyAlignment('fixed')
  void closeFixedLeadEditor()
}

function updatePreviewRange(range) {
  if (range.end !== map.preview.end) map.previewToTrackEnd = false
  const previousStart = previewSource.value.start
  const start = range.start === map.preview.start ? previousStart : range.start - map.shift
  map.preview = range
  previewSource.value = { start, end: range.end - map.shift }
}

function updateTrackDuration(duration) {
  const source = duration - map.shift - tailSeconds.value
  if (source > 0) decodedSourceDuration.value = source
}

async function editMetadata() {
  editing.value = !editing.value
  if (editing.value) {
    await nextTick()
    titleField.value?.focus()
  }
}

function chooseCover() {
  run('cover', '', async () => {
    const path = await open({
      multiple: false, title: '替换谱面封面',
      filters: [{ name: '图片', extensions: ['png', 'jpg', 'jpeg', 'webp'] }],
    })
    if (path) coverCrop.value = await invoke('prepare_cover_crop', { id: map.id, path })
  })
}

function editCoverCrop() {
  run('cover', '', async () => {
    coverCrop.value = await invoke('prepare_cover_crop', { id: map.id, path: null })
  })
}

function applyCoverCrop(region) {
  run('cover', '', async () => {
    Object.assign(media.value, await invoke('apply_cover_crop', { id: map.id, region }))
    customCover.value = true
    coverCrop.value = null
  })
}

function cancelCoverCrop() {
  run('cover', '', async () => {
    await invoke('discard_cover_crop', { id: map.id })
    coverCrop.value = null
  })
}

function playPreview() {
  run('preview', '正在准备试听', async () => {
    const { id, shift } = map
    const path = await invoke('preview_audio', { id, shift, tailSeconds: tailSeconds.value })
    preview.value = convertFileSrc(path)
  })
}

function exportMap() {
  run('export', '正在导出', async () => {
    const path = await invoke('export_map', { payload: { ...map, bpm: Number(map.bpm), tailSeconds: tailSeconds.value } })
    exportPath.value = path
    showToast('谱面导出成功', path)
  })
}

async function openExport(path) {
  try {
    await invoke('open_export_folder', { path })
  } catch (reason) {
    error.value = String(reason)
  }
}
</script>

<template>
  <div class="app-shell">
    <header class="titlebar" data-tauri-drag-region>
      <div class="window-title" data-tauri-drag-region><span class="logo-mark" data-tauri-drag-region></span><span data-tauri-drag-region>Beatmaps Starter</span></div>
      <div class="window-controls" @dblclick.stop>
        <button class="window-button" type="button" title="最小化" aria-label="最小化" @click="desktop?.minimize()"><Minus :size="15" /></button>
        <button class="window-button" type="button" title="最大化或还原" aria-label="最大化或还原" @click="desktop?.toggleMaximize()"><Square :size="12" /></button>
        <button class="window-button window-close" type="button" title="关闭" aria-label="关闭" @click="closeWindow"><X :size="17" /></button>
      </div>
    </header>

    <div v-if="!ready" class="app-loading">
      <template v-if="startupError">
        <p class="inline-error" role="alert">{{ startupError }}</p>
        <button v-if="desktop" class="button secondary" type="button" @click="load">重新加载</button>
      </template>
      <LoaderCircle v-else class="spin" :size="26" aria-label="正在加载" />
    </div>

    <div v-else class="app-body" :class="{ onboarding: !initialized }">
      <aside v-if="initialized" class="sidebar">
        <nav aria-label="主导航">
          <button v-for="item in navigation" :key="item.id" type="button" class="nav-link"
            :class="{ selected: item.id === page }"
            :aria-current="item.id === page ? 'page' : undefined"
            :disabled="!!busy" @click="navigate(item.id)">
            <component :is="item.icon" :size="18" :stroke-width="2" />{{ item.name }}
          </button>
        </nav>
        <button type="button" class="sidebar-status" :disabled="!!busy" @click="navigate('settings')">
          <span :class="['status-dot', { connected: settings.gameDir }]"></span>
          {{ settings.gameDir ? '游戏目录已配置' : '配置游戏目录' }}
          <ChevronRight :size="14" />
        </button>
      </aside>

      <main ref="scroll" class="workspace">
        <LoginPage v-if="login" :key="login.id" :platform="login" @back="login = null" @complete="completeLogin" />

        <section v-else-if="!initialized" class="wizard">
          <ol class="steps" aria-label="首次启动配置进度">
            <li v-for="(label, index) in steps" :key="label" :class="{ current: step === index, done: step > index }" :aria-current="step === index ? 'step' : undefined">
              <span class="step-dot"><Check v-if="step > index" :size="15" /><template v-else>{{ index + 1 }}</template></span>{{ label }}
            </li>
          </ol>
          <div class="wizard-center">
            <template v-if="step === 0">
              <div class="section-symbol"><Folder :size="27" /></div>
              <h1>配置 Beat Saber</h1>
              <p class="muted">选择包含 Beat Saber.exe 的游戏目录。</p>
              <div class="path-field">
                <input v-model="gameDir" aria-label="Beat Saber 游戏目录" placeholder="选择或输入游戏目录" spellcheck="false" />
                <button type="button" class="button secondary" :disabled="!!busy" @click="browseDirectory('setup')"><FolderOpen :size="17" /> 浏览</button>
              </div>
              <p v-if="scanning" class="muted scan-label" role="status"><LoaderCircle class="spin" :size="14" /> 正在搜索游戏目录</p>
            </template>
            <template v-else-if="step === 1">
              <div class="section-symbol"><UserRound :size="27" /></div>
              <h1>配置流媒体账号</h1>
              <p class="muted">绑定账号可获取账号有权播放的音质，也可以稍后配置。</p>
              <div class="platform-grid">
                <button v-for="platform in platforms" :key="platform.id" type="button" class="platform-card"
                  :class="{ connected: accounts[platform.id] }" :disabled="!!accounts[platform.id]" @click="openLogin(platform)">
                  <PlatformIcon :platform="platform.id" />
                  <strong>{{ platform.name }}</strong>
                  <span class="muted"><Check v-if="accounts[platform.id]" :size="14" />{{ accounts[platform.id] || '登录' }}</span>
                </button>
              </div>
              <p v-if="accountsError" class="inline-error" role="alert">{{ accountsError }}</p>
            </template>
            <template v-else>
              <div class="section-symbol"><AudioLines :size="27" /></div>
              <h1>配置默认谱面信息</h1>
              <p class="muted">解析完成后仍可单独调整每张谱面。</p>
              <div class="fields wizard-fields">
                <label class="field">默认谱面版本<select v-model="settings.version"><option value="legacy">V2 / V3</option><option value="v4">V4</option></select></label>
                <label class="field">谱师<input v-model="settings.mapper" maxlength="120" placeholder="填写谱师名称" /></label>
                <OffsetDefaults class="wizard-offsets" v-model:lead-in="settings.leadIn" v-model:lead-in-unit="settings.leadInUnit"
                  v-model:tail-offset="settings.tailOffset" v-model:tail-offset-unit="settings.tailOffsetUnit" :disabled="!!busy" />
              </div>
            </template>
            <p v-if="error" class="inline-error" role="alert">{{ error }}</p>
          </div>
          <footer class="wizard-actions">
            <button v-if="step > 0" type="button" class="button secondary" :disabled="!!busy" @click="setupBack"><ArrowLeft :size="17" /> 上一步</button>
            <button type="button" class="button primary" :disabled="!!busy" @click="setupNext">
              {{ step === 2 ? '完成' : step === 1 && !Object.values(accounts).some(Boolean) ? '跳过' : '下一步' }}
              <Check v-if="step === 2" :size="17" /><ArrowRight v-else :size="17" />
            </button>
          </footer>
        </section>

        <section v-else-if="page === 'home' && !media" class="home">
          <div class="home-surface">
            <div class="home-symbol" aria-hidden="true"><AudioLines :size="37" :stroke-width="1.6" /></div>
            <h1>从一首歌开始制谱</h1>
            <p class="muted">粘贴歌曲或视频链接，准备 Beat Saber 谱面的音频和封面。</p>
            <form class="parse-form" @submit.prevent="parse">
              <label class="url-field">
                <Link2 :size="20" aria-hidden="true" />
                <input v-model="sourceUrl" aria-label="歌曲或视频链接" type="text" placeholder="粘贴歌曲或视频链接" required spellcheck="false" :disabled="!!busy" />
              </label>
              <button type="submit" class="button primary parse-button" :disabled="!!busy || !sourceUrl.trim()">
                <LoaderCircle v-if="busy" class="spin" :size="18" />
                {{ busy ? progress : '解析链接' }}
                <ArrowRight v-if="!busy" :size="18" />
              </button>
            </form>
            <p v-if="error" class="inline-error" role="alert">{{ error }}</p>
            <div class="supported-platforms" aria-label="支持的平台">
              <span v-for="platform in platforms" :key="platform.id"><PlatformIcon :platform="platform.id" />{{ platform.name }}</span>
            </div>
          </div>
        </section>

        <section v-if="initialized && media" v-show="resultVisible" class="page result-page">
          <button type="button" class="text-button back-button" :disabled="!!busy" @click="closeDraft"><ArrowLeft :size="17" /> 返回首页</button>
          <form @submit.prevent="exportMap">
            <div class="media-summary">
              <div class="media-cover">
                <img :src="media.cover" alt="谱面封面" />
                <div class="cover-actions">
                  <button type="button" :disabled="!!busy" aria-label="替换封面" title="替换封面" @click="chooseCover"><FileImage :size="17" /></button>
                  <button type="button" :disabled="!!busy" aria-label="裁切封面" title="裁切封面" @click="editCoverCrop"><Crop :size="17" /></button>
                </div>
              </div>
              <div class="media-heading">
                <div class="media-source"><PlatformIcon :platform="media.platform" />{{ sourcePlatform.name }}
                  <span v-if="media.duration > 0" class="duration">{{ durationText(media.duration) }}</span>
                </div>
                <button type="button" class="media-title" :disabled="!!busy" aria-label="编辑歌曲信息" :aria-expanded="editing" @click="editMetadata">
                  <h1>{{ map.title || '标题' }}</h1><Pencil :size="17" />
                </button>
                <p class="muted" v-if="map.artist || map.subtitle">{{ [map.artist, map.subtitle].filter(Boolean).join(' · ') }}</p>
              </div>
            </div>
            <div v-if="editing" class="panel fields edit-fields">
              <label class="field">标题<input ref="titleField" v-model="map.title" required maxlength="250" :disabled="!!busy" /></label>
              <label class="field">副标题<input v-model="map.subtitle" maxlength="250" :disabled="!!busy" /></label>
              <label class="field">歌手<input v-model="map.artist" maxlength="250" :disabled="!!busy" /></label>
            </div>

            <div class="panel result-panel">
              <h2>导出音质</h2>
              <select v-if="media.qualities.length" v-model="map.quality" aria-label="音质" required :disabled="!!busy">
                <option v-for="item in media.qualities" :key="item.id" :value="item.id">{{ item.label }}</option>
              </select>
              <p v-else class="muted">没有可用的完整音轨。登录账号或更换链接后重新解析。</p>
              <PreviewPlayer :key="`${map.id}:${map.shift}:${tailSeconds}`" :range="map.preview" :track-duration="trackDuration"
                :src="preview" :loading="busy === 'preview'" :disabled="!!busy" :can-play="!!map.quality" :active="resultVisible && !coverCrop"
                :end-at-track="map.previewToTrackEnd" @duration="updateTrackDuration" @update:range="updatePreviewRange" @load="playPreview" />
              <p v-if="busy === 'preview'" class="muted preview-status" role="status">{{ progress }}</p>
              <div class="audio-offset-row">
                <div class="alignment-controls">
                  <span class="muted" aria-live="polite">音频前偏移
                    <button v-if="alignment === 'fixed'" ref="fixedLeadButton" type="button" class="alignment-value"
                      :disabled="!!busy || !canAlign" :aria-label="leadBeatLabel ? `调整固定前偏移，当前 ${leadBeatLabel}` : '调整固定前偏移'"
                      :aria-expanded="editingFixedLead" @click="editFixedLead">{{ leadBeatLabel || '—' }}</button>
                    <template v-else-if="leadBeatLabel">{{ leadBeatLabel }}</template>
                    <span v-if="currentLeadIn != null" :class="{ 'offset-positive': map.shift > 0, 'offset-negative': map.shift < 0 }">（{{ offsetSeconds.format(map.shift) }} 秒）</span>
                  </span>
                  <button type="button" class="button secondary" :disabled="!!busy || !canAlign" :aria-pressed="alignment === 'minimum'"
                    @click="applyAlignment('minimum')">最小变动</button>
                  <button type="button" class="button secondary" :class="{ 'alignment-suggested': alignment !== 'fixed' && currentLeadIn != null && currentLeadIn < 2 }"
                    :disabled="!!busy || !canAlign" :aria-pressed="alignment === 'fixed'" @click="applyAlignment('fixed')">按固定值</button>
                  <button v-if="alignment" type="button" class="text-button" :disabled="!!busy" @click="applyAlignment('')">还原</button>
                </div>
                <label class="tail-offset muted">音频后偏移
                  <input v-model.number="tailSecondsInput" type="number" min="0" step="any" required
                    aria-label="音频后偏移（秒）" placeholder="—" :disabled="!!busy" />
                  <span>秒</span>
                </label>
              </div>
              <div v-if="editingFixedLead" class="fixed-lead-editor" @keydown.esc.prevent.stop="closeFixedLeadEditor">
                <label for="fixed-lead">目标前留白（拍）</label>
                <input id="fixed-lead" ref="fixedLeadField" v-model.number="fixedLeadInput" type="number" min="0" step="any"
                  :disabled="!!busy" @keydown.enter.prevent="saveFixedLead" />
                <button type="button" class="button primary" :disabled="!!busy || !validFixedLead || !canAlign" @click="saveFixedLead">保存</button>
                <button type="button" class="text-button" :disabled="!!busy" @click="closeFixedLeadEditor">取消</button>
              </div>
            </div>

            <div class="panel result-panel">
              <h2>谱面信息</h2>
              <div class="fields result-fields">
                <label class="field">谱面版本<select v-model="map.version" :disabled="!!busy"><option value="legacy">V2 / V3</option><option value="v4">V4</option></select></label>
                <label class="field">谱师<input v-model="map.mapper" maxlength="120" placeholder="填写谱师名称" :disabled="!!busy" /></label>
                <div class="field bpm-field">
                  <label for="map-bpm">BPM</label>
                  <div class="bpm-control">
                    <input id="map-bpm" v-model.number="map.bpm" type="number" min="1" max="1000" step="any"
                      :placeholder="analyzing ? '正在分析…' : '手动填写'" :aria-busy="analyzing" aria-label="BPM，向上键翻倍，向下键减半"
                      required :disabled="!!busy || analyzing" @keydown.up.prevent="adjustBpm(2)" @keydown.down.prevent="adjustBpm(0.5)" />
                    <span class="bpm-adjusters">
                      <button type="button" aria-label="BPM 翻倍" title="翻倍" :disabled="!!busy || analyzing || !canDoubleBpm" @click="adjustBpm(2)"><ChevronUp :size="15" /></button>
                      <button type="button" aria-label="BPM 减半" title="减半" :disabled="!!busy || analyzing || !canHalveBpm" @click="adjustBpm(0.5)"><ChevronDown :size="15" /></button>
                    </span>
                  </div>
                </div>
              </div>
              <div v-if="analysisError" class="alignment-controls">
                <p class="muted" role="status">BPM 分析失败，{{ analysisError }}</p>
                <button type="button" class="text-button" :disabled="!!busy" @click="analyzeAudio(map.id)">重新分析</button>
              </div>
              <p v-else-if="analysis && !analysis.bpm" class="muted" role="status">未检测到稳定的节拍，请手动填写 BPM。</p>
            </div>
            <p v-if="error" class="inline-error" role="alert">{{ error }}</p>
            <p v-if="historySaveError" class="inline-error" role="alert">未能自动保存历史。{{ historySaveError }}</p>
            <div class="export-footer">
              <button v-if="!settings.gameDir" type="button" class="text-button" :disabled="!!busy" @click="configureForExport"><Folder :size="16" /> 设置游戏目录</button>
              <button v-else-if="exportPath" type="button" class="text-button" :disabled="!!busy" @click="openExport(exportPath)"><FolderOpen :size="16" /> 打开谱面目录</button>
              <span v-else class="muted"><Folder :size="16" /> CustomWIPLevels</span>
              <button type="submit" class="button primary" :disabled="!!busy || analyzing || editingFixedLead || !map.quality || !hasBpm || !validTail || !map.title.trim() || !settings.gameDir">
                <LoaderCircle v-if="busy === 'export'" class="spin" :size="18" /><ArrowDownToLine v-else :size="18" />
                {{ busy === 'export' ? progress : '导出谱面' }}
              </button>
            </div>
          </form>
        </section>

        <section v-if="initialized && !login && page === 'history'" class="page">
          <div class="page-title history-heading">
            <h1>{{ heading }}</h1>
            <div v-if="historyEntries.length" class="history-actions">
              <template v-if="selectingHistory">
                <label class="history-select muted">
                  <input type="checkbox" :checked="allHistorySelected" :indeterminate="!!selectedHistoryIds.length && !allHistorySelected"
                    :disabled="!!busy || loadingHistory" @change="toggleAllHistory" /> 全选
                </label>
                <button class="button secondary history-batch-delete" type="button"
                  :disabled="!!busy || loadingHistory || !selectedHistoryIds.length" @click="deleteHistory(selectedHistoryIds)">
                  <Trash2 :size="16" /> 删除所选（{{ selectedHistoryIds.length }}）
                </button>
              </template>
              <button class="button secondary" type="button" :disabled="!!busy || loadingHistory"
                @click="setHistorySelection(!selectingHistory)">{{ selectingHistory ? '取消多选' : '多选' }}</button>
            </div>
          </div>
          <p v-if="loadingHistory || busy === 'history-open'" class="history-status muted" role="status">
            <LoaderCircle class="spin" :size="16" />{{ busy === 'history-open' ? progress : '正在读取历史' }}
          </p>
          <p v-if="error || historyError || historySaveError" class="inline-error" role="alert">{{ error || historyError || historySaveError }}</p>
          <p v-else-if="!loadingHistory && !historyEntries.length" class="muted">暂无历史记录</p>
          <ul v-if="historyEntries.length" class="history-list">
            <li v-for="entry in historyEntries" :key="entry.id" class="panel history-card"
              :class="{ 'history-selected': selectedHistoryIds.includes(entry.id) }">
              <img :src="entry.cover" alt="" width="128" height="128" loading="lazy" />
              <div class="history-info">
                <time class="muted" :datetime="new Date(entry.createdAt).toISOString()">{{ historyDate.format(entry.createdAt) }}</time>
                <strong>{{ [entry.form.title, entry.form.subtitle].filter(Boolean).join(' · ') || '未填写标题' }}</strong>
                <span v-if="entry.form.artist" class="muted">{{ entry.form.artist }}</span>
              </div>
              <div class="history-actions">
                <label v-if="selectingHistory" class="history-select">
                  <input v-model="selectedHistoryIds" type="checkbox" :value="entry.id"
                    :aria-label="`选择 ${entry.form.title || '未填写标题'}`" :disabled="!!busy || loadingHistory" />
                </label>
                <button type="button" class="icon-button history-view" aria-label="查看" title="查看"
                  :disabled="!!busy || loadingHistory" @click="openHistory(entry)"><Eye :size="17" /></button>
                <button type="button" class="icon-button history-delete" aria-label="删除" title="删除"
                  :disabled="!!busy || loadingHistory" @click="deleteHistory([entry.id])"><Trash2 :size="17" /></button>
              </div>
            </li>
          </ul>
        </section>

        <section v-else-if="initialized && !login && page === 'accounts'" class="page">
          <div class="page-title"><h1>{{ heading }}</h1><p class="muted">管理用于解析和下载音轨的平台账号。</p></div>
          <div class="panel account-list">
            <div v-for="platform in platforms" :key="platform.id" class="account-row">
              <PlatformIcon :platform="platform.id" />
              <div class="account-identity">
                <strong>{{ platform.name }}</strong>
                <span class="muted">{{ accounts[platform.id] || platform.domain }}</span>
              </div>
              <span class="account-status" :class="{ 'signed-in': accounts[platform.id] }"><span class="status-dot"></span>{{ accounts[platform.id] ? '已登录' : '未登录' }}</span>
              <button v-if="accounts[platform.id]" type="button" class="button secondary" :disabled="!!busy" @click="logout(platform.id)">退出登录</button>
              <button v-else type="button" class="button secondary" :disabled="!!busy" @click="openLogin(platform)">登录 <ChevronRight :size="16" /></button>
            </div>
          </div>
          <p v-if="error || accountsError" class="inline-error" role="alert">{{ error || accountsError }}</p>
        </section>

        <section v-else-if="initialized && !login && page === 'settings'" class="page">
          <div class="page-title"><h1>{{ heading }}</h1><p class="muted">配置游戏目录和新谱面的默认选项。</p></div>
          <form class="settings-form" @submit.prevent="saveSettings">
            <div class="panel settings-panel">
              <h2>通用</h2>
              <div class="settings-row"><span>语言</span><span class="muted">跟随系统 · 简体中文</span></div>
              <label class="settings-row">界面<select v-model="changes.theme" :disabled="!!busy"><option value="dark">深色</option><option value="light">浅色</option><option value="system">跟随系统</option></select></label>
              <div class="settings-row settings-path"><label for="settings-game">Beat Saber 目录</label>
                <div class="path-field">
                  <input id="settings-game" v-model="changes.gameDir" placeholder="选择或输入游戏目录" spellcheck="false" :disabled="!!busy" />
                  <button type="button" class="button secondary" :disabled="!!busy" @click="browseDirectory('settings')"><FolderOpen :size="16" /> 浏览</button>
                </div>
              </div>
              <div class="settings-row">
                <span>软件更新</span>
                <div class="update-controls">
                  <span v-if="busy === 'update-check' || updateStatus" class="muted" role="status">{{ busy === 'update-check' ? progress : updateStatus }}</span>
                  <button class="button secondary" type="button" :disabled="!!busy" @click="checkForUpdates">检查更新</button>
                </div>
              </div>
            </div>
            <div class="panel settings-panel">
              <h2>默认音质</h2>
              <label v-for="platform in platforms" :key="platform.id" class="settings-row">{{ platform.name }}
                <select v-model="changes[platform.qualityKey]" :disabled="!!busy">
                  <option v-for="[id, label] in platform.qualities" :key="id" :value="id">{{ label }}</option>
                </select>
              </label>
            </div>
            <div class="panel settings-panel">
              <h2>默认谱面信息</h2>
              <label class="settings-row">谱面版本<select v-model="changes.version" :disabled="!!busy"><option value="legacy">V2 / V3</option><option value="v4">V4</option></select></label>
              <label class="settings-row">谱师<input v-model="changes.mapper" maxlength="120" placeholder="填写谱师名称" :disabled="!!busy" /></label>
              <OffsetDefaults class="settings-row" v-model:lead-in="changes.leadIn" v-model:lead-in-unit="changes.leadInUnit"
                v-model:tail-offset="changes.tailOffset" v-model:tail-offset-unit="changes.tailOffsetUnit" :disabled="!!busy" />
            </div>
            <p v-if="error" class="inline-error" role="alert">{{ error }}</p>
            <button type="submit" class="button primary save-button" :disabled="!!busy"><Check :size="18" /> 保存设置</button>
          </form>
          <footer class="settings-footer">
            <span>项目源码</span>
            <a href="https://github.com/hikarin31768/beatmaps-starter" @click.prevent="openProjectSource">
              github.com/hikarin31768/beatmaps-starter <ExternalLink :size="13" />
            </a>
          </footer>
        </section>
      </main>
    </div>

    <CoverCropDialog v-if="coverCrop" :image="coverCrop" :saving="busy === 'cover'" :error="error"
      @apply="applyCoverCrop" @cancel="cancelCoverCrop" />

    <UpdateDialog v-if="availableUpdate" :update="availableUpdate" :download="updateDownload"
      :installing="busy === 'update'" :error="updateError" @install="installUpdate" @dismiss="dismissUpdate" />

    <Transition>
      <div v-if="notification" class="toast" role="status">
        <Check :size="20" />
        <div class="toast-content">
          <strong>{{ notification.message }}</strong>
          <button v-if="notification.path" type="button" class="text-button" @click="openExport(notification.path)">打开谱面目录 <ArrowRight :size="13" /></button>
        </div>
        <button type="button" class="icon-button" aria-label="关闭通知" @click="notification = null"><X :size="17" /></button>
      </div>
    </Transition>
  </div>
</template>
