import init, { resetCalls, getCalls, vectorize_rgba } from 'virtual:vtracer-bench'
import { analyze } from '/src/lib/engine/analyze.ts'
import { computeMetrics } from '/src/lib/engine/core.ts'
import { presetById } from '/src/lib/engine/presets.ts'
import { vectorize } from '/src/lib/engine/vectorize.ts'
import { vtracer1Backend } from '/src/lib/engine/vtracer1-backend.ts'

const cache = new Map()

async function image(index, width, height) {
  if (!cache.has(index)) {
    const response = await fetch(`/__bench/rgba/${index}`)
    if (!response.ok) throw new Error(`RGBA input ${index}: HTTP ${response.status}`)
    const data = new Uint8ClampedArray(await response.arrayBuffer())
    if (data.length !== width * height * 4) throw new Error(`Invalid RGBA input ${index}`)
    cache.set(index, { data, width, height })
  }
  return cache.get(index)
}

function heap() {
  return performance.memory ? {
    usedJSHeapSize: performance.memory.usedJSHeapSize,
    totalJSHeapSize: performance.memory.totalJSHeapSize,
  } : null
}

async function measure(index, width, height, svg) {
  const src = await image(index, width, height)
  const rendered = await vtracer1Backend.rasterize(svg, width, height)
  return computeMetrics(src, rendered, svg)
}

window.vtracerBenchmark = {
  async initialize() {
    const started = performance.now()
    await init()
    return { wasmLoadMs: performance.now() - started, heap: heap(), userAgent: navigator.userAgent }
  },

  async raw(index, width, height, options, warmup, runs) {
    const src = await image(index, width, height)
    const bytes = new Uint8Array(src.data.buffer, src.data.byteOffset, src.data.byteLength)
    let svg
    for (let i = 0; i < warmup; i++) svg = vectorize_rgba(bytes, width, height, options)
    const samples = []
    const before = heap()
    for (let i = 0; i < runs; i++) {
      const started = performance.now()
      svg = vectorize_rgba(bytes, width, height, options)
      samples.push(performance.now() - started)
    }
    const after = heap()
    return { samples, metrics: await measure(index, width, height, svg), heapBefore: before, heapAfter: after }
  },

  async auto(index, width, height, warmup, runs) {
    const src = await image(index, width, height)
    const settings = {
      presetId: 'auto',
      params: { ...presetById('auto').params },
      auto: { enabled: true, threshold: 3, maxKB: 400 },
    }
    const trace = async () => {
      resetCalls()
      const started = performance.now()
      const analysis = analyze(src)
      const analysisMs = performance.now() - started
      const result = await vectorize(src, settings, vtracer1Backend, { analysis })
      const ms = performance.now() - started
      const chosen = result.trials[result.chosen]
      return {
        ms,
        analysisMs,
        analysis,
        engineCalls: getCalls(),
        trialCount: result.trials.length,
        chosen: result.chosen,
        autoTuned: result.autoTuned,
        failedTrials: result.failedTrials,
        params: chosen.params,
        metrics: chosen.metrics,
      }
    }
    for (let i = 0; i < warmup; i++) await trace()
    const before = heap()
    const observations = []
    for (let i = 0; i < runs; i++) observations.push(await trace())
    return { observations, heapBefore: before, heapAfter: heap() }
  },

  measure,
}
