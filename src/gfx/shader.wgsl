@group(0) @binding(0) var<uniform> first_pass_uniform: FirstPassUniform;
@group(0) @binding(0) var<uniform> output_pass_uniform: OutputPassUniform;
@group(0) @binding(0) var<uniform> batch_uniform: BatchUniform;
@group(0) @binding(1) var sample_texture: texture_2d<f32>;

// Glyph atlas: storage buffer containing all unique curves
@group(0) @binding(2) var<storage, read> curve_atlas: array<AtlasCurve>;

struct AtlasCurve {
    p0: vec2<f32>,
    p1: vec2<f32>,
    p2: vec2<f32>,
    _padding: vec2<f32>,
}

// Glyph offset for batched rendering (just the offset)
struct GlyphOffset {
    @location(0) offset: vec2<f32>,
}

// Instance data passed via vertex buffer (old per-curve approach)
struct CurveInstance {
    @location(0) curve_index: u32,
    @location(1) _padding: u32,
    @location(2) offset: vec2<f32>,
}

// Legacy format (kept for compatibility)
struct BezierCurveInstance {
    @location(0) offset: vec2<f32>,
    @location(1) p0: vec2<f32>,
    @location(2) p1: vec2<f32>,
    @location(3) p2: vec2<f32>,
}

struct FirstPassUniform {
    components: vec4<f32>,
    scale: vec2<f32>,
    translation: vec2<f32>,
}

// Batched rendering uniform - includes glyph type info
struct BatchUniform {
    components: vec4<f32>,
    scale: vec2<f32>,
    translation: vec2<f32>,
    curve_start: u32,
    curve_count: u32,
    _padding: vec2<u32>,
}

struct OutputPassUniform {
    sample_count: u32,
    subpixel_aa: u32,
    gamma: f32,
}



/// Transforms a position in em space to NDC using batch uniform.
fn batch_em_to_ndc(em_pos: vec2<f32>) -> vec4<f32> {
    let xy = (em_pos + batch_uniform.translation) * batch_uniform.scale;
    return vec4(xy, 0.0, 1.0);
}

fn batch_additive_sample_output_color(front_facing: bool) -> vec4<f32> {
    let out = select(1.0/255.0, 16.0/255.0, front_facing);
    return vec4(vec3(out), 0.0) * batch_uniform.components;
}

/// Transforms a position in em space to NDC.
fn em_to_ndc(em_pos: vec2<f32>) -> vec4<f32> {
    let xy = (em_pos + first_pass_uniform.translation) * first_pass_uniform.scale;
    return vec4(xy, 0.0, 1.0);
}

fn additive_sample_output_color(front_facing: bool) -> vec4<f32> {
    // If back-facing, +1. If front-facing, +16.
    let out = select(1.0/255.0, 16.0/255.0, front_facing);
    return vec4(vec3(out), 0.0) * first_pass_uniform.components;
}


// ============================================================================
// Batched glyph rendering (most efficient - one draw per glyph type)
// ============================================================================

// Batched triangle vertex shader
// vertex_index encodes both which curve (/ 3) and which vertex within curve (% 3)
@vertex
fn batched_triangle_vertex(
    @builtin(vertex_index) vertex_index: u32,
    glyph: GlyphOffset,
) -> TriangleVertexOutput {
    let curve_local_idx = vertex_index / 3u;
    let vertex_in_curve = vertex_index % 3u;
    
    let curve_idx = batch_uniform.curve_start + curve_local_idx;
    let curve = curve_atlas[curve_idx];
    
    let verts = array(
        vec2(0.0, 0.0),
        curve.p0,
        curve.p2,
    );
    
    var out: TriangleVertexOutput;
    out.clip_position = batch_em_to_ndc(glyph.offset + verts[vertex_in_curve]);
    return out;
}

// Batched bezier vertex shader
@vertex
fn batched_bezier_vertex(
    @builtin(vertex_index) vertex_index: u32,
    glyph: GlyphOffset,
) -> BezierVertexOutput {
    let curve_local_idx = vertex_index / 3u;
    let vertex_in_curve = vertex_index % 3u;
    
    let curve_idx = batch_uniform.curve_start + curve_local_idx;
    let curve = curve_atlas[curve_idx];
    
    let verts = array(
        curve.p0,
        curve.p1,
        curve.p2,
    );
    
    var out: BezierVertexOutput;
    out.clip_position = batch_em_to_ndc(glyph.offset + verts[vertex_in_curve]);
    out.uv.x = f32(vertex_in_curve) * 0.5;
    out.uv.y = f32(vertex_in_curve == 2u);
    return out;
}

@fragment
fn batched_triangle_fragment(in: TriangleVertexOutput, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    return batch_additive_sample_output_color(front_facing);
}

@fragment
fn batched_bezier_fragment(in: BezierVertexOutput, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    if in.uv.x * in.uv.x >= in.uv.y {
        discard;
    }
    return batch_additive_sample_output_color(front_facing);
}


// ============================================================================
// Atlas-based rendering (per-curve instances - kept for reference)
// ============================================================================

@vertex
fn atlas_triangle_vertex(@builtin(vertex_index) index: u32, instance: CurveInstance) -> TriangleVertexOutput {
    let curve = curve_atlas[instance.curve_index];
    let verts = array(
        vec2(0.0, 0.0),
        curve.p0,
        curve.p2,
    );
    var out: TriangleVertexOutput;
    out.clip_position = em_to_ndc(instance.offset + verts[index]);
    return out;
}

@vertex
fn atlas_bezier_vertex(@builtin(vertex_index) index: u32, instance: CurveInstance) -> BezierVertexOutput {
    let curve = curve_atlas[instance.curve_index];
    let verts = array(
        curve.p0,
        curve.p1,
        curve.p2,
    );
    var out: BezierVertexOutput;
    out.clip_position = em_to_ndc(instance.offset + verts[index]);
    out.uv.x = f32(index) * 0.5;
    out.uv.y = f32(index == 2);
    return out;
}


// ============================================================================
// Legacy rendering (kept for compatibility)
// ============================================================================

@vertex
fn triangle_vertex(@builtin(vertex_index) index: u32, curve_instance: BezierCurveInstance) -> TriangleVertexOutput {
    let verts = array(
        vec2(0.0, 0.0),
        curve_instance.p0,
        curve_instance.p2,
    );
    var out: TriangleVertexOutput;
    out.clip_position = em_to_ndc(curve_instance.offset + verts[index]);
    return out;
}

struct TriangleVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@fragment
fn triangle_fragment(in: TriangleVertexOutput, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    return additive_sample_output_color(front_facing);
}



@vertex
fn bezier_vertex(@builtin(vertex_index) index: u32, curve_instance: BezierCurveInstance) -> BezierVertexOutput {
    let verts = array(
        curve_instance.p0,
        curve_instance.p1,
        curve_instance.p2,
    );
    var out: BezierVertexOutput;
    out.clip_position = em_to_ndc(curve_instance.offset + verts[index]);
    out.uv.x = f32(index) * 0.5;
    out.uv.y = f32(index == 2);
    return out;
}

struct BezierVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@fragment
fn bezier_fragment(in: BezierVertexOutput, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    // Discard fragment if outside the bezier curve.
    if in.uv.x * in.uv.x >= in.uv.y {
        discard;
    }

    return additive_sample_output_color(front_facing);
}



@vertex
fn output_vertex(@builtin(vertex_index) index: u32) -> BlitVertexOutput {
    let uv = vec2(f32(index % 2), f32(index / 2));
    var out: BlitVertexOutput;
    out.clip_position = vec4(uv * 2.0 - vec2(1.0), 0.0, 1.0);
    return out;
}

struct BlitVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
}

fn get_totals(coords: vec2<u32>) -> vec4<f32> {
    if any(coords > textureDimensions(sample_texture)) {
        return vec4(0.0);
    }
    let texture_value = textureLoad(sample_texture, coords, 0);

    let data: u32 = pack4x8unorm(texture_value);
    // For each component, compute front-facing count minus back-facing count.
    let packed_totals: u32 = ((data >> 4) & 0x0F0F0F0F) - (data & 0x0F0F0F0F);

    // Display bright red to indicate underflow.
    // If the curve data is good, then this should be impossible.
    if (packed_totals & 0xF0F0F0F0) != 0 {
        return vec4(1.0, 0.0, 0.0, 1.0);
    }

    // Get total for each component separately, then convert to float.
    let totals = vec4<f32>(unpack4xU8(packed_totals));

    return vec4(totals.rgb, 1.0);
}

@fragment
fn output_fragment(in: BlitVertexOutput) -> @location(0) vec4<f32> {
    let coords = vec2<u32>(in.clip_position.xy);

    let sample_count = f32(output_pass_uniform.sample_count);
    let gamma = output_pass_uniform.gamma;

    let mid = get_totals(coords);
    if output_pass_uniform.subpixel_aa != 0 {
        let left = get_totals(coords - vec2(1, 0));
        let right = get_totals(coords + vec2(1, 0));
        return vec4(
            pow((left.b + mid.r + mid.g) / sample_count, gamma),
            pow((mid.r + mid.g + mid.b) / sample_count, gamma),
            pow((mid.g + mid.b + right.r) / sample_count, gamma),
            mid.a,
        );
    } else {
        return vec4(
            vec3(pow((mid.r + mid.g + mid.b) / sample_count, gamma)),
            mid.a,
        );
    }
}
