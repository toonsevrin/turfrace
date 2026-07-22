#import bevy_pbr::forward_io::VertexOutput

struct PaperMaterial { color: vec4<f32>, parameters: vec4<f32> };
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: PaperMaterial;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let grid = abs(fract(mesh.world_position.xz * 0.72) - vec2(0.5));
    let dot = 1.0 - smoothstep(0.025, 0.075, length(grid));
    let variation = 0.5 + 0.5 * sin(mesh.world_position.x * 0.173 + mesh.world_position.z * 0.117);
    let broad = 0.5 + 0.5 * sin(mesh.world_position.x * 0.041 - mesh.world_position.z * 0.037);
    let shade = material.parameters.x * (dot * 0.20 + variation * 0.20 + broad * 0.12);
    return vec4(material.color.rgb * (1.0 - shade), material.color.a);
}
