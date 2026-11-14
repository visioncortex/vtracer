import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { test } from 'node:test'
import { integerOption, optionsFromArgs, sha256, summarize } from './bench-utils.mjs'

test('parses options and rejects malformed or duplicated arguments', () => {
  assert.deepEqual(optionsFromArgs(['--runs', '5', '--output', 'result.json'], ['runs', 'output']), {
    runs: '5',
    output: 'result.json',
  })
  assert.throws(() => optionsFromArgs(['--other', 'x'], ['runs']), /Unknown argument/)
  assert.throws(() => optionsFromArgs(['--runs'], ['runs']), /Missing value/)
  assert.throws(() => optionsFromArgs(['--runs', '1', '--runs', '2'], ['runs']), /Duplicate/)
  assert.equal(integerOption('5', '--runs', 1), 5)
  assert.equal(integerOption('0', '--warmup', 0), 0)
  assert.throws(() => integerOption('0', '--runs', 1), /integer/)
  assert.throws(() => integerOption('1.5', '--runs', 1), /integer/)
  assert.throws(() => integerOption('9007199254740992', '--runs', 1), /integer/)
})

test('reports median and nearest-rank p95 without modifying the input', () => {
  const samples = [5, 1, 4, 2, 3]
  assert.deepEqual(summarize(samples), { samplesMs: samples, medianMs: 3, p95Ms: 5 })
  assert.equal(summarize([4, 2]).medianMs, 3)
  assert.throws(() => summarize([]), /Timing samples/)
  assert.throws(() => summarize([NaN]), /Timing samples/)
  assert.throws(() => summarize([-1]), /Timing samples/)
  assert.equal(sha256('abc'), 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad')
})

test('benchmark CLI rejects missing and invalid options', () => {
  const script = fileURLToPath(new URL('./bench-wasm.mjs', import.meta.url))
  const missing = spawnSync(process.execPath, [script], { encoding: 'utf8' })
  assert.equal(missing.status, 1)
  assert.match(missing.stderr, /--build is required/)
  const invalid = spawnSync(process.execPath, [
    script, '--build', 'unused', '--app', 'unused', '--output', 'unused', '--workload', 'wrong',
  ], { encoding: 'utf8' })
  assert.equal(invalid.status, 1)
  assert.match(invalid.stderr, /--workload must be all, engine, or auto/)
})
