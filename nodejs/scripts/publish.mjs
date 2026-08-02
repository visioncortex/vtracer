#!/usr/bin/env node
// Build the wasm package and publish it, by default to a local npm registry
// (e.g. a Verdaccio instance at http://localhost:4873).
//
//   node scripts/publish.mjs                       # publish to the local registry
//   node scripts/publish.mjs --dry-run             # build + pack, don't publish
//   node scripts/publish.mjs --registry=http://... # override the registry
//   NPM_REGISTRY=http://... node scripts/publish.mjs
//
// The registry may also be given via the NPM_REGISTRY env var.

import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const pkgDir = resolve(dirname(fileURLToPath(import.meta.url)), '..');

const args = process.argv.slice(2);
const dryRun = args.includes('--dry-run');
const regArg = args.find((a) => a.startsWith('--registry='));
const registry =
  (regArg && regArg.slice('--registry='.length)) ||
  process.env.NPM_REGISTRY ||
  'http://localhost:4873';

function run(cmd, cmdArgs) {
  console.log(`\n$ ${cmd} ${cmdArgs.join(' ')}`);
  execFileSync(cmd, cmdArgs, { stdio: 'inherit', cwd: pkgDir });
}

// 1. Fresh wasm build (regenerates pkg/).
run('wasm-pack', ['build', '--target', 'nodejs', '--out-dir', 'pkg']);

// 2. Sanity check before publishing.
run('node', ['test.js']);

// 3. Publish (or dry-run) to the chosen registry.
const publishArgs = ['publish', '--registry', registry];
if (dryRun) publishArgs.push('--dry-run');
run('npm', publishArgs);

console.log(`\n✔ ${dryRun ? 'dry-run for' : 'published to'} ${registry}`);
