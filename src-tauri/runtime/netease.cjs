const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const write = process.stdout.write.bind(process.stdout)

// Keep the SDK's diagnostic output away from the command's JSON reply.
for (const method of ['log', 'debug', 'info', 'warn', 'error']) console[method] = () => {}

const bitrates = { standard: 128000, higher: 192000, exhigh: 320000, lossless: 999000 }

async function prepareXeapi() {
  const keyFile = path.join(os.tmpdir(), 'xeapi_public_key')
  const stat = fs.statSync(keyFile, { throwIfNoEntry: false })
  if (stat && Date.now() - stat.mtimeMs < 24 * 60 * 60 * 1000) return true
  const { getXeapiPublicKey } = require('@neteasecloudmusicapienhanced/api/util/xeapiKey')
  try {
    const currentKey = stat ? JSON.parse(fs.readFileSync(keyFile, 'utf8')) : {}
    // Key registration in the SDK has no timeout. Keep playback and legacy lookup responsive.
    const publicKey = await Promise.race([
      getXeapiPublicKey(currentKey),
      new Promise((_, reject) => setTimeout(() => reject(new Error('Key registration timed out')), 5500)),
    ])
    const pending = `${keyFile}.${process.pid}`
    fs.writeFileSync(pending, JSON.stringify(publicKey))
    fs.renameSync(pending, keyFile)
    return true
  } catch {
    return Boolean(stat)
  }
}

async function request(handler, params) {
  try {
    const { cookieToJson } = require('@neteasecloudmusicapienhanced/api/util/index.js')
    const cookies = typeof params.cookie === 'string' ? cookieToJson(params.cookie) : params.cookie || {}
    const send = (...args) => require('@neteasecloudmusicapienhanced/api/util/request.js')(...args)
    const { body, cookie } = await handler({ ...params, cookie: cookies, timeout: 9000 }, send)
    return { body, cookie }
  } catch (error) {
    return { body: error?.body || { code: 503 } }
  }
}

async function main() {
  const operations = {
    song_detail: require('@neteasecloudmusicapienhanced/api/module/song_detail.js'),
    song_chorus: require('@neteasecloudmusicapienhanced/api/module/song_chorus.js'),
    song_url_v1: require('@neteasecloudmusicapienhanced/api/module/song_url_v1.js'),
    login_qr_key: require('@neteasecloudmusicapienhanced/api/module/login_qr_key.js'),
    login_qr_create: require('@neteasecloudmusicapienhanced/api/module/login_qr_create.js'),
    login_qr_check: require('@neteasecloudmusicapienhanced/api/module/login_qr_check.js'),
    user_account: require('@neteasecloudmusicapienhanced/api/module/user_account.js'),
    login: require('@neteasecloudmusicapienhanced/api/module/login.js'),
    login_cellphone: require('@neteasecloudmusicapienhanced/api/module/login_cellphone.js'),
    captcha_sent: require('@neteasecloudmusicapienhanced/api/module/captcha_sent.js'),
  }
  const legacySongUrl = require('@neteasecloudmusicapienhanced/api/module/song_url.js')
  const calls = JSON.parse(fs.readFileSync(0, 'utf8'))
  // The SDK's request module reads this cache on first use.
  fs.closeSync(fs.openSync(path.join(os.tmpdir(), 'anonymous_token'), 'a'))
  const hasTrack = calls.some(call => call.operation === 'song_url_v1')
  const xeapi = hasTrack ? prepareXeapi() : null
  return Promise.all(calls.map(async ({ operation, params }) => {
    if (!Object.hasOwn(operations, operation)) throw new Error('Unsupported operation')
    const handler = operations[operation]
    if (operation !== 'song_url_v1') return request(handler, params)
    const primary = await xeapi ? await request(handler, params) : { body: { code: 503 } }
    const track = primary.body?.data?.[0]
    const bitrate = bitrates[params.level]
    if (!bitrate || (track?.url && track.level === params.level && track.freeTrialInfo == null)) return primary
    const fallback = await request(legacySongUrl, { id: params.id, cookie: params.cookie, br: bitrate })
    if (fallback.body?.data?.[0]?.url) {
      return fallback
    }
    return primary
  }))
}

main().then(
  result => write(JSON.stringify(result), () => process.exit(0)),
  error => write(JSON.stringify({ error: error?.code === 'MODULE_NOT_FOUND' ? 'dependency' : 'runtime' }), () => process.exit(1)),
)
