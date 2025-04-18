struct VertexInput {
    @location(0) pos: vec2<f32>,
}

@vertex
fn vertex(
    @builtin(vertex_index) index: u32,
    in: VertexInput,
) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.pos, 0.0, 1.0);
    return out;
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(0.3, 0.2, 0.1, 1.0);
}
