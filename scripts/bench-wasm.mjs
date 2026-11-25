import { createServer as createHttpServer } from 'node:http'
import { createRequire } from 'node:module'
import { cpus, platform, release, tmpdir, totalmem } from 'node:os'
import { dirname, join, relative, resolve } from 'node:path'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright-core'
import { PNG } from 'pngjs'
import ts from 'typescript'
import { fileHash, gitInfo, integerOption, optionsFromArgs, sha256, summarize } from './bench-utils.mjs'

const scriptDir = dirname(fileURLToPath(import.meta.url))
const engineOptions = {
  clustering: 'color-cluster',
  hierarchical: 'stacked',
  mode: 'spline',
  filterSpeckle: 6,
  colorPrecision: 8,
  layerDifference: 8,
  cornerThreshold: 60,
  lengthThreshold: 4,
  maxIterations: 10,
  spliceThreshold: 45,
  pathPrecision: 2,
  optimize: 1,
}

function sourceHashes(root) {
  const hashes = new Map()
  function visit(path) {
    if (hashes.has(path)) return
    const source = readFileSync(path, 'utf8')
    hashes.set(path, sha256(source))
    for (const imported of ts.preProcessFile(source, true, true).importedFiles) {
      if (!imported.fileName.startsWith('.')) continue
      if (imported.fileName === './vtracer1-wasm/vtracer1.js') continue
      const resolved = ts.resolveModuleName(imported.fileName, path, {
        allowJs: true, moduleResolution: ts.ModuleResolutionKind.Bundler,
      }, ts.sys).resolvedModule
      if (!resolved) throw new Error(`Could not resolve ${imported.fileName} from ${path}`)
      visit(resolve(resolved.resolvedFileName))
    }
  }
  for (const name of ['analyze.ts', 'core.ts', 'presets.ts', 'vectorize.ts', 'vtracer1-backend.ts']) {
    visit(join(root, 'src', 'lib', 'engine', name))
  }
  for (const file of [
    join('wasm', 'vtracer', 'Cargo.toml'), join('wasm', 'vtracer', 'src', 'lib.rs'),
    'package-lock.json', 'tsconfig.app.json',
  ]) {
    const path = resolve(root, file)
    if (!existsSync(path)) throw new Error(`Missing application file: ${path}`)
    hashes.set(path, fileHash(path))
  }
  return Object.fromEntries([...hashes].map(([path, hash]) => [relative(root, path), hash])
    .sort(([a], [b]) => a.localeCompare(b)))
}

function decodeImages(directory, requested) {
  const available = readdirSync(directory).filter((name) => name.toLowerCase().endsWith('.png')).sort()
  const names = requested ? requested.split(',') : available
  if (!names.length || names.some((name) => !available.includes(name))) {
    throw new Error(`Requested PNGs must exist directly in ${directory}`)
  }
  if (new Set(names).size !== names.length) throw new Error('Duplicate image name')
  return names.map((name) => {
    const encoded = readFileSync(join(directory, name))
    const decoded = PNG.sync.read(encoded)
    if (decoded.data.length !== decoded.width * decoded.height * 4) throw new Error(`Invalid RGBA: ${name}`)
    return {
      name, encoded, data: decoded.data, width: decoded.width, height: decoded.height,
      encodedSha256: sha256(encoded), rgbaSha256: sha256(decoded.data),
    }
  })
}

function runNode(fn, warmup, runs) {
  for (let i = 0; i < warmup; i++) fn()
  const samples = []
  const hashes = []
  const before = process.memoryUsage()
  let svg
  for (let i = 0; i < runs; i++) {
    const started = performance.now()
    svg = fn()
    samples.push(performance.now() - started)
    if (typeof svg !== 'string' || !svg.includes('<svg')) throw new Error('WASM returned an invalid SVG')
    hashes.push(sha256(svg))
  }
  return { timing: summarize(samples), svg, svgSha256ByRun: hashes, memoryBefore: before, memoryAfter: process.memoryUsage() }
}

function normalizeFs(path) {
  return `/@fs/${resolve(path).replaceAll('\\', '/')}`
}

async function browserServer(app, build, images) {
  const appLock = JSON.parse(readFileSync(join(app, 'package-lock.json'), 'utf8'))
  for (const name of ['vite', 'typescript', '@tsconfig/svelte']) {
    const required = appLock.packages?.[`node_modules/${name}`]?.version
    const local = JSON.parse(readFileSync(join(scriptDir, 'node_modules', ...name.split('/'), 'package.json'), 'utf8')).version
    if (!required || local !== required) {
      throw new Error(`Application locks ${name} ${required ?? '(unknown)'}; benchmark has ${local}`)
    }
  }
  const parseConfig = (path) => {
    const parsed = ts.parseConfigFileTextToJson(path, readFileSync(path, 'utf8'))
    if (parsed.error) throw new Error(ts.flattenDiagnosticMessageText(parsed.error.messageText, '\n'))
    return parsed.config
  }
  const appConfig = parseConfig(join(app, 'tsconfig.app.json'))
  if (appConfig.extends !== '@tsconfig/svelte/tsconfig.json') {
    throw new Error('Unsupported application TypeScript configuration inheritance')
  }
  const baseConfig = parseConfig(join(scriptDir, 'node_modules', '@tsconfig', 'svelte', 'tsconfig.json'))
  if (baseConfig.extends) throw new Error('Unsupported nested base TypeScript configuration')
  const tsconfigRaw = { compilerOptions: { ...baseConfig.compilerOptions, ...appConfig.compilerOptions } }
  const { createServer } = await import('vite')
  const glue = join(build, 'browser', 'pkg', 'vtracer1.js')
  const id = '\0vtracer-benchmark-engine'
  const plugin = {
    name: 'vtracer-benchmark-engine',
    enforce: 'pre',
    resolveId(source, importer) {
      if (source === 'virtual:vtracer-bench' ||
          (source === './vtracer1-wasm/vtracer1.js' &&
           importer?.replaceAll('\\', '/').split('?')[0].endsWith('/src/lib/engine/vtracer1-backend.ts'))) return id
    },
    load(source) {
      if (source !== id) return
      return `import init, { vectorize_rgba as trace } from ${JSON.stringify(normalizeFs(glue))};
let calls = 0;
export default init;
export function vectorize_rgba(...args) { calls++; return trace(...args); }
export function resetCalls() { calls = 0; }
export function getCalls() { return calls; }`
    },
  }
  const cacheDirectory = mkdtempSync(join(tmpdir(), 'vtracer-bench-'))
  let vite
  let http
  const close = async () => {
    try {
      if (http?.listening) await new Promise((accept, reject) => http.close((error) => error ? reject(error) : accept()))
    } finally {
      try {
        if (vite) await vite.close()
      } finally {
        rmSync(cacheDirectory, { recursive: true, force: true })
      }
    }
  }
  try {
    vite = await createServer({
      configFile: false, root: app, cacheDir: cacheDirectory, logLevel: 'error', plugins: [plugin],
      server: { middlewareMode: true, fs: { allow: [app, scriptDir, build] } },
      optimizeDeps: { noDiscovery: true, include: [] },
      esbuild: { tsconfigRaw },
      appType: 'custom',
    })
    http = createHttpServer((request, response) => {
      const url = new URL(request.url, 'http://127.0.0.1')
      if (url.pathname === '/__bench') {
        response.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' })
        response.end(`<!doctype html><meta charset="utf-8"><script type="module" src="${normalizeFs(join(scriptDir, 'bench-wasm-browser.mjs'))}"></script>`)
        return
      }
      const match = /^\/__bench\/rgba\/(\d+)$/.exec(url.pathname)
      if (match) {
        const input = images[Number(match[1])]
        if (!input) { response.writeHead(404); response.end(); return }
        response.writeHead(200, { 'content-type': 'application/octet-stream', 'cache-control': 'no-store' })
        response.end(input.data)
        return
      }
      vite.middlewares(request, response)
    })
    await new Promise((accept, reject) => {
      http.once('error', reject)
      http.listen(0, '127.0.0.1', accept)
    })
    return { url: `http://127.0.0.1:${http.address().port}/__bench`, close }
  } catch (error) {
    await close()
    throw error
  }
}

async function main() {
  const options = optionsFromArgs(process.argv.slice(2), [
    'build', 'app', 'assets', 'output', 'warmup', 'runs', 'images', 'workload', 'browser',
  ])
  for (const name of ['build', 'app', 'output']) if (!options[name]) throw new Error(`--${name} is required`)
  const build = resolve(options.build)
  const app = resolve(options.app)
  const output = resolve(options.output)
  const warmup = integerOption(options.warmup ?? '3', '--warmup', 0)
  const runs = integerOption(options.runs ?? '5', '--runs', 1)
  const workload = options.workload ?? 'all'
  if (!['all', 'engine', 'auto'].includes(workload)) throw new Error('--workload must be all, engine, or auto')
  const executablePath = options.browser ?? process.env.CHROME_PATH ??
    (process.platform === 'win32' ? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe' : '')
  if (!executablePath || !existsSync(executablePath)) throw new Error('Set --browser or CHROME_PATH to an installed Chrome executable')
  const provenance = JSON.parse(readFileSync(join(build, 'provenance.json'), 'utf8'))
  for (const [file, hash] of [
    [join('node', 'pkg', 'vtracer_wasm_bg.wasm'), provenance.nodeWasmSha256],
    [join('node', 'pkg', 'vtracer_wasm.js'), provenance.nodeGlueSha256],
    [join('browser', 'pkg', 'vtracer1_bg.wasm'), provenance.browserWasmSha256],
    [join('browser', 'pkg', 'vtracer1.js'), provenance.browserGlueSha256],
    [join('node', 'index.js'), provenance.nodeIndexSha256],
  ]) {
    if (fileHash(resolve(build, file)) !== hash) throw new Error(`Build artifact changed: ${file}`)
  }
  if (fileHash(join(app, 'wasm', 'vtracer', 'src', 'lib.rs')) !== provenance.appAdapterSourceSha256 ||
      fileHash(join(app, 'wasm', 'vtracer', 'Cargo.toml')) !== provenance.appAdapterManifestSha256) {
    throw new Error('The application WASM adapter changed after the build')
  }
  const hashes = sourceHashes(app)
  const appInfo = gitInfo(app)
  const images = decodeImages(resolve(options.assets ?? join(app, 'assets')), options.images)
  const report = {
    schemaVersion: 1, status: 'in_progress', createdAt: new Date().toISOString(),
    settings: {
      warmup, runs, workload, engineOptions, auto: { presetId: 'auto', enabled: true, threshold: 3, maxKB: 400 },
      percentile: 'nearest-rank; with five samples p95 is the maximum',
      input: 'PNG decoded once with pngjs to shared RGBA8 before timing',
      autoTiming: 'application analysis plus vectorize with that analysis; image decoding is excluded',
      memory: 'Node process memory and Chromium JS heap observations are not peak WASM memory',
    },
    environment: {
      platform: platform(), release: release(), cpu: cpus()[0]?.model, logicalCpus: cpus().length,
      totalMemoryBytes: totalmem(), node: process.version, browserMode: 'headless Chromium',
    },
    provenance,
    benchmarkSourceSha256: Object.fromEntries(
      ['bench-wasm.mjs', 'bench-wasm-browser.mjs', 'bench-utils.mjs', 'build-bench-wasm.mjs', 'package-lock.json']
        .map((file) => [file, fileHash(join(scriptDir, file))]),
    ),
    application: { ...appInfo, engineSourceSha256: hashes }, results: [],
  }
  mkdirSync(dirname(output), { recursive: true })
  const save = (flag = 'w') => writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag })
  save('wx')
  let server
  let browser
  const cleanup = async () => {
    try {
      if (browser) {
        const instance = browser
        browser = undefined
        await instance.close()
      }
    } finally {
      if (server) {
        const instance = server
        server = undefined
        await instance.close()
      }
    }
  }
  try {
    const require = createRequire(import.meta.url)
    const moduleStarted = performance.now()
    const engine = require(join(build, 'node', 'index.js'))
    report.environment.nodeWasmLoadMs = performance.now() - moduleStarted
    server = await browserServer(app, build, images)
    browser = await chromium.launch({ executablePath, headless: true, args: ['--disable-background-networking'] })
    report.environment.browserVersion = browser.version()
    const page = await browser.newPage()
    page.on('pageerror', (error) => console.error(`Browser error: ${error.message}`))
    await page.goto(server.url, { timeout: 120_000 })
    await page.waitForFunction(() => Boolean(window.vtracerBenchmark), null, { timeout: 120_000 })
    report.environment.browserWasm = await page.evaluate(() => window.vtracerBenchmark.initialize())
    save()

    for (const [index, input] of images.entries()) {
      console.log(`[${index + 1}/${images.length}] ${input.name}`)
      const result = {
        image: {
          name: input.name, width: input.width, height: input.height,
          encodedBytes: input.encoded.length, encodedSha256: input.encodedSha256, rgbaSha256: input.rgbaSha256,
        },
      }
      if (workload !== 'auto') {
        const pixels = runNode(() => engine.convertPixels(input.data, input.width, input.height, engineOptions), warmup, runs)
        const buffer = runNode(() => engine.convertBuffer(input.encoded, engineOptions), warmup, runs)
        pixels.metrics = await page.evaluate(({ i, w, h, svg }) => window.vtracerBenchmark.measure(i, w, h, svg), {
          i: index, w: input.width, h: input.height, svg: pixels.svg,
        })
        buffer.metrics = await page.evaluate(({ i, w, h, svg }) => window.vtracerBenchmark.measure(i, w, h, svg), {
          i: index, w: input.width, h: input.height, svg: buffer.svg,
        })
        delete pixels.svg
        delete buffer.svg
        const raw = await page.evaluate(({ i, w, h, params, warm, n }) =>
          window.vtracerBenchmark.raw(i, w, h, params, warm, n), {
          i: index, w: input.width, h: input.height, params: engineOptions, warm: warmup, n: runs,
        })
        result.engine = { nodePixels: pixels, nodeBuffer: buffer, browserPixels: { ...raw, timing: summarize(raw.samples) } }
        delete result.engine.browserPixels.samples
      }
      if (workload !== 'engine') {
        const auto = await page.evaluate(({ i, w, h, warm, n }) =>
          window.vtracerBenchmark.auto(i, w, h, warm, n), {
          i: index, w: input.width, h: input.height, warm: warmup, n: runs,
        })
        result.auto = {
          ...auto,
          timing: summarize(auto.observations.map((observation) => observation.ms)),
          analysisTiming: summarize(auto.observations.map((observation) => observation.analysisMs)),
        }
      }
      if (JSON.stringify(sourceHashes(app)) !== JSON.stringify(hashes)) {
        throw new Error('Application engine sources changed while benchmarking')
      }
      report.results.push(result)
      save()
    }
    await cleanup()
    report.status = 'complete'
    report.completedAt = new Date().toISOString()
    save()
    console.log(`Results written to ${output}`)
  } catch (error) {
    report.status = 'failed'
    report.error = error instanceof Error ? error.message : String(error)
    save()
    throw error
  } finally {
    await cleanup()
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.stack : String(error))
  process.exitCode = 1
})
