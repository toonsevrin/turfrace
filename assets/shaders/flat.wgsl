#import bevy_pbr::forward_io::VertexOutput

struct FlatMaterial {
    color: vec4<f32>,
    parameters: vec4<f32>,
};
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: FlatMaterial;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let light_direction = normalize(vec3(-0.42, 0.82, 0.38));
    let diffuse = 0.78 + 0.22 * max(dot(normalize(mesh.world_normal), light_direction), 0.0);
    let shade = mix(1.0, diffuse, material.parameters.x);
    let alpha = material.color.a;
    return vec4(material.color.rgb * shade * alpha, alpha);
}
