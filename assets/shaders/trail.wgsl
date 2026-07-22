#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::globals

struct TrailMaterial {
    color: vec4<f32>,
    parameters: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: TrailMaterial;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    var along = mesh.world_position.x * 0.08 + mesh.world_position.z * 0.11;
#ifdef VERTEX_UVS_A
    along = mesh.uv.x;
#endif
    // Time comes from Bevy's shared globals uniform, not this material.  The
    // material remains stable while the ribbon still gets a moving highlight.
    let shimmer = 0.5 + 0.5 * cos(along * 7.0 - globals.time * 4.0);
    let brightness = 1.0 + material.parameters.y * 0.06 * pow(shimmer, 12.0);
    let edge = smoothstep(0.0, 1.0, 1.0 - abs(mesh.uv.y * 2.0 - 1.0));
    let alpha = material.color.a * (0.78 + 0.22 * edge);
    return vec4(material.color.rgb * brightness * alpha, alpha);
}
