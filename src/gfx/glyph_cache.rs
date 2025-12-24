use std::collections::HashMap;
use std::sync::Arc;

/// Cached bezier curve data for a single glyph shape.
/// This is shared across all instances of the same glyph.
#[derive(Debug, Clone)]
pub struct CachedGlyphShape {
    /// Bezier curve data for the glyph, measured in font units (not ems).
    /// Each curve is [start, control, end] points.
    pub curves: Arc<Vec<[[f32; 2]; 3]>>,
}

/// Key for looking up cached glyph shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphCacheKey {
    /// Glyph ID from the font.
    pub glyph_id: u16,
    /// Whether hinting was applied (affects outline shape).
    pub hinted: bool,
}

/// Cache for glyph curve data to avoid re-extracting outlines.
#[derive(Debug, Default)]
pub struct GlyphCache {
    shapes: HashMap<GlyphCacheKey, CachedGlyphShape>,
}

impl GlyphCache {
    pub fn new() -> Self {
        Self {
            shapes: HashMap::new(),
        }
    }

    /// Get or insert a cached glyph shape.
    pub fn get_or_insert(
        &mut self,
        key: GlyphCacheKey,
        f: impl FnOnce() -> Vec<[[f32; 2]; 3]>,
    ) -> &CachedGlyphShape {
        self.shapes.entry(key).or_insert_with(|| CachedGlyphShape {
            curves: Arc::new(f()),
        })
    }

    /// Get a cached glyph shape if it exists.
    pub fn get(&self, key: &GlyphCacheKey) -> Option<&CachedGlyphShape> {
        self.shapes.get(key)
    }

    /// Clear the cache (e.g., when font changes).
    pub fn clear(&mut self) {
        self.shapes.clear();
    }

    /// Number of cached glyphs.
    pub fn len(&self) -> usize {
        self.shapes.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }
}

/// Subdivide a cubic bezier curve into quadratic bezier curves.
/// Uses de Casteljau subdivision to approximate the cubic with multiple quadratics.
pub fn cubic_to_quadratics(
    p0: [f32; 2],
    p1: [f32; 2],
    p2: [f32; 2],
    p3: [f32; 2],
    tolerance: f32,
) -> Vec<[[f32; 2]; 3]> {
    let mut result = Vec::new();
    subdivide_cubic_recursive(p0, p1, p2, p3, tolerance, &mut result, 0);
    result
}

/// Maximum recursion depth to prevent infinite loops.
const MAX_SUBDIVISION_DEPTH: u32 = 8;

fn subdivide_cubic_recursive(
    p0: [f32; 2],
    p1: [f32; 2],
    p2: [f32; 2],
    p3: [f32; 2],
    tolerance: f32,
    output: &mut Vec<[[f32; 2]; 3]>,
    depth: u32,
) {
    // Check if we can approximate this cubic with a single quadratic.
    // A cubic can be well-approximated by a quadratic if the control points
    // are nearly collinear with the appropriate quadratic control point.

    // For a cubic P0, P1, P2, P3, the ideal quadratic control point is:
    // Q1 = (3*P1 - P0 + 3*P2 - P3) / 4
    // But we use a simpler approximation: midpoint of P1 and P2

    let q1 = [
        (3.0 * p1[0] - p0[0] + 3.0 * p2[0] - p3[0]) / 4.0,
        (3.0 * p1[1] - p0[1] + 3.0 * p2[1] - p3[1]) / 4.0,
    ];

    // Measure error: distance from cubic midpoint to quadratic midpoint
    let cubic_mid = eval_cubic(p0, p1, p2, p3, 0.5);
    let quad_mid = eval_quadratic(p0, q1, p3, 0.5);

    let error =
        ((cubic_mid[0] - quad_mid[0]).powi(2) + (cubic_mid[1] - quad_mid[1]).powi(2)).sqrt();

    if error <= tolerance || depth >= MAX_SUBDIVISION_DEPTH {
        // Good enough approximation, output the quadratic
        output.push([p0, q1, p3]);
    } else {
        // Subdivide the cubic at t=0.5 using de Casteljau
        let (left, right) = split_cubic_at_half(p0, p1, p2, p3);
        subdivide_cubic_recursive(left.0, left.1, left.2, left.3, tolerance, output, depth + 1);
        subdivide_cubic_recursive(
            right.0,
            right.1,
            right.2,
            right.3,
            tolerance,
            output,
            depth + 1,
        );
    }
}

/// Evaluate a cubic bezier at parameter t.
fn eval_cubic(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2], t: f32) -> [f32; 2] {
    let t2 = t * t;
    let t3 = t2 * t;
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    let mt3 = mt2 * mt;

    [
        mt3 * p0[0] + 3.0 * mt2 * t * p1[0] + 3.0 * mt * t2 * p2[0] + t3 * p3[0],
        mt3 * p0[1] + 3.0 * mt2 * t * p1[1] + 3.0 * mt * t2 * p2[1] + t3 * p3[1],
    ]
}

/// Evaluate a quadratic bezier at parameter t.
fn eval_quadratic(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], t: f32) -> [f32; 2] {
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    let t2 = t * t;

    [
        mt2 * p0[0] + 2.0 * mt * t * p1[0] + t2 * p2[0],
        mt2 * p0[1] + 2.0 * mt * t * p1[1] + t2 * p2[1],
    ]
}

/// Split a cubic bezier at t=0.5 using de Casteljau algorithm.
/// Returns (left_half, right_half) where each is (p0, p1, p2, p3).
fn split_cubic_at_half(
    p0: [f32; 2],
    p1: [f32; 2],
    p2: [f32; 2],
    p3: [f32; 2],
) -> (
    ([f32; 2], [f32; 2], [f32; 2], [f32; 2]),
    ([f32; 2], [f32; 2], [f32; 2], [f32; 2]),
) {
    // First level
    let q0 = midpoint(p0, p1);
    let q1 = midpoint(p1, p2);
    let q2 = midpoint(p2, p3);

    // Second level
    let r0 = midpoint(q0, q1);
    let r1 = midpoint(q1, q2);

    // Third level (the split point)
    let s = midpoint(r0, r1);

    (
        (p0, q0, r0, s), // Left half
        (s, r1, q2, p3), // Right half
    )
}

fn midpoint(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cubic_to_quadratics_straight_line() {
        // A "cubic" that's actually a straight line should produce one quadratic
        let result = cubic_to_quadratics([0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [3.0, 0.0], 0.1);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_cubic_to_quadratics_curve() {
        // A typical curve
        let result = cubic_to_quadratics([0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0], 0.01);
        // Should subdivide into multiple quadratics for tight tolerance
        assert!(!result.is_empty());
    }
}
