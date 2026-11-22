import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync, readFileSync, realpathSync, renameSync, writeFileSync } from 'node:fs'
import { basename, dirname, join, resolve } from 'node:path'

const root = resolve(import.meta.dirname, '..')
const runtime = join(root, 'src-tauri', 'runtime')
const notices = join(runtime, 'licenses')
const base = 'https://github.com/yt-dlp/yt-dlp/releases/latest/download'

function run(program, args, options = {}) {
  const result = spawnSync(program, args, { cwd: root, encoding: 'utf8', stdio: 'inherit', ...options })
  if (result.error || result.status !== 0) {
    throw new Error(`${basename(program)} 运行失败`, { cause: result.error })
  }
  return result.stdout?.trim()
}

function capture(program, args) {
  return run(program, args, { stdio: ['ignore', 'pipe', 'pipe'] })
}

function fetch(url, output) {
  run('curl.exe', ['--fail', '--location', '--silent', '--show-error', '--connect-timeout', '15',
    '--max-time', '180', '--retry', '3', '--retry-all-errors', url, '-o', output])
}

if (process.platform !== 'win32' || process.arch !== 'x64' || Number(process.versions.node.split('.')[0]) < 24) {
  throw new Error('请在 Windows x64 上使用 Node.js 24 或更新版本打包。')
}

const ffmpegTarget = join(runtime, 'ffmpeg.exe')
const ffmpegSource = process.env.BEATMAPS_FFMPEG || ffmpegTarget
if (!existsSync(ffmpegSource)) {
  throw new Error('请先用 scripts/build-ffmpeg.sh 构建音频版 FFmpeg。')
}
const ffmpeg = realpathSync(ffmpegSource)
const ffmpegInfo = capture(ffmpeg, ['-version'])
if (!process.env.BEATMAPS_FFMPEG && !ffmpegInfo.includes('-beatmaps-starter-audio')) {
  throw new Error('请先用 scripts/build-ffmpeg.sh 生成项目的音频版 FFmpeg。')
}
if (!capture(ffmpeg, ['-hide_banner', '-h', 'encoder=libvorbis']).includes('Encoder libvorbis')
  || !capture(ffmpeg, ['-hide_banner', '-h', 'encoder=flac']).includes('Encoder flac')
  || !capture(ffmpeg, ['-hide_banner', '-h', 'filter=adelay']).includes('Filter adelay')
  || !capture(ffmpeg, ['-hide_banner', '-h', 'filter=ebur128']).includes('Filter ebur128')) {
  throw new Error('请提供支持 Vorbis、FLAC、音频偏移和 ebur128 的 FFmpeg Windows 版本。')
}
const ffprobeTarget = join(runtime, 'ffprobe.exe')
const ffprobeSource = process.env.BEATMAPS_FFPROBE || ffprobeTarget
if (!existsSync(ffprobeSource)) throw new Error('请先用 scripts/build-ffmpeg.sh 构建 FFprobe。')
const ffprobe = realpathSync(ffprobeSource)
capture(ffprobe, ['-version'])
if (ffprobe.toLowerCase() !== ffprobeTarget.toLowerCase()) copyFileSync(ffprobe, ffprobeTarget)

mkdirSync(notices, { recursive: true })
run(process.execPath, [process.env.npm_execpath, '--prefix', runtime, 'ci', '--omit=dev', '--ignore-scripts', '--no-fund', '--no-audit'])
run(process.execPath, [join(root, 'scripts/bundle-netease.mjs')])
copyFileSync(process.execPath, join(runtime, 'node.exe'))
fetch(`https://raw.githubusercontent.com/nodejs/node/v${process.versions.node}/LICENSE`, join(notices, 'Node.js.txt'))

if (ffmpeg.toLowerCase() !== ffmpegTarget.toLowerCase()) copyFileSync(ffmpeg, ffmpegTarget)
const ffmpegLicense = [join(dirname(ffmpeg), 'LICENSE'), join(dirname(ffmpeg), '..', 'LICENSE')].find(existsSync)
writeFileSync(join(notices, 'FFmpeg-build.txt'), ffmpegInfo + '\n\n' + capture(ffmpegTarget, ['-L']) + '\n')
if (ffmpegLicense) copyFileSync(ffmpegLicense, join(notices, 'FFmpeg-LICENSE.txt'))
copyFileSync(join(root, 'TEMPLATE-LICENSE'), join(notices, 'Tauri-Vue-Template.txt'))

const checksumFile = join(runtime, 'yt-dlp-sha256.txt')
fetch(`${base}/SHA2-256SUMS`, checksumFile)
const expected = readFileSync(checksumFile, 'utf8').match(/^([a-f\d]{64})\s+\*?yt-dlp\.exe\s*$/im)?.[1]?.toLowerCase()
if (!expected) throw new Error('无法读取 yt-dlp 发布校验和。')
const digest = file => createHash('sha256').update(readFileSync(file)).digest('hex')
const ytDlp = join(runtime, 'yt-dlp.exe')
if (!existsSync(ytDlp) || digest(ytDlp) !== expected) {
  const download = join(runtime, 'yt-dlp.partial')
  fetch(`${base}/yt-dlp.exe`, download)
  if (digest(download) !== expected) throw new Error('yt-dlp 发布校验和不匹配。')
  renameSync(download, ytDlp)
}
const ytDlpVersion = capture(ytDlp, ['--version'])
if (!/^\d{4}\.\d{2}\.\d{2}(?:\.\d+)?$/.test(ytDlpVersion)) {
  throw new Error('无法识别 yt-dlp 发布版本。')
}
fetch(`https://raw.githubusercontent.com/yt-dlp/yt-dlp/${ytDlpVersion}/LICENSE`, join(notices, 'yt-dlp.txt'))
writeFileSync(join(notices, 'Upstream.txt'), [
  'FFmpeg: https://ffmpeg.org/download.html',
  'libogg and libvorbis: https://xiph.org/downloads/',
  'ArrowVortex tempo detection reference: https://github.com/uvcat7/ArrowVortex',
  'aubio: https://github.com/aubio/aubio',
  `yt-dlp: https://github.com/yt-dlp/yt-dlp/releases/tag/${ytDlpVersion}`,
  'Node.js: https://nodejs.org/download/release/',
  'NeteaseCloudMusicApiEnhanced: https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced',
].join('\n') + '\n')

const config = join(runtime, 'bundle.windows.json')
const updatePubkey = process.env.BEATMAPS_UPDATE_PUBKEY?.trim()
const updaterEnabled = Boolean(process.env.BEATMAPS_UPDATE_ENDPOINT?.trim() && updatePubkey)
writeFileSync(config, JSON.stringify({
  ...(updaterEnabled && { plugins: { updater: { pubkey: updatePubkey } } }),
  bundle: {
    active: true,
    createUpdaterArtifacts: updaterEnabled,
    resources: {
      'runtime/netease.bundle.js': 'runtime/netease.cjs',
      'runtime/xhr-sync-worker.js': 'runtime/xhr-sync-worker.js',
      'runtime/china_ip_ranges.txt': 'data/china_ip_ranges.txt',
      'runtime/node.exe': 'runtime/node.exe',
      'runtime/ffmpeg.exe': 'runtime/ffmpeg.exe',
      'runtime/ffprobe.exe': 'runtime/ffprobe.exe',
      'runtime/yt-dlp.exe': 'runtime/yt-dlp.exe',
      'runtime/licenses/': 'runtime/licenses/',
    },
  },
}))
run(process.execPath, [join(root, 'node_modules/@tauri-apps/cli/tauri.js'), 'build', '--bundles', 'nsis', '--config', config])
