@group(0) @binding(0) var<uniform> first_pass_uniform: FirstPassUniform;
@group(0) @binding(0) var<uniform> output_pass_uniform: OutputPassUniform;
@group(0) @binding(1) var sample_texture: texture_2d<f32>;

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

struct OutputPassUniform {
    sample_count: u32,
    subpixel_aa: u32,
    gamma: f32,
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
    // return vec4(in.uv, 0.5, select(1.0, 0.2, in.uv.x * in.uv.x < in.uv.y)) * first_pass_uniform.components;
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
