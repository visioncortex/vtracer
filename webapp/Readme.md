<div align="center">
  <img src="https://github.com/visioncortex/vtracer/raw/master/docs/images/visioncortex-banner.png">
</div>

# visioncortex VTracer

A web app to convert raster images into vector graphics.

## Setup

0. `sudo apt install git build-essential`
1. https://www.rust-lang.org/tools/install
2. https://rustwasm.github.io/wasm-pack/installer/
3. https://github.com/nvm-sh/nvm
```
nvm install --lts
```

## Getting Started

0. Setup
```
cd app
npm install
```

1. Build wasm
```
wasm-pack build
```

2. Start development server
```
cd app
npm run start
```
Open browser on http://localhost:8080/

3. Release
```
npm run build
```