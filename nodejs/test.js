'use strict';
const assert = require('assert');
const fs = require('fs');
const path = require('path');
const vtracer = require('./index.js');

const SAMPLE = path.join(__dirname, '..', 'docs', 'assets', 'samples', 'tank-unit-preview.png');
const data = fs.readFileSync(SAMPLE);

// encoded bytes, default options
let svg = vtracer.convertBuffer(data);
assert(svg.includes('<svg') && svg.includes('<path'), 'default convertBuffer');
console.log('convertBuffer default:', (svg.match(/<path/g) || []).length, 'paths');

// options: bw preset -> all black
svg = vtracer.convertBuffer(data, { colorMode: 'bw' });
assert(svg.includes('fill="#000000"'), 'bw produces black');
console.log('convertBuffer bw:', (svg.match(/<path/g) || []).length, 'paths');

// options: mosaic + polygon + palette
svg = vtracer.convertBuffer(data, { hierarchical: 'cutout', mode: 'polygon', palette: ['#000000', '#ffffff'], optimize: 2 });
assert(svg.includes('<svg'), 'mosaic+palette');
console.log('convertBuffer cutout/polygon/palette:', (svg.match(/<path/g) || []).length, 'paths');

// preset
svg = vtracer.convertBuffer(data, { preset: 'poster' });
console.log('convertBuffer poster:', (svg.match(/<path/g) || []).length, 'paths');

// raw pixels: 20x20, left red / right blue
const w = 20, h = 20;
const rgba = Buffer.alloc(w * h * 4);
for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
  const i = (y * w + x) * 4;
  const [r, g, b] = x < w / 2 ? [220, 40, 40] : [40, 40, 220];
  rgba[i] = r; rgba[i + 1] = g; rgba[i + 2] = b; rgba[i + 3] = 255;
}
svg = vtracer.convertPixels(rgba, w, h);
assert(svg.includes('<svg'), 'convertPixels');
console.log('convertPixels:', (svg.match(/<path/g) || []).length, 'paths');

// file I/O
const out = path.join(require('os').tmpdir(), 'vtracer_node_out.svg');
vtracer.convertFileSync(SAMPLE, out, { mode: 'spline' });
assert(fs.statSync(out).size > 0, 'convertFileSync wrote file');
console.log('convertFileSync wrote:', fs.statSync(out).size, 'bytes');

// error handling
assert.throws(() => vtracer.convertBuffer(data, { palette: ['nope'] }), /rrggbb/, 'bad palette rejected');
assert.throws(() => vtracer.convertPixels(Buffer.alloc(8), 10, 10), /rgba length/, 'bad pixel length rejected');
console.log('errors rejected OK');

console.log('ALL OK');
