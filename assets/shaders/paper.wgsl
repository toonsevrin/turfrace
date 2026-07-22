#import bevy_pbr::forward_io::VertexOutput

struct PaperMaterial { color: vec4<f32>, parameters: vec4<f32> };
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: PaperMaterial;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let scaled = mesh.world_position.xz * 0.52;
    let distance_to_line = min(abs(fract(scaled.x) - 0.5), abs(fract(scaled.y) - 0.5));
    let grid = 1.0 - smoothstep(0.465, 0.492, distance_to_line);
    let variation = 0.5 + 0.5 * sin(mesh.world_position.x * 0.173 + mesh.world_position.z * 0.117);
    let broad = 0.5 + 0.5 * sin(mesh.world_position.x * 0.041 - mesh.world_position.z * 0.037);
    let shade = material.parameters.x * (grid * 0.08 + variation * 0.025 + broad * 0.018);
    return vec4(material.color.rgb * (1.0 - shade), material.color.a);
}
