import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { delimiter, dirname, join, resolve } from 'node:path'
import { execFileSync, spawnSync } from 'node:child_process'
import { fileHash, gitInfo, optionsFromArgs } from './bench-utils.mjs'

function run(command, args, cwd, env) {
  const result = spawnSync(command, args, { cwd, env, stdio: 'inherit' })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`${command} failed with status ${result.status}`)
}

function capture(command, args, cwd, env) {
  return execFileSync(command, args, { cwd, env, encoding: 'utf8' }).trim()
}

function samePath(a, b) {
  const left = resolve(a)
  const right = resolve(b)
  return process.platform === 'win32' ? left.toLowerCase() === right.toLowerCase() : left === right
}

function prepareManifest(source, destination, vtracerPath, visioncortexPath) {
  const contents = readFileSync(source, 'utf8')
  const matches = contents.match(/^vtracer\s*=\s*[^\r\n]+$/gm) ?? []
  if (matches.length !== 1) throw new Error(`Expected one vtracer dependency in ${source}`)
  if (visioncortexPath && /^\[patch\.crates-io\]/m.test(contents)) {
    throw new Error(`Existing crates.io patch requires manual review: ${source}`)
  }
  let manifest = contents.replace(matches[0], `vtracer = { path = ${JSON.stringify(vtracerPath)} }`)
  if (visioncortexPath) {
    manifest += `\n[patch.crates-io]\nvisioncortex = { path = ${JSON.stringify(visioncortexPath)} }\n`
  }
  writeFileSync(destination, manifest, { flag: 'wx' })
}

function prepareCrate({ source, output, vtracerPath, visioncortexPath, lockfile, appAdapter }) {
  mkdirSync(join(output, 'src'), { recursive: true })
  prepareManifest(join(source, 'Cargo.toml'), join(output, 'Cargo.toml'), vtracerPath, visioncortexPath)
  copyFileSync(join(source, 'src', 'lib.rs'), join(output, 'src', 'lib.rs'))
  if (lockfile && existsSync(lockfile)) copyFileSync(lockfile, join(output, 'Cargo.lock'))
  if (!appAdapter) copyFileSync(join(source, 'index.js'), join(output, 'index.js'))
}

function resolveDependencies(directory, cargo, env, visioncortexPath) {
  capture(cargo, ['metadata', '--format-version', '1', '--no-deps'], directory, env)
  const version = visioncortexPath
    ? JSON.parse(capture(cargo, ['metadata', '--format-version', '1', '--no-deps',
      '--manifest-path', join(visioncortexPath, 'Cargo.toml')], directory, env))
      .packages.find((entry) => entry.name === 'visioncortex')?.version
    : '0.9.3'
  if (!version) throw new Error('Could not determine the VisionCortex fork version')
  run(cargo, ['update', '-p', 'visioncortex', '--precise', version, '--offline'], directory, env)
  const metadata = JSON.parse(capture(cargo, ['metadata', '--format-version', '1', '--locked'], directory, env))
  const matches = metadata.packages.filter((entry) => entry.name === 'visioncortex')
  if (matches.length !== 1) throw new Error(`Expected exactly one VisionCortex dependency in ${directory}`)
  const vision = matches[0]
  if (visioncortexPath) {
    if (vision.source !== null || !samePath(vision.manifest_path, join(visioncortexPath, 'Cargo.toml'))) {
      throw new Error(`Fork build resolved an unexpected VisionCortex: ${vision.manifest_path}`)
    }
  } else if (vision.version !== '0.9.3' || !vision.source?.startsWith('registry+')) {
    throw new Error(`Expected published VisionCortex 0.9.3, got ${vision.version} from ${vision.source}`)
  }
  const vcsPath = join(dirname(vision.manifest_path), '.cargo_vcs_info.json')
  const sourceCommit = existsSync(vcsPath) ? JSON.parse(readFileSync(vcsPath, 'utf8')).git?.sha1 : null
  if (!visioncortexPath && sourceCommit !== '0062088c89645aac76c00e066deb7e8f53980dd7') {
    throw new Error(`Unexpected published VisionCortex source revision: ${sourceCommit}`)
  }
  return { version: vision.version, source: vision.source, manifestPath: vision.manifest_path, sourceCommit }
}

function requireClean(info, label, expected) {
  if (info.trackedChanges) throw new Error(`${label} has tracked changes; benchmark a pinned clean checkout`)
  if (expected && info.commit !== expected) throw new Error(`${label} is ${info.commit}, expected ${expected}`)
}

function main() {
  const options = optionsFromArgs(process.argv.slice(2), [
    'checkout', 'app', 'output', 'visioncortex', 'expected-commit', 'visioncortex-commit',
    'wasm-pack', 'cargo', 'target-dir',
  ])
  for (const name of ['checkout', 'app', 'output']) {
    if (!options[name]) throw new Error(`--${name} is required`)
  }
  const checkout = resolve(options.checkout)
  const app = resolve(options.app)
  const output = resolve(options.output)
  const visioncortex = options.visioncortex ? resolve(options.visioncortex) : null
  const cargo = options.cargo ?? 'cargo'
  const wasmPack = options['wasm-pack'] ?? 'wasm-pack'
  const checkoutInfo = gitInfo(checkout)
  const engineChanges = capture('git', [
    'status', '--porcelain=v1', '--untracked-files=normal', '--',
    'Cargo.toml', 'Cargo.lock', 'crates/vtracer', 'nodejs',
  ], checkout, process.env)
  requireClean({ ...checkoutInfo, trackedChanges: engineChanges }, 'VTracer engine', options['expected-commit'])
  checkoutInfo.engineChanges = engineChanges
  const visionInfo = visioncortex ? gitInfo(visioncortex) : null
  if (visionInfo) requireClean(visionInfo, 'VisionCortex checkout', options['visioncortex-commit'])
  if (options['visioncortex-commit'] && !visioncortex) throw new Error('--visioncortex-commit requires --visioncortex')
  if (existsSync(output)) throw new Error(`Output already exists: ${output}`)

  const env = { ...process.env }
  if (options['target-dir']) env.CARGO_TARGET_DIR = resolve(options['target-dir'])
  mkdirSync(dirname(output), { recursive: true })
  mkdirSync(output)
  const node = join(output, 'node')
  const browser = join(output, 'browser')
  const vtracerPath = join(checkout, 'crates', 'vtracer')
  const nodeSource = join(checkout, 'nodejs')
  const browserSource = join(app, 'wasm', 'vtracer')
  prepareCrate({
    source: nodeSource, output: node, vtracerPath, visioncortexPath: visioncortex,
    lockfile: existsSync(join(nodeSource, 'Cargo.lock')) ? join(nodeSource, 'Cargo.lock') : join(checkout, 'Cargo.lock'),
    appAdapter: false,
  })
  prepareCrate({
    source: browserSource, output: browser, vtracerPath, visioncortexPath: visioncortex,
    lockfile: join(browserSource, 'Cargo.lock'), appAdapter: true,
  })

  const nodeVision = resolveDependencies(node, cargo, env, visioncortex)
  const browserVision = resolveDependencies(browser, cargo, env, visioncortex)
  const nodeLock = fileHash(join(node, 'Cargo.lock'))
  const browserLock = fileHash(join(browser, 'Cargo.lock'))
  run(wasmPack, ['build', '--target', 'nodejs', '--out-dir', 'pkg'], node, env)
  run(wasmPack, ['build', '--target', 'web', '--out-dir', 'pkg', '--out-name', 'vtracer1', '--release'], browser, env)
  if (nodeLock !== fileHash(join(node, 'Cargo.lock')) || browserLock !== fileHash(join(browser, 'Cargo.lock'))) {
    throw new Error('Cargo.lock changed during the build; the resolved dependency record is invalid')
  }
  const provenance = {
    checkout: checkoutInfo,
    visioncortex: visionInfo,
    nodeVisioncortex: nodeVision,
    browserVisioncortex: browserVision,
    app: gitInfo(app),
    appAdapterSourceSha256: fileHash(join(browserSource, 'src', 'lib.rs')),
    appAdapterManifestSha256: fileHash(join(browserSource, 'Cargo.toml')),
    nodeWrapperSourceSha256: fileHash(join(nodeSource, 'src', 'lib.rs')),
    nodeIndexSha256: fileHash(join(nodeSource, 'index.js')),
    nodeLockSha256: nodeLock,
    browserLockSha256: browserLock,
    nodeWasmSha256: fileHash(join(node, 'pkg', 'vtracer_wasm_bg.wasm')),
    nodeGlueSha256: fileHash(join(node, 'pkg', 'vtracer_wasm.js')),
    browserWasmSha256: fileHash(join(browser, 'pkg', 'vtracer1_bg.wasm')),
    browserGlueSha256: fileHash(join(browser, 'pkg', 'vtracer1.js')),
    wasmPackVersion: capture(wasmPack, ['--version'], output, env),
    wasmOpt: (() => {
      const binary = (env.PATH ?? env.Path ?? '').split(delimiter)
        .map((directory) => join(directory, process.platform === 'win32' ? 'wasm-opt.exe' : 'wasm-opt'))
        .find((path) => existsSync(path))
      return binary
        ? { version: capture(binary, ['--version'], output, env), sha256: fileHash(binary) }
        : { source: 'wasm-pack managed; version not recorded' }
    })(),
    cargoVersion: capture(cargo, ['--version'], output, env),
    rustcVersion: capture('rustc', ['-vV'], output, env),
    nodeBuild: ['wasm-pack', 'build', '--target', 'nodejs', '--out-dir', 'pkg'],
    browserBuild: ['wasm-pack', 'build', '--target', 'web', '--out-dir', 'pkg', '--out-name', 'vtracer1', '--release'],
  }
  writeFileSync(join(output, 'provenance.json'), `${JSON.stringify(provenance, null, 2)}\n`, { flag: 'wx' })
  console.log(`Built benchmark artifacts: ${output}`)
}

try {
  main()
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error))
  process.exitCode = 1
}
