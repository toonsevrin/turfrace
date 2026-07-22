#import bevy_pbr::forward_io::VertexOutput

struct TerritoryMaterial { parameters: vec4<f32> };
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: TerritoryMaterial;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    var color = vec4(1.0);
#ifdef VERTEX_COLORS
    color = mesh.color;
#endif
    var pattern_id = 0.0;
    var is_wall = 0.0;
#ifdef VERTEX_UVS_B
    pattern_id = mesh.uv_b.x;
    is_wall = mesh.uv_b.y;
#endif
    let p = mesh.world_position.xz * 3.4;
    let family = i32(pattern_id) % 12;
    var ink = 0.0;
    if family == 1 { ink = smoothstep(0.74, 0.92, fract((p.x + p.y) * 0.32)); }
    if family == 2 { ink = smoothstep(0.74, 0.92, fract((p.x - p.y) * 0.32)); }
    if family == 3 { ink = smoothstep(0.76, 0.95, fract(p.x * 0.40)) * smoothstep(0.76, 0.95, fract(p.y * 0.40)); }
    if family == 4 { ink = smoothstep(0.78, 0.95, fract(p.x * 0.32)) + smoothstep(0.78, 0.95, fract(p.y * 0.32)); }
    if family == 5 { ink = smoothstep(0.76, 0.95, fract(p.x * 0.38)); }
    if family == 6 { ink = smoothstep(0.76, 0.95, fract(p.y * 0.38)); }
    if family == 7 { ink = smoothstep(0.76, 0.95, fract((p.x + p.y) * 0.28)) * smoothstep(0.76, 0.95, fract((p.x - p.y) * 0.28)); }
    if family == 8 { ink = smoothstep(0.74, 0.95, fract(p.x * 0.30)) * smoothstep(0.74, 0.95, fract(p.y * 0.30)); }
    if family == 9 { ink = smoothstep(0.78, 0.95, fract((p.y + sin(p.x * 0.9) * 0.42) * 0.34)); }
    if family == 10 { ink = smoothstep(0.76, 0.95, fract(length(p) * 0.28)); }
    if family == 11 { ink = smoothstep(0.76, 0.95, fract((abs(fract(p.x * 0.22) - 0.5) + p.y * 0.24))); }
    let wash = 0.5 + 0.5 * sin(mesh.world_position.x * 0.17 + mesh.world_position.z * 0.13);
    let pattern_strength = material.parameters.x * (1.0 - is_wall) * 0.09;
    let wall_strength = is_wall * 0.14;
    return vec4(color.rgb * (1.0 - ink * pattern_strength) * (1.0 - wall_strength + wash * 0.015), 1.0);
}
