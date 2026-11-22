import { readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { basename, join } from 'node:path'

const root = process.cwd()
const config = JSON.parse(readFileSync(join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'))
const tag = process.env.RELEASE_TAG
const repository = process.env.GITHUB_REPOSITORY || 'hikarin31768/beatmaps-starter'
if (!tag) throw new Error('RELEASE_TAG is required to create the updater manifest.')

const bundleDir = join(root, 'src-tauri', 'target', 'release', 'bundle', 'nsis')
const updaterArchives = readdirSync(bundleDir)
  .filter(name => name.endsWith('.nsis.zip'))
if (updaterArchives.length !== 1) {
  throw new Error(`Expected one signed NSIS updater archive, found ${updaterArchives.length}.`)
}

const archive = updaterArchives[0]
const signaturePath = join(bundleDir, `${archive}.sig`)
const signature = readFileSync(signaturePath, 'utf8').trim()
if (!signature) throw new Error(`Updater signature is empty: ${signaturePath}`)

const url = `https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(basename(archive))}`
const manifest = {
  version: config.version,
  notes: `Beatmaps Starter ${config.version}`,
  pub_date: new Date().toISOString(),
  platforms: {
    'windows-x86_64': { signature, url },
  },
}

writeFileSync(join(root, 'latest.json'), `${JSON.stringify(manifest, null, 2)}\n`)
