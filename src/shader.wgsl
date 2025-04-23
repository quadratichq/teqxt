@group(0) @binding(0) var<uniform> uniform_params: Uniform;

// origin, p0, p2, p1. These are arranged so that taking the first 3 gives the
// flat triangle, and taking the last 3 gives the bezier curve.
@group(0) @binding(1) var<storage> curve_data: array<vec2<f32>>;
// struct BezierCurve {
//     @location(0) origin: vec2<f32>,
//     @location(1) p0: vec2<f32>,
//     @location(2) p2: vec2<f32>,
//     @location(3) p1: vec2<f32>,
// }

@group(0) @binding(2) var sample_texture: texture_2d<f32>;

struct Uniform {
    scale: vec2<f32>,
    translation: vec2<f32>,
    components: vec4<f32>,
}

const SAMPLE_COUNT: f32 = 2.0;



fn get_and_transform_pos(i: u32) -> vec4<f32> {
    let xy: vec2<f32> = curve_data[i];
    return vec4(xy * uniform_params.scale + uniform_params.translation, 0.0, 1.0);
}

fn additive_sample_output_color(front_facing: bool) -> vec4<f32> {
    // If back-facing, +1. If front-facing, +16.
    let out = select(1.0/255.0, 16.0/255.0, front_facing);
    return vec4(vec3(out), 0.0);
}



@vertex
fn triangle_vertex(@builtin(vertex_index) index: u32) -> TriangleVertexOutput {
    var out: TriangleVertexOutput;
    out.clip_position = get_and_transform_pos(index + index / 3);
    return out;
}

struct TriangleVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@fragment
fn triangle_fragment(in: TriangleVertexOutput, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    return additive_sample_output_color(front_facing) * uniform_params.components;
}



@vertex
fn bezier_vertex(@builtin(vertex_index) index: u32) -> BezierVertexOutput {
    var out: BezierVertexOutput;

    // +1 gives the Bezier curve data instead of the triangle data.
    out.clip_position = get_and_transform_pos(index + index / 3 + 1);

    let vertex_index = index % 3;
    out.uv = vec2<f32>(
        (vec3<f32>(1.0, 0.0, 0.5))[vertex_index],
        f32(vertex_index == 0),
    );

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

    return additive_sample_output_color(front_facing) * uniform_params.components;
    // return vec4(in.uv, 0.5, select(1.0, 0.2, in.uv.x * in.uv.x < in.uv.y)) * uniform_params.components;
}



@vertex
fn postprocess_vertex(@builtin(vertex_index) index: u32) -> PostprocessVertexOutput {
    let uv = vec2(f32(index % 2), f32(index / 2));
    var out: PostprocessVertexOutput;
    out.clip_position = vec4(uv * 2.0 - vec2(1.0), 0.0, 1.0);
    return out;
}

struct PostprocessVertexOutput {
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
fn postprocess_fragment(in: PostprocessVertexOutput) -> @location(0) vec4<f32> {
    let coords = vec2<u32>(in.clip_position.xy);

    let left = get_totals(coords - vec2(1, 0));
    let mid = get_totals(coords);
    let right = get_totals(coords + vec2(1, 0));
    return vec4(
        (left.b + mid.r + mid.g) / (SAMPLE_COUNT * 3),
        (mid.r + mid.g + mid.b) / (SAMPLE_COUNT * 3),
        (mid.g + mid.b + right.r) / (SAMPLE_COUNT * 3),
        mid.a,
    );
}
