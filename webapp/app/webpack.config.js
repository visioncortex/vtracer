const path = require('path');

module.exports = {
  entry: "./bootstrap.js",
  output: {
    path: path.resolve(__dirname, "dist"),
    filename: "bootstrap.js",
    clean: true,
  },
  mode: "development",
  // wasm-pack's `bundler` target emits ESM imports of the .wasm module; webpack
  // 5 handles those natively once this experiment is on.
  experiments: {
    asyncWebAssembly: true,
  },
  devServer: {
    //host: "0.0.0.0",
    port: 8080,
  }
};
