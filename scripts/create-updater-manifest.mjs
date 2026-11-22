import { copyFileSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const root = join(import.meta.dirname, '..')
const config = JSON.parse(readFileSync(join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'))
const tag = process.env.RELEASE_TAG
const repository = process.env.GITHUB_REPOSITORY || 'hikarin31768/beatmaps-starter'
if (!tag) throw new Error('RELEASE_TAG is required to create the updater manifest.')

const bundleDir = join(root, 'src-tauri', 'target', 'release', 'bundle', 'nsis')
const installers = readdirSync(bundleDir)
  .filter(name => name.endsWith('-setup.exe'))
if (installers.length !== 1) {
  throw new Error(`Expected one NSIS installer, found ${installers.length}.`)
}

const installerPath = join(bundleDir, installers[0])
const signaturePath = `${installerPath}.sig`
const signature = readFileSync(signaturePath, 'utf8').trim()
if (!signature) throw new Error(`Updater signature is empty: ${signaturePath}`)

const assetsDir = join(root, 'src-tauri', 'target', 'release-assets')
const installer = `beatmaps-starter_${config.version}_x64-setup.exe`
mkdirSync(assetsDir, { recursive: true })
copyFileSync(installerPath, join(assetsDir, installer))
copyFileSync(signaturePath, join(assetsDir, `${installer}.sig`))

const url = `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${installer}`
const manifest = {
  version: config.version,
  notes: `Beatmaps Starter ${config.version}`,
  pub_date: new Date().toISOString(),
  platforms: {
    'windows-x86_64': { signature, url },
  },
}

writeFileSync(join(assetsDir, 'latest.json'), `${JSON.stringify(manifest, null, 2)}\n`)
