#import bevy_pbr::forward_io::VertexOutput

struct TerritoryMaterial {
    color: vec4<f32>,
    parameters: vec4<f32>,
};
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: TerritoryMaterial;

fn stripe(value: f32, width: f32) -> f32 {
    let centered = abs(fract(value) - 0.5);
    return 1.0 - smoothstep(width, width + 0.10, centered);
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let p = mesh.world_position.xz * 1.18;
    let family = i32(material.parameters.x) % 12;
    var ink = 0.0;
    if family == 1 { ink = stripe((p.x + p.y) * 0.42, 0.12); }
    if family == 2 { ink = stripe((p.x - p.y) * 0.42, 0.12); }
    if family == 3 { ink = stripe((p.x + p.y * 0.28) * 0.52, 0.10) * stripe((p.y - p.x * 0.28) * 0.52, 0.10); }
    if family == 4 { ink = stripe(p.x * 0.42, 0.08); }
    if family == 5 { ink = stripe(p.x * 0.55, 0.11); }
    if family == 6 { ink = stripe(p.y * 0.55, 0.11); }
    if family == 7 { ink = stripe((p.x + p.y) * 0.38, 0.08) * stripe((p.x - p.y) * 0.38, 0.08); }
    if family == 8 { ink = stripe(p.x * 0.48, 0.07) * stripe(p.y * 0.48, 0.07); }
    if family == 9 { ink = stripe(p.y * 0.46, 0.10); }
    if family == 10 { ink = stripe(length(p) * 0.36, 0.10); }
    if family == 11 { ink = stripe(abs(fract(p.x * 0.30) - 0.5) + p.y * 0.28, 0.10); }

    let side = mesh.uv_b.x;
    let depth = mesh.uv_b.y;
    let side_shade = side * (0.24 + depth * 0.10);
    return vec4(material.color.rgb * (1.0 - ink * 0.075 * material.parameters.y) * (1.0 - side_shade), 1.0);
}
