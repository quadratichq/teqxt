@group(0) @binding(0) var<uniform> uniform_params: Uniform;

struct Uniform {
    scale: vec2<f32>,
}

struct VertexInput {
    @location(0) pos: vec2<f32>,
}

@vertex
fn vertex(
    @builtin(vertex_index) index: u32,
    in: VertexInput,
) -> VertexOutput {
    var out: VertexOutput;
    let out_xy = in.pos * uniform_params.scale * vec2(1.0, -1.0);
    out.clip_position = vec4(out_xy, 0.0, 1.0);
    return out;
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(0.3, 0.2, 0.1, 1.0);
}
