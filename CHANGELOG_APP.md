# VTracer App Changelog

All notable changes to the VTracer desktop app will be documented in this file.

## 1.0.0-alpha.3 - Build 59 - 2026-08-14

### Added

* User-managed presets. A preset stores the complete tracing configuration and its source image; presets can be created or updated by name, reordered, deleted, and restored to the shipped defaults.
* Side-by-side comparison mode with synchronized, mirrored views of the source image and generated SVG. Switching between overlay and side-by-side comparison preserves the canvas position and zoom.
* A new etched-cat sample demonstrating adaptive black-and-white tracing.

### Changed

* The canvas comparator and SVG rendering now remain sharp and stable while zooming, panning, dragging the divider, and switching comparison modes.

### Fixed

* Trace sessions recover after an engine panic instead of leaving the app stuck with an unavailable session cache.
* Large images and repeated image uploads no longer leave the frontend unable to start a new trace.

## 1.0.0-alpha.3 - 2026-08-01

First public preview of the rebuilt VTracer desktop app.

### Added

* Native VTracer 1.0 tracing for macOS and Windows, with Linux packaging support.
* Cancellable tracing with stage-aware progress. Changing a tracing control aborts obsolete work and immediately starts the new trace.
* Session caching that reuses expensive clustering work while tuning compatible curve and color fitting settings.
* An interactive source/SVG comparator with a draggable divider, zoom controls, scroll-to-zoom, drag-to-pan, actual-size and fit modes, and a focused full-canvas view.
* SVG shape inspection with yellow hover outlines and selectable curve nodes.
* Full controls for color, black-and-white, and watershed clustering; stacked and seam-free cutout composition; pixel, polygon, and spline fitting; fixed palettes; adaptive thresholding; and curve simplification.
* Open, paste, and drag-and-drop image input, including EXIF orientation normalization for camera images.
* A scrollable sample preset strip with image credits.
* Native SVG save dialogs and save completion feedback.
* System light and dark themes.
* In-app update checks, engine release notes, and open-source license information.

### Changed

* Tracing runs in the native backend rather than WebAssembly.
* The desktop shell serves the frontend and API over an authenticated loopback HTTP session, aligning development and production behavior.

### Fixed

* External links open directly in the default browser on macOS and Windows without displaying a command window.
* Native context menus are suppressed across production app surfaces.
