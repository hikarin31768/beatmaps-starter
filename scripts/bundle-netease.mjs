import { build } from 'esbuild'
import { isBuiltin } from 'node:module'
import { copyFileSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'

const root = resolve(import.meta.dirname, '..')
const runtime = join(root, 'src-tauri', 'runtime')
const modules = join(runtime, 'node_modules')
const notices = join(runtime, 'licenses')
const optionalAddons = ['canvas', 'bufferutil', 'utf-8-validate']

const { metafile } = await build({
  absWorkingDir: root,
  entryPoints: {
    'netease.bundle': join(runtime, 'netease.cjs'),
    'xhr-sync-worker': join(modules, 'jsdom/lib/jsdom/living/xhr/xhr-sync-worker.js'),
  },
  outdir: runtime,
  bundle: true,
  platform: 'node',
  format: 'cjs',
  target: 'node24',
  minify: true,
  legalComments: 'eof',
  metafile: true,
  external: optionalAddons,
  plugins: [{
    name: 'jsdom-sync-worker',
    setup(builder) {
      builder.onResolve({ filter: /^\.\/xhr-sync-worker\.js$/ }, args => {
        // jsdom starts this entry in a separate process and resolves it next to the helper.
        if (args.kind === 'require-resolve') return { path: args.path, external: true }
      })
    },
  }],
})

for (const output of Object.values(metafile.outputs)) {
  const missing = output.imports.find(item => item.external && !isBuiltin(item.path)
    && !optionalAddons.includes(item.path) && item.path !== './xhr-sync-worker.js')
  if (missing) throw new Error(`网易云助手仍依赖外部文件 ${missing.path}`)
}

const bundledPackages = new Set()
for (const file of Object.keys(metafile.inputs)) {
  const parts = file.split('/')
  const index = parts.lastIndexOf('node_modules') + 1
  if (index === 0) continue
  const end = index + (parts[index].startsWith('@') ? 2 : 1)
  bundledPackages.add(resolve(root, ...parts.slice(0, end)))
}

const licenseSections = []
for (const folder of [...bundledPackages].sort()) {
  const { name, version, license } = JSON.parse(readFileSync(join(folder, 'package.json'), 'utf8'))
  const files = readdirSync(folder, { withFileTypes: true })
    .filter(file => file.isFile() && /^(licen[cs]e|copying|notice|copyright)(?:[._-]|$)/i.test(file.name))
    .map(file => readFileSync(join(folder, file.name), 'utf8').trim())
  licenseSections.push([`${name}@${version} (${license ?? '见许可文本'})`, ...files].join('\n\n'))
}
mkdirSync(notices, { recursive: true })
writeFileSync(join(notices, 'Netease-Dependencies.txt'), licenseSections.join('\n\n' + '-'.repeat(60) + '\n\n') + '\n')

// The SDK resolves this file relative to the installed runtime directory.
copyFileSync(join(modules, '@neteasecloudmusicapienhanced/api/data/china_ip_ranges.txt'), join(runtime, 'china_ip_ranges.txt'))
const bytes = ['netease.bundle.js', 'xhr-sync-worker.js'].reduce((sum, name) => sum + statSync(join(runtime, name)).size, 0)
console.log(`网易云运行依赖已合并为两个脚本（${(bytes / 1024 / 1024).toFixed(2)} MiB）和一个数据文件。`)
