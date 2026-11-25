import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { execFileSync } from 'node:child_process'

export function optionsFromArgs(args, allowed) {
  const result = {}
  for (let i = 0; i < args.length; i++) {
    const key = args[i]
    if (!key.startsWith('--') || !allowed.includes(key.slice(2))) {
      throw new Error(`Unknown argument: ${key}`)
    }
    const name = key.slice(2)
    if (Object.hasOwn(result, name)) throw new Error(`Duplicate argument: ${key}`)
    const value = args[++i]
    if (value === undefined || value.startsWith('--')) throw new Error(`Missing value for ${key}`)
    result[name] = value
  }
  return result
}

export function integerOption(value, name, minimum) {
  if (!/^\d+$/.test(value)) throw new Error(`${name} must be an integer >= ${minimum}`)
  const result = Number(value)
  if (!Number.isSafeInteger(result) || result < minimum) {
    throw new Error(`${name} must be an integer >= ${minimum}`)
  }
  return result
}

export function summarize(samples) {
  if (!samples.length || samples.some((sample) => !Number.isFinite(sample) || sample < 0)) {
    throw new Error('Timing samples must contain finite, non-negative values')
  }
  const sorted = [...samples].sort((a, b) => a - b)
  const middle = Math.floor(sorted.length / 2)
  return {
    samplesMs: samples,
    medianMs: sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2,
    p95Ms: sorted[Math.ceil(sorted.length * 0.95) - 1],
  }
}

export function sha256(data) {
  return createHash('sha256').update(data).digest('hex')
}

export function fileHash(path) {
  return sha256(readFileSync(path))
}

export function gitInfo(directory) {
  const git = (...args) => execFileSync('git', ['-C', directory, ...args], { encoding: 'utf8' }).trim()
  return {
    commit: git('rev-parse', 'HEAD'),
    trackedChanges: git('status', '--porcelain=v1', '--untracked-files=no'),
  }
}
