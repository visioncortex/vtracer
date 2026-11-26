//! Automatic palette selection by interior vote and pooled ink; see
//! [`AutoQuantize`].

use std::array;
use std::collections::{BTreeMap, HashMap};

use visioncortex::Color;

use crate::ir::Segmentation;

use super::oklab::Oklab;
use super::{ColorFitter, FixedPalette};

type Swatch = [u8; 3];
type Votes = BTreeMap<Swatch, usize>;

/// OKLab distance within which two paints count as the same color.
const SAME: f64 = 0.05;
/// Least OKLab contrast between a mark pixel and its flanks.
const MARK_CONTRAST: f64 = 0.03;
/// Furthest OKLab distance at which a mark's ink snaps to a histogram peak.
const INK_SNAP: f64 = 0.15;
/// Least ink coverage, as a fraction of the visible area, to admit an ink.
const INK_SUPPORT: f64 = 0.01;
/// A fill group needs one interior vote per this many visible pixels.
const VOTE_DIVISOR: usize = 20_000;
/// Noise floor, in pixels, under both the vote and the ink thresholds.
const MIN_PIXELS: usize = 4;
/// Largest squared RGB residual off the background-to-ink mixture line for a
/// pixel to count as partly inked.
const BLEND_ERROR: f64 = 3. * 32. * 32.;
/// Histogram side, in bins per channel (8 levels per bin).
const SIDE: usize = 32;
/// Owner of a pixel no layer covers.
const NONE: u32 = u32::MAX;

/// Reduce the layer palette to at most `max_colors` colors the image is
/// actually painted with, then snap each layer to the nearest one in OKLab.
///
/// Averaging buckets of colors (median cut) spends slots on, and invents, the
/// blend colors that anti-aliasing leaves along every edge. This selector votes
/// instead, in two rounds:
///
/// 1. **Interior fills.** Each pixel is resolved to the layer visible there. A
///    pixel votes for its layer's paint only when its whole 3×3 neighbourhood
///    has the same owner, so edge fringes and AA ramps, which are never
///    interior, cannot win a slot. Votes pool per perceptual group (OKLab
///    radius 0.05), and each group is represented by its most-voted observed
///    paint, never an average. A group needs one vote per 20 000 visible
///    pixels (at least 4).
/// 2. **Pooled ink.** Thin marks such as outlines and hairlines have no
///    interior, so the remaining slots go to inks found by cross-section: a
///    pixel whose two flanks agree with each other but contrast with it lies on
///    a mark over one background. Each connected mark's ink is snapped to a
///    peak of the smoothed color histogram, and its support is the ink coverage
///    estimated from the AA ramp. An ink needs 1% of the visible area.
///
/// The palette can come out smaller than `max_colors`: the selector never
/// invents a color to fill a budget. [`palette`](Self::palette) returns it
/// without snapping.
#[derive(Debug, Clone)]
pub struct AutoQuantize {
    pub max_colors: usize,
}

impl Default for AutoQuantize {
    fn default() -> Self {
        Self { max_colors: 16 }
    }
}

impl AutoQuantize {
    /// The palette [`fit`](ColorFitter::fit) snaps `seg` to: at most
    /// `max_colors` paints, interior fills first, then inks by coverage.
    pub fn palette(&self, seg: &Segmentation) -> Vec<Color> {
        if self.max_colors == 0 || seg.width == 0 || seg.height == 0 {
            return Vec::new();
        }
        let canvas = Canvas::new(seg);
        let (visible, interior) = canvas.votes();
        let total: usize = visible.values().sum();
        let fills = fills(&interior, &visible, self.max_colors);
        let peaks: Vec<(Swatch, Oklab)> = histogram_peaks(&visible)
            .into_iter()
            .map(|(_, rgb)| (rgb, lab(rgb)))
            .collect();
        canvas
            .add_inks(fills, &peaks, total, self.max_colors)
            .into_iter()
            .map(|[r, g, b]| Color::new(r, g, b))
            .collect()
    }
}

impl ColorFitter for AutoQuantize {
    fn fit(&self, seg: &mut Segmentation) {
        FixedPalette::new(self.palette(seg)).fit(seg);
    }
}

fn lab([r, g, b]: Swatch) -> Oklab {
    Oklab::from_color(&Color::new(r, g, b))
}

fn near(a: &Oklab, b: &Oklab) -> bool {
    a.distance_squared(b) <= SAME * SAME
}

/// [`lab`], memoized: marks revisit the same few background colors.
fn cached(cache: &mut HashMap<Swatch, Oklab>, rgb: Swatch) -> Oklab {
    *cache.entry(rgb).or_insert_with(|| lab(rgb))
}

/// The visible owner layer of every pixel, plus each layer's paint.
struct Canvas {
    width: usize,
    height: usize,
    /// Topmost layer covering each pixel ([`NONE`] = uncovered).
    owners: Vec<u32>,
    paints: Vec<Swatch>,
    labs: Vec<Oklab>,
}

impl Canvas {
    fn new(seg: &Segmentation) -> Self {
        let width = seg.width as usize;
        let height = seg.height as usize;
        // Resolve occlusion before counting anything: a stacked background can
        // extend under the whole picture, but only owns what still shows.
        let mut owners = vec![NONE; width * height];
        for (id, layer) in seg.layers.iter().enumerate() {
            let mask = &layer.mask;
            let (ox, oy) = (mask.offset.x as usize, mask.offset.y as usize);
            for y in 0..mask.height() {
                for x in 0..mask.width() {
                    if mask.image.get_pixel(x, y) {
                        owners[(oy + y) * width + ox + x] = id as u32;
                    }
                }
            }
        }
        let paints: Vec<Swatch> = seg.layers.iter().map(|l| l.paint.color().rgb_u8()).collect();
        let labs = paints.iter().map(|&rgb| lab(rgb)).collect();
        Self { width, height, owners, paints, labs }
    }

    /// Whether the 3×3 neighbourhood of (x, y), clipped to the canvas, is all
    /// owned by `id`.
    fn is_interior(&self, id: u32, x: usize, y: usize) -> bool {
        let (x0, x1) = (x.saturating_sub(1), (x + 1).min(self.width - 1));
        (y.saturating_sub(1)..=(y + 1).min(self.height - 1)).all(|yy| {
            let row = yy * self.width;
            self.owners[row + x0..=row + x1].iter().all(|&n| n == id)
        })
    }

    /// Visible area and interior votes, per paint.
    fn votes(&self) -> (Votes, Votes) {
        let mut area = vec![0usize; self.paints.len()];
        let mut inner = vec![0usize; self.paints.len()];
        for (i, &id) in self.owners.iter().enumerate() {
            if id == NONE {
                continue;
            }
            area[id as usize] += 1;
            if self.is_interior(id, i % self.width, i / self.width) {
                inner[id as usize] += 1;
            }
        }
        let mut visible = Votes::new();
        let mut interior = Votes::new();
        for (id, &paint) in self.paints.iter().enumerate() {
            if area[id] > 0 {
                *visible.entry(paint).or_default() += area[id];
            }
            if inner[id] > 0 {
                *interior.entry(paint).or_default() += inner[id];
            }
        }
        (visible, interior)
    }

    /// The background behind pixel `i` if it lies on a thin mark: the mean of
    /// the two flanks of its strongest cross-section whose flanks agree. Two
    /// similar flanks mean one background behind the mark; at an ordinary edge
    /// between two fills the flanks differ.
    fn background(&self, i: usize) -> Option<Swatch> {
        let id = self.owners[i];
        let (x, y) = (i % self.width, i / self.width);
        if id == NONE || self.is_interior(id, x, y) {
            return None;
        }
        let mut best = None;
        let mut best_contrast = MARK_CONTRAST * MARK_CONTRAST;
        for (dx, dy) in [(1isize, 0isize), (0, 1), (1, 1), (1, -1)] {
            for r in [1, 2] {
                let sample = |step: isize| {
                    let xx = x.checked_add_signed(step * dx)?;
                    let yy = y.checked_add_signed(step * dy)?;
                    if xx >= self.width || yy >= self.height {
                        return None;
                    }
                    let n = self.owners[yy * self.width + xx];
                    (n != NONE && n != id).then_some(n as usize)
                };
                let (Some(a), Some(b)) = (sample(r), sample(-r)) else {
                    continue;
                };
                if !near(&self.labs[a], &self.labs[b]) {
                    continue;
                }
                let contrast = self.labs[a].distance_squared(&self.labs[id as usize]);
                if contrast > best_contrast {
                    best_contrast = contrast;
                    let (pa, pb) = (self.paints[a], self.paints[b]);
                    best = Some(array::from_fn(|d| ((pa[d] as u16 + pb[d] as u16) / 2) as u8));
                }
            }
        }
        best
    }

    /// Fill the slots `fills` leaves with the inks of thin marks, ranked by
    /// estimated coverage and admitted from [`INK_SUPPORT`] of the visible area.
    fn add_inks(
        &self,
        fills: Vec<Swatch>,
        peaks: &[(Swatch, Oklab)],
        total: usize,
        k: usize,
    ) -> Vec<Swatch> {
        let mut palette = fills;
        if palette.len() >= k || peaks.is_empty() {
            return palette;
        }
        let fill_labs: Vec<Oklab> = palette.iter().map(|&c| lab(c)).collect();
        let taken = |c: &Oklab| fill_labs.iter().any(|f| near(f, c));
        let mut flanks: Vec<Option<Swatch>> =
            (0..self.owners.len()).map(|i| self.background(i)).collect();
        let mut cache = HashMap::new();
        let mut support = BTreeMap::<Swatch, f64>::new();
        let mut mark: Vec<(usize, Swatch)> = Vec::new();
        for start in 0..flanks.len() {
            let Some(bg) = flanks[start].take() else {
                continue;
            };
            // Flood one connected mark: 8-neighbours that are mark pixels over a
            // similar background. Its ink is its pixel of strongest contrast.
            mark.clear();
            mark.push((start, bg));
            let mut cursor = 0;
            let mut ink = self.paints[self.owners[start] as usize];
            let mut strongest = 0.;
            while cursor < mark.len() {
                let (i, bg) = mark[cursor];
                cursor += 1;
                let paint = self.paints[self.owners[i] as usize];
                let contrast: f64 = (0..3).map(|d| (paint[d] as f64 - bg[d] as f64).powi(2)).sum();
                if contrast > strongest {
                    strongest = contrast;
                    ink = paint;
                }
                let bg_lab = cached(&mut cache, bg);
                let (x, y) = (i % self.width, i / self.width);
                for yy in y.saturating_sub(1)..=(y + 1).min(self.height - 1) {
                    for xx in x.saturating_sub(1)..=(x + 1).min(self.width - 1) {
                        let j = yy * self.width + xx;
                        if let Some(other) = flanks[j]
                            && near(&bg_lab, &cached(&mut cache, other))
                        {
                            flanks[j] = None;
                            mark.push((j, other));
                        }
                    }
                }
            }
            let ink_lab = lab(ink);
            if taken(&ink_lab) {
                continue;
            }
            let nearest = peaks
                .iter()
                .map(|(rgb, peak_lab)| (*rgb, peak_lab.distance_squared(&ink_lab)))
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let Some((peak, distance)) = nearest else {
                continue;
            };
            if distance > INK_SNAP * INK_SNAP || taken(&lab(peak)) {
                continue;
            }
            for &(i, bg) in &mark {
                let paint = self.paints[self.owners[i] as usize];
                // Project the observed color onto the background-to-peak mixture
                // line; a pixel near the line is partly covered by this ink.
                // Coverage sums once per pixel across all marks of this peak.
                let ray: [f64; 3] = array::from_fn(|d| peak[d] as f64 - bg[d] as f64);
                let offset: [f64; 3] = array::from_fn(|d| paint[d] as f64 - bg[d] as f64);
                let norm: f64 = ray.iter().map(|v| v * v).sum();
                if norm == 0. {
                    continue;
                }
                let alpha = ((0..3).map(|d| ray[d] * offset[d]).sum::<f64>() / norm).clamp(0., 1.);
                let error: f64 = (0..3).map(|d| (offset[d] - alpha * ray[d]).powi(2)).sum();
                if error > BLEND_ERROR {
                    continue;
                }
                // A fitted peak can be lighter than the real ink. Count the least
                // opacity that could produce this pixel with any ink, so a pale
                // peak cannot inflate support over the gate.
                let coverage = (0..3)
                    .map(|d| {
                        let headroom =
                            if offset[d] < 0. { bg[d] as f64 } else { 255. - bg[d] as f64 };
                        if headroom > 0. { offset[d].abs() / headroom } else { 0. }
                    })
                    .fold(0., f64::max);
                *support.entry(peak).or_default() += coverage;
            }
        }
        let mut ranked: Vec<(Swatch, f64)> = support.into_iter().collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        let floor = (total as f64 * INK_SUPPORT).max(MIN_PIXELS as f64);
        for (peak, area) in ranked {
            if area < floor || palette.len() == k {
                break;
            }
            let peak_lab = lab(peak);
            if !palette.iter().any(|&c| near(&lab(c), &peak_lab)) {
                palette.push(peak);
            }
        }
        palette
    }
}

/// Up to `k` interior fills: the heaviest vote groups clearing a noise floor of
/// one vote per [`VOTE_DIVISOR`] visible pixels. An image of hairlines alone has
/// no interior, so it keeps its most visible paint.
fn fills(interior: &Votes, visible: &Votes, k: usize) -> Vec<Swatch> {
    let total: usize = visible.values().sum();
    let min_votes = (total / VOTE_DIVISOR).max(MIN_PIXELS);
    let mut palette: Vec<Swatch> = grouped(interior)
        .into_iter()
        .filter(|&(n, _)| n >= min_votes)
        .take(k)
        .map(|(_, rgb)| rgb)
        .collect();
    if palette.is_empty() {
        palette.extend(visible.iter().max_by_key(|(_, n)| **n).map(|(&rgb, _)| rgb));
    }
    palette
}

/// Pool votes into perceptual groups, heaviest first. Each group starts at, and
/// is represented by, its most-voted paint: an intermediate fringe color must
/// not displace a solid fill just because it lies near several shades.
fn grouped(votes: &Votes) -> Vec<(usize, Swatch)> {
    let mut candidates: Vec<(Swatch, Oklab, usize)> =
        votes.iter().map(|(&rgb, &n)| (rgb, lab(rgb), n)).collect();
    candidates.sort_unstable_by_key(|&(rgb, _, n)| (n, rgb));
    let mut groups = Vec::new();
    while let Some((rgb, seed, mut total)) = candidates.pop() {
        candidates.retain(|&(_, other, n)| {
            if near(&seed, &other) {
                total += n;
                false
            } else {
                true
            }
        });
        groups.push((total, rgb));
    }
    groups.sort_unstable_by_key(|&group| std::cmp::Reverse(group));
    groups
}

/// Peaks of the visible-color histogram, heaviest basin first, each with its
/// pixel mass and a representative observed paint. A 32³ RGB histogram is
/// smoothed and drained uphill: each occupied bin flows to its highest
/// neighbour until a local maximum, and connected bins of equal height drain
/// as one plateau.
fn histogram_peaks(visible: &Votes) -> Vec<(usize, Swatch)> {
    const SIZE: usize = SIDE * SIDE * SIDE;
    let bin = |[r, g, b]: Swatch| (r as usize / 8) * SIDE * SIDE + (g as usize / 8) * SIDE + b as usize / 8;
    let mut density = vec![0u64; SIZE];
    for (&rgb, &area) in visible {
        density[bin(rgb)] += area as u64;
    }
    // Two passes of a separable [1 2 1] blur.
    for _ in 0..2 {
        for stride in [1, SIDE, SIDE * SIDE] {
            let mut smooth = vec![0; SIZE];
            for (i, v) in smooth.iter_mut().enumerate() {
                let axis = i / stride % SIDE;
                let left = if axis > 0 { i - stride } else { i };
                let right = if axis + 1 < SIDE { i + stride } else { i };
                *v = density[left] + 2 * density[i] + density[right];
            }
            density = smooth;
        }
    }

    let mut visited = vec![false; SIZE];
    let mut drain: Vec<usize> = (0..SIZE).collect();
    for start in 0..SIZE {
        if visited[start] || density[start] == 0 {
            continue;
        }
        let mut plateau = vec![start];
        visited[start] = true;
        let mut outlet = start;
        let mut cursor = 0;
        while cursor < plateau.len() {
            let i = plateau[cursor];
            cursor += 1;
            let (r, g, b) = (i / (SIDE * SIDE), i / SIDE % SIDE, i % SIDE);
            for rr in r.saturating_sub(1)..=(r + 1).min(SIDE - 1) {
                for gg in g.saturating_sub(1)..=(g + 1).min(SIDE - 1) {
                    for bb in b.saturating_sub(1)..=(b + 1).min(SIDE - 1) {
                        let n = (rr * SIDE + gg) * SIDE + bb;
                        if density[n] == density[start] && !visited[n] {
                            visited[n] = true;
                            plateau.push(n);
                        }
                        if density[n] > density[outlet] {
                            outlet = n;
                        }
                    }
                }
            }
        }
        for i in plateau {
            drain[i] = outlet;
        }
    }
    // A basin's mass is its visible pixel count. Its representative is the most
    // frequent paint in the peak bin, or in the nearest occupied bin on a flat top.
    let mut basins = BTreeMap::new();
    for (&rgb, &area) in visible {
        let pos = bin(rgb);
        let mut peak = pos;
        while drain[peak] != peak {
            peak = drain[peak];
        }
        let distance: usize = [1, SIDE, SIDE * SIDE]
            .iter()
            .map(|stride| ((pos / stride) % SIDE).abs_diff((peak / stride) % SIDE).pow(2))
            .sum();
        let score = (std::cmp::Reverse(distance), area, rgb);
        let (mass, best) = basins.entry(peak).or_insert((0, score));
        *mass += area;
        *best = (*best).max(score);
    }
    let mut peaks: Vec<(usize, Swatch)> =
        basins.into_values().map(|(mass, (_, _, rgb))| (mass, rgb)).collect();
    peaks.sort_unstable_by_key(|&p| std::cmp::Reverse(p));
    peaks
}
