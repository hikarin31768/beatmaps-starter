<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { open } from '@tauri-apps/plugin-dialog'
import {
  ArrowDownToLine, ArrowLeft, ArrowRight, AudioLines, Check, ChevronDown, ChevronRight, ChevronUp, FileImage, Folder,
  FolderOpen, House, Link2, LoaderCircle, Minus, Pencil, Settings2, Square, UserRound, X,
} from 'lucide-vue-next'
import LoginPage from './components/LoginPage.vue'
import PlatformIcon from './components/PlatformIcon.vue'
import PreviewPlayer from './components/PreviewPlayer.vue'
import { durationText, pickQuality, platforms } from './platforms'

const navigation = [
  { id: 'home', name: '首页', icon: House },
  { id: 'accounts', name: '账号', icon: UserRound },
  { id: 'settings', name: '设置', icon: Settings2 },
]
const steps = ['游戏目录', '流媒体账号', '谱面偏好']
const settings = reactive({
  initialized: false, gameDir: '', theme: 'dark', version: 'legacy', mapper: '', leadIn: 2,
  neteaseQuality: 'best', bilibiliQuality: 'best', youtubeQuality: 'best',
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
const editing = ref(false)
const preview = ref('')
const previewSource = ref(null)
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
const updateVersion = ref('')
const updateStatus = ref('')
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
const sourcePlatform = computed(() => platforms.find(({ id }) => id === media.value?.platform))
const trackDuration = computed(() => (media.value?.duration || media.value?.preview.end || 0) + (map.shift || 0))
const canAlign = computed(() => analysis.value?.firstBeat != null && map.bpm >= 1 && map.bpm <= 1000)
const canDoubleBpm = computed(() => Number.isFinite(Number(map.bpm)) && Number(map.bpm) >= 1 && Number(map.bpm) <= 500)
const canHalveBpm = computed(() => Number.isFinite(Number(map.bpm)) && Number(map.bpm) >= 2 && Number(map.bpm) <= 1000)
const currentLeadIn = computed(() => analysis.value ? Math.max(0, analysis.value.leading + map.shift) : null)
const fixedLeadTarget = computed(() => fixedLeadOverride.value ?? settings.leadIn)
const validFixedLead = computed(() => Number.isFinite(fixedLeadInput.value) && fixedLeadInput.value >= 0)
const seconds = new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 3 })
const offsetSeconds = new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 3, signDisplay: 'always' })
const heading = computed(() => navigation.find(({ id }) => id === page.value)?.name ?? '解析结果')
let releaseProgress
let toastTimer

function syncTheme(event) { systemDark.value = event.matches }
function adjustBpm(multiplier) {
  const bpm = Number(map.bpm)
  if (!Number.isFinite(bpm) || bpm < 1 || bpm > 1000) return
  const adjusted = bpm * multiplier
  if (adjusted < 1 || adjusted > 1000) return
  map.bpm = Number(adjusted.toPrecision(12))
}
watch(effectiveTheme, color => { document.documentElement.dataset.theme = color }, { immediate: true })
watch([page, step, login], () => scroll.value?.scrollTo(0, 0))
watch(() => map.shift, () => { preview.value = '' })
watch(() => map.bpm, () => { if (alignment.value) applyAlignment(canAlign.value ? alignment.value : '') })

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
    updateVersion.value = ''
    updateStatus.value = ''
    try {
      updateVersion.value = await invoke('check_update') || ''
      updateStatus.value = updateVersion.value ? `发现新版本 ${updateVersion.value}` : '没有可用的正式版更新'
    } catch (reason) {
      updateStatus.value = String(reason)
    }
  })
}

function installUpdate() {
  run('update', '正在下载更新', () => invoke('install_update', { version: updateVersion.value }))
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
    if (!Number.isFinite(settings.leadIn) || settings.leadIn < 0) throw new Error('默认音频偏移必须是大于或等于 0 的秒数')
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
  preview.value = ''
  login.value = null
  settingsFromResult.value = false
  page.value = destination
  if (destination === 'accounts') loadAccounts()
  if (destination === 'settings') Object.assign(changes, settings)
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
    if (media.value?.platform === platform) media.value = null
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
      page.value = 'result'
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
  run('resolve', '正在解析链接', async () => {
    const result = await invoke('resolve_media', { url: sourceUrl.value })
    media.value = result
    preview.value = ''
    exportPath.value = ''
    editing.value = false
    analysis.value = null
    analysisError.value = ''
    analyzing.value = false
    alignment.value = ''
    fixedLeadOverride.value = null
    editingFixedLead.value = false
    previewSource.value = result.preview
    Object.assign(map, {
      id: result.id, title: result.title, subtitle: '', artist: result.artist,
      quality: pickQuality(result, settings), version: settings.version, mapper: settings.mapper, bpm: null,
      preview: result.preview, shift: 0,
    })
    page.value = 'result'
    if (result.qualities.length) void analyzeAudio(result.id)
  })
}

async function analyzeAudio(id) {
  analyzing.value = true
  analysisError.value = ''
  try {
    const result = await invoke('analyze_audio', { id })
    if (id === map.id) {
      analysis.value = result
      if (!map.bpm) map.bpm = result.bpm
    }
  } catch (reason) {
    if (id === map.id) analysisError.value = String(reason)
  } finally {
    if (id === map.id) analyzing.value = false
  }
}

function applyAlignment(mode) {
  let shift = 0
  if (mode && canAlign.value) {
    const { leading, firstBeat } = analysis.value
    const period = 60 / map.bpm
    const beatIndex = mode === 'minimum'
      ? Math.ceil(firstBeat / period - 1e-9)
      : Math.max(Math.ceil((firstBeat - leading) / period - 1e-9), Math.round((fixedLeadTarget.value + firstBeat - leading) / period))
    shift = beatIndex * period - firstBeat
    if (Math.abs(shift) < 1e-8) shift = 0
  }
  alignment.value = mode
  if (mode !== 'fixed') editingFixedLead.value = false
  if (shift === map.shift) return
  map.shift = shift
  const end = Math.min(trackDuration.value, previewSource.value.end + shift)
  const start = Math.max(0, previewSource.value.start + shift)
  map.preview = end > start ? { start, end } : { start: 0, end: Math.min(trackDuration.value, 10) }
}

async function editFixedLead() {
  if (!editingFixedLead.value) fixedLeadInput.value = fixedLeadTarget.value
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
  fixedLeadOverride.value = fixedLeadInput.value
  applyAlignment('fixed')
  void closeFixedLeadEditor()
}

function updatePreviewRange(range) {
  map.preview = range
  previewSource.value = { start: range.start - map.shift, end: range.end - map.shift }
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
    if (path) media.value.cover = await invoke('replace_cover', { id: map.id, path })
  })
}

function playPreview() {
  run('preview', '正在准备试听', async () => {
    const { id, shift } = map
    const path = await invoke('preview_audio', { id, shift })
    preview.value = convertFileSrc(path)
  })
}

function exportMap() {
  run('export', '正在导出', async () => {
    const path = await invoke('export_map', { payload: { ...map, bpm: Number(map.bpm) } })
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
        <button class="window-button window-close" type="button" title="关闭" aria-label="关闭" @click="desktop?.close()"><X :size="17" /></button>
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
            :class="{ selected: item.id === page || (item.id === 'home' && page === 'result') }"
            :aria-current="item.id === page || (item.id === 'home' && page === 'result') ? 'page' : undefined"
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
                <label class="field">默认音频偏移（秒）<input v-model.number="settings.leadIn" type="number" min="0" step="any" required /></label>
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

        <section v-else-if="page === 'home'" class="home">
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

        <section v-else-if="page === 'result' && media" class="page result-page">
          <button type="button" class="text-button back-button" :disabled="!!busy" @click="navigate('home')"><ArrowLeft :size="17" /> 返回首页</button>
          <form @submit.prevent="exportMap">
            <div class="media-summary">
              <button class="cover-button" type="button" :disabled="!!busy" aria-label="替换封面" title="替换封面" @click="chooseCover">
                <img :src="media.cover" alt="谱面封面" />
                <span><FileImage :size="15" /> 替换封面</span>
              </button>
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
              <PreviewPlayer :key="`${map.id}:${map.shift}`" :range="map.preview" :track-duration="trackDuration"
                :src="preview" :loading="busy === 'preview'" :disabled="!!busy" :can-play="!!map.quality"
                @update:range="updatePreviewRange" @load="playPreview" />
              <p v-if="busy === 'preview'" class="muted preview-status" role="status">{{ progress }}</p>
              <div class="alignment-controls">
                <span class="muted" aria-live="polite">音频偏移
                  <button v-if="alignment === 'fixed'" ref="fixedLeadButton" type="button" class="alignment-value"
                    :disabled="!!busy || !canAlign" :aria-label="`调整固定偏移目标，当前 ${seconds.format(currentLeadIn)} 秒`" :aria-expanded="editingFixedLead"
                    @click="editFixedLead">{{ seconds.format(currentLeadIn) }} 秒</button>
                  <template v-else-if="currentLeadIn != null">{{ seconds.format(currentLeadIn) }} 秒</template>
                  <span v-if="currentLeadIn != null" :class="{ 'offset-positive': map.shift > 0, 'offset-negative': map.shift < 0 }">（{{ offsetSeconds.format(map.shift) }} 秒）</span>
                </span>
                <button type="button" class="button secondary" :disabled="!!busy || !canAlign" :aria-pressed="alignment === 'minimum'"
                  @click="applyAlignment('minimum')">最小变动</button>
                <button type="button" class="button secondary" :class="{ 'alignment-suggested': alignment !== 'fixed' && currentLeadIn != null && currentLeadIn < 2 }"
                  :disabled="!!busy || !canAlign" :aria-pressed="alignment === 'fixed'" @click="applyAlignment('fixed')">按固定值</button>
                <button v-if="alignment" type="button" class="text-button" :disabled="!!busy" @click="applyAlignment('')">还原</button>
              </div>
              <div v-if="editingFixedLead" class="fixed-lead-editor" @keydown.esc.prevent.stop="closeFixedLeadEditor">
                <label for="fixed-lead">目标留白（秒）</label>
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
            <div class="export-footer">
              <button v-if="!settings.gameDir" type="button" class="text-button" :disabled="!!busy" @click="configureForExport"><Folder :size="16" /> 设置游戏目录</button>
              <button v-else-if="exportPath" type="button" class="text-button" :disabled="!!busy" @click="openExport(exportPath)"><FolderOpen :size="16" /> 打开谱面目录</button>
              <span v-else class="muted"><Folder :size="16" /> CustomWIPLevels</span>
              <button type="submit" class="button primary" :disabled="!!busy || analyzing || editingFixedLead || !map.quality || !map.bpm || !map.title.trim() || !settings.gameDir">
                <LoaderCircle v-if="busy === 'export'" class="spin" :size="18" /><ArrowDownToLine v-else :size="18" />
                {{ busy === 'export' ? progress : '导出谱面' }}
              </button>
            </div>
          </form>
        </section>

        <section v-else-if="page === 'accounts'" class="page">
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

        <section v-else-if="page === 'settings'" class="page">
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
                  <span v-if="busy === 'update-check' || busy === 'update' || updateStatus" class="muted" role="status">{{ busy === 'update-check' || busy === 'update' ? progress : updateStatus }}</span>
                  <button v-if="updateVersion" class="button primary" type="button" :disabled="!!busy" @click="installUpdate">下载并安装</button>
                  <button class="button secondary" type="button" :disabled="!!busy" @click="checkForUpdates">检查更新</button>
                </div>
              </div>
            </div>
            <div class="panel settings-panel">
              <h2>默认音质</h2>
              <label v-for="platform in platforms" :key="platform.id" class="settings-row">{{ platform.name }}
                <select v-model="changes[platform.qualityKey]" :disabled="!!busy">
                  <option value="best">最高可用音质</option>
                  <option v-for="[id, label] in platform.qualities" :key="id" :value="id">{{ label }}</option>
                </select>
              </label>
            </div>
            <div class="panel settings-panel">
              <h2>默认谱面信息</h2>
              <label class="settings-row">谱面版本<select v-model="changes.version" :disabled="!!busy"><option value="legacy">V2 / V3</option><option value="v4">V4</option></select></label>
              <label class="settings-row">谱师<input v-model="changes.mapper" maxlength="120" placeholder="填写谱师名称" :disabled="!!busy" /></label>
              <label class="settings-row">默认音频偏移（秒）<input v-model.number="changes.leadIn" type="number" min="0" step="any" required :disabled="!!busy" /></label>
            </div>
            <p v-if="error" class="inline-error" role="alert">{{ error }}</p>
            <button type="submit" class="button primary save-button" :disabled="!!busy"><Check :size="18" /> 保存设置</button>
          </form>
        </section>
      </main>
    </div>

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
