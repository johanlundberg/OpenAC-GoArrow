//! Smooth shapes the plugin draws for itself: the canvas painter has only
//! lines, rectangles, text and images, and its lines are not smoothed, so
//! the arrow, the dial and the map's marks are drawn here into coverage
//! masks once and handed to the client as white images it tints
//! ([`crate::sprites`]).
//!
//! A mask's pixel is the share of its area the shape covers, counted over
//! [`SAMPLES`] x [`SAMPLES`] points spread evenly over the pixel: 255 well
//! inside, 0 well outside, in between along the edges (anti-aliased).
//! Coordinates are the mask's pixels, `x` to the right and `y` down, a
//! pixel `(x, y)` covering `x..x + 1` and `y..y + 1`.
//!
//! Pure: no host calls.

/// A straight line: its two ends.
pub type Segment = ((f32, f32), (f32, f32));

/// Sample points per pixel along each side.
pub const SAMPLES: u32 = 4;

/// The navigation arrow's right half, pointing up, in units of its
/// half-length about its middle: the tip, the right wing's end and the
/// notch in the tail. The left half is its mirror.
pub const ARROW: [(f32, f32); 3] = [(0.0, -1.0), (0.7, 0.82), (0.0, 0.42)];

/// How much of each pixel a shape covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mask {
    /// Width, pixels.
    pub width: u32,
    /// Height, pixels.
    pub height: u32,
    /// Coverage per pixel, rows from the top: 0 none, 255 all.
    pub alpha: Vec<u8>,
}

impl Mask {
    /// The coverage of the shape `inside` says a point is in.
    #[must_use]
    pub fn rasterise(width: u32, height: u32, inside: impl Fn(f32, f32) -> bool) -> Self {
        let samples = SAMPLES * SAMPLES;
        let step = 1.0 / SAMPLES as f32;
        let mut alpha = Vec::with_capacity(width as usize * height as usize);
        for y in 0..height {
            for x in 0..width {
                let mut hits = 0;
                for j in 0..SAMPLES {
                    let sy = y as f32 + (j as f32 + 0.5) * step;
                    for i in 0..SAMPLES {
                        let sx = x as f32 + (i as f32 + 0.5) * step;
                        if inside(sx, sy) {
                            hits += 1;
                        }
                    }
                }
                alpha.push(((hits * 255 + samples / 2) / samples) as u8);
            }
        }
        Self {
            width,
            height,
            alpha,
        }
    }

    /// The coverage of pixel `(x, y)`; 0 off the mask.
    #[must_use]
    pub fn at(&self, x: u32, y: u32) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.alpha[(y * self.width + x) as usize]
    }

    /// The mask as an image: white, its coverage the alpha (straight, not
    /// premultiplied), so a tint gives the shape its colour.
    #[must_use]
    pub fn rgba(&self) -> Vec<u8> {
        self.alpha
            .iter()
            .flat_map(|a| [255, 255, 255, *a])
            .collect()
    }
}

/// How far `p` is from the segment `a`-`b`.
#[must_use]
pub fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (cx, cy) = (a.0 + t * dx, a.1 + t * dy);
    ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
}

/// Whether `p` is inside the polygon `points` (even-odd).
#[must_use]
pub fn polygon_contains(points: &[(f32, f32)], p: (f32, f32)) -> bool {
    let mut inside = false;
    let mut j = points.len().wrapping_sub(1);
    for (i, &(xi, yi)) in points.iter().enumerate() {
        let (xj, yj) = points[j];
        if (yi > p.1) != (yj > p.1) && p.0 < (xj - xi) * (p.1 - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// How far `p` is from the polygon `points`: 0 inside, else to its
/// nearest edge.
#[must_use]
pub fn polygon_distance(points: &[(f32, f32)], p: (f32, f32)) -> f32 {
    if polygon_contains(points, p) {
        return 0.0;
    }
    let mut nearest = f32::INFINITY;
    let mut j = points.len().wrapping_sub(1);
    for (i, &a) in points.iter().enumerate() {
        nearest = nearest.min(segment_distance(p, points[j], a));
        j = i;
    }
    nearest
}

/// The navigation arrow ([`ARROW`]) pointing up in a square `side` pixels
/// across, its middle the square's, `half` pixels from middle to tip;
/// `grow` pixels wider all round (0: the arrow itself; more: an edge to
/// draw under it).
#[must_use]
pub fn arrow(side: u32, half: f32, grow: f32) -> Mask {
    let c = side as f32 / 2.0;
    let half_shape = ARROW.map(|(x, y)| (x * half, y * half));
    // Mirrored about the spine: exactly symmetric.
    Mask::rasterise(side, side, |x, y| {
        polygon_distance(&half_shape, ((x - c).abs(), y - c)) <= grow
    })
}

/// The arrow's left half (west of its spine), for a lit face.
#[must_use]
pub fn arrow_left_half(side: u32, half: f32) -> Mask {
    let c = side as f32 / 2.0;
    let half_shape = ARROW.map(|(x, y)| (x * half, y * half));
    Mask::rasterise(side, side, |x, y| {
        x < c && polygon_contains(&half_shape, (c - x, y - c))
    })
}

/// A ring in a square `side` pixels across about its middle: `radius` to
/// the middle of its band, `width` across.
#[must_use]
pub fn ring(side: u32, radius: f32, width: f32) -> Mask {
    let c = side as f32 / 2.0;
    Mask::rasterise(side, side, |x, y| {
        ((x - c).hypot(y - c) - radius).abs() <= width / 2.0
    })
}

/// A dial's ticks in a square `side` pixels across about its middle: one
/// each way, `inner` to `outer` pixels out and `width` across, the north
/// one its own `(inner, outer, width)`.
#[must_use]
pub fn ticks(side: u32, inner: f32, outer: f32, width: f32, north: (f32, f32, f32)) -> Mask {
    let c = side as f32 / 2.0;
    let tick = |along: f32, across: f32, (inner, outer, width): (f32, f32, f32)| {
        along >= inner && along <= outer && across.abs() <= width / 2.0
    };
    Mask::rasterise(side, side, |x, y| {
        let (lx, ly) = (x - c, y - c);
        tick(-ly, lx, north)
            || tick(ly, lx, (inner, outer, width))
            || tick(lx, ly, (inner, outer, width))
            || tick(-lx, ly, (inner, outer, width))
    })
}

/// A disc of `radius` in a square `side` pixels across, about its middle.
#[must_use]
pub fn disk(side: u32, radius: f32) -> Mask {
    let c = side as f32 / 2.0;
    Mask::rasterise(side, side, |x, y| (x - c).hypot(y - c) <= radius)
}

/// A map pin, `width` x `height` pixels: a round head of `radius` about
/// `(width / 2, head)` narrowing to its point at `(width / 2, tip)`;
/// `grow` pixels wider all round (0: the pin itself).
#[must_use]
pub fn pin(width: u32, height: u32, head: f32, radius: f32, tip: f32, grow: f32) -> Mask {
    let c = width as f32 / 2.0;
    let reach = tip - head;
    // Where the sides leave the head: tangent from the point.
    let (sin, cos) = if reach > radius {
        let cos = radius / reach;
        ((1.0 - cos * cos).sqrt(), cos)
    } else {
        (1.0, 0.0)
    };
    let side = [
        (0.0, head),
        (radius * sin, head + radius * cos),
        (0.0, tip.max(head)),
    ];
    Mask::rasterise(width, height, |x, y| {
        let p = ((x - c).abs(), y);
        let to_head = (p.0.hypot(y - head) - radius).max(0.0);
        to_head.min(polygon_distance(&side, p)) <= grow
    })
}

/// A "play" triangle pointing right in a square `side` pixels across,
/// `half` pixels from the middle to its point, its weight centred.
#[must_use]
pub fn play(side: u32, half: f32) -> Mask {
    let c = side as f32 / 2.0;
    let points = [
        (-0.6 * half, -0.85 * half),
        (half, 0.0),
        (-0.6 * half, 0.85 * half),
    ];
    let shift = (points[0].0 + points[1].0 + points[2].0) / 3.0;
    Mask::rasterise(side, side, |x, y| {
        polygon_contains(&points, (x - c + shift, y - c))
    })
}

/// Lines `thickness` pixels across, round-ended, in a mask `width` x
/// `height` pixels.
#[must_use]
pub fn strokes(width: u32, height: u32, segments: &[Segment], thickness: f32) -> Mask {
    Mask::rasterise(width, height, |x, y| {
        segments
            .iter()
            .any(|(a, b)| segment_distance((x, y), *a, *b) <= thickness / 2.0)
    })
}
