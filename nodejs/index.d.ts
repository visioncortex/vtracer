/** Conversion options. Any field may be omitted; omitted fields use the framework default. */
export interface Options {
  /** Applied before other fields: "bw" | "poster" | "photo". */
  preset?: 'bw' | 'poster' | 'photo';
  /** Region forming: hierarchical color clustering (default), binary threshold, or watershed. */
  clustering?: 'color-cluster' | 'bw' | 'watershed';
  hierarchical?: 'stacked' | 'cutout';
  mode?: 'pixel' | 'polygon' | 'spline';
  filterSpeckle?: number;
  colorPrecision?: number;
  layerDifference?: number;
  cornerThreshold?: number;
  lengthThreshold?: number;
  maxIterations?: number;
  spliceThreshold?: number;
  /** Curve simplification tolerance in px (omit = off; try 1-2.5). */
  simplify?: number;
  pathPrecision?: number;
  /** Fixed palette: `#rrggbb` strings. */
  palette?: string[];
  /** Auto-quantize target color count. */
  maxColors?: number;
  /** 0 = off, 1 = quantize+simplify, 2 = + shorthands/grouping. */
  optimize?: number;
  /** Binary mode (`clustering: 'bw'`): fixed threshold 0..=255; foreground when intensity is below it. */
  binaryThreshold?: number;
  /** Binary mode: use Bradley–Roth adaptive thresholding (handles uneven lighting). */
  adaptive?: boolean;
  /** Adaptive window side length in px; 0 = auto (~1/8 of the shorter side). */
  adaptiveWindow?: number;
  /** Adaptive sensitivity: percent below the local mean (default 15). */
  adaptiveT?: number;
  /** Watershed clustering: hierarchy cut level (default 128; higher = more regions, uncapped). */
  watershedDetail?: number;
}

/** Vectorize an encoded image (PNG/JPEG/GIF/BMP) buffer to an SVG string. */
export function convertBuffer(buffer: Uint8Array, options?: Options): string;

/** Vectorize a raw RGBA8 buffer (`width * height * 4` bytes) to an SVG string. */
export function convertPixels(rgba: Uint8Array, width: number, height: number, options?: Options): string;

/** Read an image file, vectorize it, and write the SVG to disk. */
export function convertFile(inputPath: string, outputPath: string, options?: Options): Promise<void>;

/** Synchronous {@link convertFile}. */
export function convertFileSync(inputPath: string, outputPath: string, options?: Options): void;
