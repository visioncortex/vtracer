import { spawnSync } from 'node:child_process'
import { cpus, platform, release, tmpdir, totalmem } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { performance } from 'node:perf_hooks'
import { PNG } from 'pngjs'
import { fileHash, integerOption, optionsFromArgs, sha256, summarize } from './bench-utils.mjs'

const configArgs = [
  '--clustering', 'color-cluster', '--hierarchical', 'stacked', '--mode', 'spline',
  '--filter-speckle', '6', '--color-precision', '8', '--gradient-step', '8',
  '--corner-threshold', '60', '--segment-length', '4', '--splice-threshold', '45',
  '--path-precision', '2', '--optimize', '1',
]

function run(executable, args, label) {
  const result = spawnSync(executable, args, { encoding: 'utf8', windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'] })
  if (result.error) throw result.error
  if (result.status !== 0) {
    throw new Error(`${label} exited ${result.status}: ${(result.stderr ?? '').trim()}`)
  }
}

function version(executable, label) {
  const result = spawnSync(executable, ['--version'], { encoding: 'utf8', windowsHide: true })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`${label} --version exited ${result.status}`)
  return (result.stdout ?? '').trim()
}

function readImages(directory, requested) {
  const available = readdirSync(directory).filter((name) => name.toLowerCase().endsWith('.png')).sort()
  const names = requested ? requested.split(',') : available
  if (!names.length || names.some((name) => !available.includes(name))) {
    throw new Error(`Requested PNGs must exist directly in ${directory}`)
  }
  if (new Set(names).size !== names.length) throw new Error('Duplicate image name')
  return names.map((name) => {
    const bytes = readFileSync(join(directory, name))
    const decoded = PNG.sync.read(bytes)
    return {
      name, width: decoded.width, height: decoded.height,
      encodedBytes: bytes.length, sha256: sha256(bytes),
    }
  })
}

function execute(executable, input, output, label) {
  const args = ['--input', input, '--output', output, ...configArgs]
  const started = performance.now()
  run(executable, args, label)
  return performance.now() - started
}

function main() {
  const options = optionsFromArgs(process.argv.slice(2), [
    'baseline', 'baseline-commit', 'optimized', 'optimized-commit', 'assets', 'output', 'warmup', 'runs', 'images',
  ])
  for (const name of ['baseline', 'baseline-commit', 'optimized', 'optimized-commit', 'assets', 'output']) {
    if (!options[name]) throw new Error(`--${name} is required`)
  }
  const baseline = resolve(options.baseline)
  const optimized = resolve(options.optimized)
  const assets = resolve(options.assets)
  const output = resolve(options.output)
  for (const [label, executable] of [['baseline', baseline], ['optimized', optimized]]) {
    if (!existsSync(executable)) throw new Error(`Missing ${label} executable: ${executable}`)
  }
  if (existsSync(output)) throw new Error(`Output already exists: ${output}`)
  const warmup = integerOption(options.warmup ?? '3', '--warmup', 0)
  const runs = integerOption(options.runs ?? '7', '--runs', 1)
  const images = readImages(assets, options.images)
  const temp = mkdtempSync(join(tmpdir(), 'vtracer-native-bench-'))
  mkdirSync(dirname(output), { recursive: true })
  const report = {
    schemaVersion: 1,
    status: 'in_progress',
    createdAt: new Date().toISOString(),
    settings: {
      warmup, runs, configArgs,
      timing: 'CLI process startup, PNG decode, tracing, SVG serialization, and output write are included',
      order: 'baseline then optimized for even sample indices; optimized then baseline for odd indices',
      maxIterations: 'CLI default (10); no CLI override is available',
      percentile: 'nearest-rank p95; with seven samples this is the maximum',
    },
    environment: {
      platform: platform(), osRelease: release(), cpu: cpus()[0]?.model,
      logicalCpus: cpus().length, totalMemoryBytes: totalmem(), node: process.version,
    },
    binaries: {
      baseline: { path: baseline, commit: options['baseline-commit'], version: version(baseline, 'baseline'), sha256: fileHash(baseline) },
      optimized: { path: optimized, commit: options['optimized-commit'], version: version(optimized, 'optimized'), sha256: fileHash(optimized) },
    },
    aggregate: null,
    results: [],
  }
  const save = (flag = 'w') => writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`, { flag })

  try {
    save('wx')
    for (const [index, image] of images.entries()) {
      console.log(`[${index + 1}/${images.length}] ${image.name}`)
      const input = join(assets, image.name)
      const baselineOutput = join(temp, `${index}-baseline.svg`)
      const optimizedOutput = join(temp, `${index}-optimized.svg`)
      for (let i = 0; i < warmup; i++) {
        const order = i % 2 === 0
          ? [[baseline, baselineOutput, 'baseline'], [optimized, optimizedOutput, 'optimized']]
          : [[optimized, optimizedOutput, 'optimized'], [baseline, baselineOutput, 'baseline']]
        for (const [exe, svg, label] of order) execute(exe, input, svg, label)
      }
      const baselineSamples = []
      const optimizedSamples = []
      for (let i = 0; i < runs; i++) {
        const order = i % 2 === 0
          ? [[baseline, baselineOutput, 'baseline', baselineSamples], [optimized, optimizedOutput, 'optimized', optimizedSamples]]
          : [[optimized, optimizedOutput, 'optimized', optimizedSamples], [baseline, baselineOutput, 'baseline', baselineSamples]]
        for (const [exe, svg, label, samples] of order) samples.push(execute(exe, input, svg, label))
      }
      const baselineTiming = summarize(baselineSamples)
      const optimizedTiming = summarize(optimizedSamples)
      const result = {
        image,
        baseline: {
          timing: baselineTiming,
          svgBytes: readFileSync(baselineOutput).length,
          svgSha256: fileHash(baselineOutput),
        },
        optimized: {
          timing: optimizedTiming,
          svgBytes: readFileSync(optimizedOutput).length,
          svgSha256: fileHash(optimizedOutput),
        },
        outputBytesEqual: readFileSync(baselineOutput).length === readFileSync(optimizedOutput).length,
        outputSha256Equal: fileHash(baselineOutput) === fileHash(optimizedOutput),
        speedup: baselineTiming.medianMs / optimizedTiming.medianMs,
        improvementPercent: 100 * (1 - optimizedTiming.medianMs / baselineTiming.medianMs),
      }
      report.results.push(result)
      save()
    }
    const speedups = report.results.map((result) => result.speedup)
    const geometricMeanSpeedup = Math.exp(speedups.reduce((sum, value) => sum + Math.log(value), 0) / speedups.length)
    report.aggregate = {
      imagesImproved: report.results.filter((result) => result.speedup > 1).length,
      imagesTotal: report.results.length,
      geometricMeanSpeedup,
      geometricMeanImprovementPercent: 100 * (1 - 1 / geometricMeanSpeedup),
      outputHashesEqual: report.results.filter((result) => result.outputSha256Equal).length,
    }
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
    rmSync(temp, { recursive: true, force: true })
  }
}

try {
  main()
} catch (error) {
  console.error(error instanceof Error ? error.stack : String(error))
  process.exitCode = 1
}
