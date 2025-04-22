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
}


fn get_pos(i: u32) -> vec2<f32> {
    return curve_data[i];
}
fn transform_pos(xy: vec2<f32>) -> vec4<f32> {
    return vec4((xy + uniform_params.translation) * uniform_params.scale, 0.0, 1.0);
}



@vertex
fn triangle_vertex(@builtin(vertex_index) index: u32) -> TriangleVertexOutput {
    var out: TriangleVertexOutput;
    out.clip_position = transform_pos(get_pos(index + index / 3));
    return out;
}

struct TriangleVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@fragment
fn triangle_fragment(in: TriangleVertexOutput) -> @location(0) vec4<f32> {
    return vec4(1.0/255.0, 0.0, 0.0, 0.1);
}



@vertex
fn bezier_vertex(@builtin(vertex_index) index: u32) -> BezierVertexOutput {
    var out: BezierVertexOutput;
    out.clip_position = transform_pos(get_pos(index + index / 3 + 1));
    let vertex_index = index % 3;
    out.barycentric_coordinates = vec2<f32>(vec2(vertex_index) == vec2<u32>(0, 2));
    return out;
}

struct BezierVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) barycentric_coordinates: vec2<f32>,
}

@fragment
fn bezier_fragment(in: BezierVertexOutput) -> @location(0) vec4<f32> {
    let t = in.barycentric_coordinates[0];
    let s = in.barycentric_coordinates[1];
    let tmp = s/2 + t;
    if tmp * tmp > t {
        discard;
    }
    return vec4(1.0/255.0, 0.0, 0.0, 0.1);
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

@fragment
fn postprocess_fragment(in: PostprocessVertexOutput) -> @location(0) vec4<f32> {
    // let size = ;
    // let uv = in.clip_position.xy / vec2<f32>(textureDimensions(sample_texture))
    let coords = vec2<u32>(in.clip_position.xy);
    let data = unpack4xU8(pack4x8unorm(textureLoad(sample_texture, coords, 0)));
    // if data == 0 {
    //     discard;
    // }
    return vec4(1.0, 1.0, 1.0, f32(data.r % 2));
}
