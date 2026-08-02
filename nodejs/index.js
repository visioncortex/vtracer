'use strict';

// Node package: image decoding + vectorization happen in wasm (no native
// dependency); this layer only adds file I/O and a camelCase API.

const fs = require('fs');
const fsp = require('fs/promises');
const wasm = require('./pkg/vtracer_wasm.js');

/**
 * Vectorize an encoded image (PNG/JPEG/GIF/BMP) Buffer/Uint8Array to an SVG string.
 * @param {Uint8Array} buffer
 * @param {object} [options]
 * @returns {string}
 */
function convertBuffer(buffer, options = {}) {
  return wasm.vectorize_bytes(buffer, options);
}

/**
 * Vectorize a raw RGBA8 buffer (width*height*4 bytes) to an SVG string.
 * @param {Uint8Array} rgba
 * @param {number} width
 * @param {number} height
 * @param {object} [options]
 * @returns {string}
 */
function convertPixels(rgba, width, height, options = {}) {
  return wasm.vectorize_rgba(rgba, width, height, options);
}

/**
 * Read an image file, vectorize it, and write the SVG to disk.
 * @returns {Promise<void>}
 */
async function convertFile(inputPath, outputPath, options = {}) {
  const data = await fsp.readFile(inputPath);
  const svg = wasm.vectorize_bytes(data, options);
  await fsp.writeFile(outputPath, svg);
}

/** Synchronous {@link convertFile}. */
function convertFileSync(inputPath, outputPath, options = {}) {
  const data = fs.readFileSync(inputPath);
  const svg = wasm.vectorize_bytes(data, options);
  fs.writeFileSync(outputPath, svg);
}

module.exports = { convertBuffer, convertPixels, convertFile, convertFileSync };
