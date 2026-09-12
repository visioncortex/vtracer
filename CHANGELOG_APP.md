# VTracer App Changelog

All notable changes to the VTracer desktop app will be documented in this file.

## 1.0.0-alpha.4 - Build 161 - 2026-09-12

### Added

* 简体中文 for UI display language.

### Added (Gen AI)

* Generation on your own sd-server
* Inked, an art style for SDXL-Turbo

## 1.0.0-alpha.4 - Build 151 - 2026-09-09

### Added

* An model manager to install/uninstall VTracer 2 engine versions
* Factory Reset under Settings > Application. It removes everything VTracer 2 and Gen AI keep on the device

### Fixed

* Trial activation issues

## 1.0.0-alpha.4 - Build 143 - 2026-09-08

### Added (Gen AI)

* The FLUX model family, alongside the Stable Diffusion models
* Art styles for the FLUX models
* Starring a round in the left panel, each round also shows the model
* Gradient Step is now determined dynamically, picking the optimal value per artwork

## 1.0.0-alpha.4 - Build 133 - 2026-09-07

### Added

* Gen AI preview. Supports 2 Stable Diffusion model lines, SD Turbo and SDXL Turbo. Requires Vulkan support on Windows. Requires an active VT2 license.
* The app now offer VTracer 2 engine update.

### Changed

* VTracer 2 composites with Stacked rather than Cutout by default.
* An expired VTracer 2 trial no longer locks the images you have already traced. They can still be reopened and exported, and only new tracing needs an active licence.

### Fixed

* Retrying VTracer 2 setup after an interrupted install no longer asks for a second device seat for the same machine.
* A trace with its background removed now sits on a light checkerboard in dark mode.

## 1.0.0-alpha.4 - Build 116 - 2026-08-29

### Added

* VTracer 2 preview. Trial license can be activated in app.
* SVG saving without a dialog. A preferred save folder can be set in Preferences, or the app can keep asking each time.
* A source image display preference, choosing smooth or pixelated scaling for the source image.
* Selected SVG shapes now show their bounds, and can be dragged around the canvas to inspect what sits beneath them.
* A configurable disk cache limit for VTracer 2, defaulting to 2 GB. Lowering it reclaims space immediately.

### Fixed

* Pasting an image now works consistently across the app rather than only when the canvas held focus.

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
