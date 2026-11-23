<script setup>
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import QRCode from 'qrcode'
import { ArrowLeft, ExternalLink, FileUp, LoaderCircle, RefreshCw, ScanLine } from 'lucide-vue-next'
import PlatformIcon from './PlatformIcon.vue'

const props = defineProps({ platform: { type: Object, required: true } })
const emit = defineEmits(['back', 'complete'])
const mode = ref(props.platform.id === 'youtube' ? 'cookies' : 'qr')
const qrImage = ref('')
const status = ref('正在获取二维码')
const error = ref('')
const pending = ref(false)
const account = ref('')
const country = ref('86')
const secret = ref('')
const youtubeLabel = ref('')
const smsDialog = ref(null)
const cooldown = ref(0)
const isBili = computed(() => props.platform.id === 'bilibili')
let serial = 0
let pollTimer
let expiryTimer
let cooldownTimer

function resetPolling() {
  serial += 1
  clearTimeout(pollTimer)
  clearTimeout(expiryTimer)
  qrImage.value = ''
}

function showResult(result) {
  if (result.status !== 'complete') return false
  resetPolling()
  emit('complete', result.name)
  return true
}

async function refreshQr() {
  resetPolling()
  const token = serial
  error.value = ''
  status.value = '正在获取二维码'
  try {
    const { key, url, expiresIn } = await invoke('start_qr', { platform: props.platform.id })
    const image = await QRCode.toDataURL(url, { width: 224, margin: 2, errorCorrectionLevel: 'M' })
    if (token !== serial) return
    qrImage.value = image
    status.value = '使用手机 App 扫码'
    expiryTimer = setTimeout(refreshQr, expiresIn * 1000)

    async function poll() {
      try {
        const result = await invoke('poll_qr', { platform: props.platform.id, key })
        if (token !== serial || showResult(result)) return
        error.value = ''
        if (result.status === 'expired') {
          refreshQr()
          return
        }
        status.value = result.status === 'scanned' ? '已扫码，请在手机上确认' : '使用手机 App 扫码'
      } catch (reason) {
        if (token !== serial) return
        error.value = String(reason)
      }
      pollTimer = setTimeout(poll, 2500)
    }
    pollTimer = setTimeout(poll, 1800)
  } catch (reason) {
    if (token !== serial) return
    status.value = '二维码获取失败'
    error.value = String(reason)
  }
}

watch(mode, next => {
  resetPolling()
  error.value = ''
  secret.value = ''
  if (isBili.value) invoke('close_bili_login').catch(() => {})
  if (next === 'qr') refreshQr()
})

onMounted(() => {
  if (mode.value === 'qr') refreshQr()
})

onBeforeUnmount(() => {
  resetPolling()
  clearInterval(cooldownTimer)
  if (isBili.value) invoke('close_bili_login').catch(() => {})
})

async function run(action) {
  if (pending.value) return
  const token = serial
  pending.value = true
  error.value = ''
  try {
    await action(token)
  } catch (reason) {
    if (token === serial) error.value = String(reason)
  } finally {
    pending.value = false
  }
}

function biliLogin() {
  run(async token => {
    await invoke('open_bili_login')
    async function poll() {
      try {
        const result = await invoke('poll_bili_login')
        if (token !== serial || showResult(result) || result.status === 'closed') return
        error.value = ''
      } catch (reason) {
        if (token !== serial) return
        error.value = String(reason)
      }
      pollTimer = setTimeout(poll, 1800)
    }
    clearTimeout(pollTimer)
    if (token === serial) pollTimer = setTimeout(poll, 1200)
  })
}

function submitNetease() {
  run(async () => {
    const payload = {
      method: mode.value, account: account.value, country: country.value, secret: secret.value,
    }
    const result = await invoke('netease_login', { payload })
    smsDialog.value?.close()
    showResult(result)
  })
}

function sendCode() {
  if (cooldown.value) {
    smsDialog.value?.showModal()
    return
  }
  run(async () => {
    await invoke('netease_sms', { phone: account.value.trim(), country: country.value.trim().replace(/^\+/, '') })
    cooldown.value = 60
    clearInterval(cooldownTimer)
    cooldownTimer = setInterval(() => {
      cooldown.value -= 1
      if (!cooldown.value) clearInterval(cooldownTimer)
    }, 1000)
    smsDialog.value?.showModal()
  })
}

function importCookies() {
  run(async () => {
    const path = await open({ multiple: false, filters: [{ name: 'Cookie 文本文件', extensions: ['txt'] }] })
    if (!path) return
    showResult(await invoke('import_youtube', { path, label: youtubeLabel.value }))
  })
}
</script>

<template>
  <section class="login-page">
    <button type="button" class="text-button back-button" @click="emit('back')"><ArrowLeft :size="17" /> 返回</button>
    <div class="login-content">
      <PlatformIcon :platform="platform.id" />
      <h1>{{ platform.name }} 登录</h1>
      <template v-if="mode === 'qr'">
        <div class="qr-frame" :aria-busy="!qrImage && !error">
          <img v-if="qrImage" :src="qrImage" width="212" height="212" :alt="`${platform.name} 登录二维码`" />
          <LoaderCircle v-else-if="!error" class="spin" :size="34" />
          <ScanLine v-else :size="34" />
        </div>
        <div class="qr-status">
          <span role="status">{{ status }}</span>
          <button class="icon-button" type="button" aria-label="刷新二维码" title="刷新二维码" @click="refreshQr"><RefreshCw :size="16" /></button>
        </div>
      </template>
      <div v-else-if="mode === 'cookies'" class="login-fields">
        <ol class="youtube-login-guide muted">
          <li>在 Chrome 或 Edge 安装
            <button type="button" class="youtube-extension-link" :disabled="pending"
              @click="run(() => invoke('open_youtube_cookie_extension'))">Get cookies.txt LOCALLY <ExternalLink :size="13" /></button>。
          </li>
          <li>在同一个浏览器登录 YouTube，停留在 YouTube 页面。打开扩展，选择 <strong>Netscape</strong>，点击 <strong>Export</strong> 导出当前网站的 Cookie。</li>
          <li>回到这里点「导入 Cookie」，选择刚导出的 .txt 文件。</li>
        </ol>
        <label class="field">账号名称（可选）<input v-model="youtubeLabel" maxlength="80" placeholder="用于识别账号" /></label>
        <div class="login-actions">
          <button type="button" class="button secondary" @click="run(() => invoke('open_youtube'))"><ExternalLink :size="16" /> 打开 YouTube</button>
          <button type="button" class="button primary" :disabled="pending" @click="importCookies"><FileUp :size="17" /> {{ pending ? '正在验证' : '导入 Cookie' }}</button>
        </div>
      </div>
      <div v-else-if="isBili" class="login-fields">
        <p class="muted">在 Bilibili 官方窗口选择{{ mode === 'sms' ? '短信登录' : '密码登录' }}。完成登录后将自动返回。</p>
        <button type="button" class="button primary" :disabled="pending" @click="biliLogin"><ExternalLink :size="16" /> 打开登录窗口</button>
      </div>
      <form v-else class="login-fields" @submit.prevent="mode === 'sms' ? sendCode() : submitNetease()">
        <label v-if="mode === 'password'" class="field">账号<input v-model="account" required autocomplete="username" placeholder="邮箱或手机号码" /></label>
        <div v-if="mode === 'sms' || (account && !account.includes('@'))" class="phone-row">
          <label class="field">区号<input v-model="country" required inputmode="numeric" maxlength="5" /></label>
          <label v-if="mode === 'sms'" class="field">手机号码<input v-model="account" required autocomplete="tel-national" inputmode="tel" /></label>
        </div>
        <label v-if="mode === 'password'" class="field">密码<input v-model="secret" required type="password" autocomplete="current-password" /></label>
        <button type="submit" class="button primary" :disabled="pending">{{ mode === 'password' ? '登录' : cooldown ? '输入验证码' : '发送验证码' }}</button>
      </form>
      <p v-if="error && !smsDialog?.open" class="inline-error" role="alert">{{ error }}</p>
      <div v-if="mode !== 'cookies'" class="login-options" aria-label="登录方式">
        <button v-for="[id, label] in [['qr', '扫码登录'], ['password', '账号密码'], ['sms', '手机号登录']]" :key="id"
          type="button" class="text-button" :class="{ active: mode === id }" :aria-pressed="mode === id"
          :disabled="pending" @click="mode = id">{{ label }}</button>
      </div>
    </div>

    <dialog ref="smsDialog" class="sms-dialog" @close="secret = ''; error = ''">
      <form @submit.prevent="submitNetease">
        <h2>输入短信验证码</h2>
        <p class="muted">验证码已发送至 {{ account }}。</p>
        <label class="field">验证码<input v-model="secret" required inputmode="numeric" autocomplete="one-time-code" maxlength="8" autofocus /></label>
        <p v-if="error" class="inline-error" role="alert">{{ error }}</p>
        <p v-if="cooldown" class="muted">{{ cooldown }} 秒后可以重新获取</p>
        <button v-else type="button" class="text-button" :disabled="pending" @click="sendCode">重新发送</button>
        <div class="login-actions">
          <button type="button" class="button secondary" @click="smsDialog.close()">取消</button>
          <button type="submit" class="button primary" :disabled="pending">确认登录</button>
        </div>
      </form>
    </dialog>
  </section>
</template>

<style scoped>
.youtube-login-guide { display: grid; gap: 8px; margin: 0; padding-inline-start: 1.45em; line-height: 1.55; }
.youtube-extension-link { display: inline-flex; align-items: center; gap: 4px; padding: 0; border: 0; color: var(--accent-text); background: transparent; text-decoration: underline; text-underline-offset: 2px; }
.youtube-extension-link:hover { text-decoration-thickness: 2px; }
</style>
