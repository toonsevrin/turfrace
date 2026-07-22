//! Fast, deterministic territory-only renderer review.
//!
//! This intentionally bypasses match simulation. It can render the same
//! predefined shapes through the production ownership-texture renderer:
//!
//! ```text
//! cargo run --example territory_review -- --output target/territory-review.png
//! ```

use std::path::PathBuf;

use bevy::{
    app::AppExit,
    asset::AssetPlugin,
    core_pipeline::Core3d,
    prelude::*,
    render::camera::CameraRenderGraph,
    render::view::screenshot::{Screenshot, save_to_disk},
    window::{PresentMode, WindowResolution},
};
use turfrace::render::{FieldVisual, RenderPlugin, TerritoryVisual};

#[derive(Resource)]
struct ReviewState {
    output: PathBuf,
    frame: u32,
    screenshot: Option<Entity>,
}

fn main() {
    let output = arguments();
    let field = FieldVisual {
        contour: vec![
            Vec2::new(-24.0, -15.0),
            Vec2::new(-20.0, -18.0),
            Vec2::new(18.0, -18.0),
            Vec2::new(24.0, -12.0),
            Vec2::new(24.0, 13.0),
            Vec2::new(17.0, 18.0),
            Vec2::new(-19.0, 18.0),
            Vec2::new(-24.0, 12.0),
        ],
        revision: 1,
    };
    let shapes = predefined_shapes();
    let mut territory = TerritoryVisual {
        width: 96,
        height: 72,
        cell_size: 0.5,
        origin: Vec2::new(-24.0, -18.0),
        owners: vec![0; 96 * 72],
        ..default()
    };
    territory.color_ids[..3].copy_from_slice(&[0, 4, 2]);
    territory.pattern_ids[..3].copy_from_slice(&[2, 7, 4]);
    rasterize_shapes(&mut territory, &shapes);
    territory.revision = 1;
    App::new()
        .insert_resource(ClearColor(Color::srgb_u8(10, 18, 31)))
        .insert_resource(field)
        .insert_resource(territory)
        .insert_resource(ReviewState {
            output,
            frame: 0,
            screenshot: None,
        })
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Turfrace Territory Review".to_owned(),
                        resolution: WindowResolution::new(1280, 720),
                        present_mode: PresentMode::Immediate,
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: format!("{}/assets", env!("CARGO_MANIFEST_DIR")),
                    ..default()
                }),
        )
        .add_plugins(RenderPlugin)
        .add_systems(Startup, setup_camera)
        .add_systems(Update, capture_review)
        .run();
}

fn arguments() -> PathBuf {
    let mut output = None;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--output" => output = Some(PathBuf::from(args.next().expect("--output needs a path"))),
            "--help" | "-h" => {
                println!("usage: territory_review [--output PATH.png]");
                std::process::exit(0);
            }
            other => panic!("unknown argument `{other}`; use --help"),
        }
    }
    output.unwrap_or_else(|| PathBuf::from("target/visual-feedback/territory-review.png"))
}

fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        CameraRenderGraph::new(Core3d),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb_u8(10, 18, 31)),
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            fov: 48.0_f32.to_radians(),
            ..default()
        }),
        Transform::from_xyz(0.0, 34.0, 27.0).looking_at(Vec3::new(0.0, 0.0, 0.0), Vec3::Y),
    ));
}

fn capture_review(
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
    screenshots: Query<Entity, With<Screenshot>>,
    mut review: ResMut<ReviewState>,
) {
    if let Some(entity) = review.screenshot {
        if screenshots.get(entity).is_err() {
            exit.write(AppExit::Success);
        }
        return;
    }
    review.frame += 1;
    // Allow the software Vulkan path several render/asset extraction frames;
    // a screenshot requested during the first swapchain submit can otherwise
    // contain only the clear color even though the meshes already exist.
    if review.frame < 120 {
        return;
    }
    info!(output = ?review.output, "capturing territory review");
    let entity = commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(review.output.clone()))
        .id();
    review.screenshot = Some(entity);
}

#[derive(Clone)]
struct Shape {
    outer: Vec<Vec2>,
    holes: Vec<Vec<Vec2>>,
}

fn predefined_shapes() -> [Shape; 3] {
    [
        Shape {
            outer: radial_shape(Vec2::new(-11.0, 4.0), 7.5, 6.0, 0.35),
            holes: Vec::new(),
        },
        Shape {
            outer: radial_shape(Vec2::new(8.0, 5.0), 7.5, 6.5, 1.7),
            holes: vec![radial_shape_clockwise(Vec2::new(8.0, 5.0), 2.4, 1.9, 0.0)],
        },
        Shape {
            outer: vec![
                Vec2::new(-18.0, -11.0),
                Vec2::new(-12.0, -8.5),
                Vec2::new(-6.0, -5.5),
                Vec2::new(0.0, -1.5),
                Vec2::new(7.0, 2.5),
                Vec2::new(13.0, 6.5),
                Vec2::new(15.0, 9.0),
                Vec2::new(11.0, 9.8),
                Vec2::new(5.0, 7.0),
                Vec2::new(-1.0, 3.4),
                Vec2::new(-7.0, 0.2),
                Vec2::new(-13.0, -3.0),
                Vec2::new(-19.0, -7.0),
            ],
            holes: Vec::new(),
        },
    ]
}

fn radial_shape(center: Vec2, radius_x: f32, radius_y: f32, phase: f32) -> Vec<Vec2> {
    (0..40)
        .map(|index| {
            let angle = std::f32::consts::TAU * index as f32 / 40.0;
            let radius =
                1.0 + 0.10 * (3.0 * angle + phase).sin() + 0.06 * (7.0 * angle - phase).cos();
            center
                + Vec2::new(
                    angle.cos() * radius_x * radius,
                    angle.sin() * radius_y * radius,
                )
        })
        .collect()
}

fn radial_shape_clockwise(center: Vec2, radius_x: f32, radius_y: f32, phase: f32) -> Vec<Vec2> {
    let mut shape = radial_shape(center, radius_x, radius_y, phase);
    shape.reverse();
    shape
}

fn rasterize_shapes(territory: &mut TerritoryVisual, shapes: &[Shape; 3]) {
    for index in 0..territory.owners.len() {
        let cell = CellCoord::from_index(index, territory.width);
        let point = territory.origin
            + Vec2::new(cell.x as f32 + 0.5, cell.y as f32 + 0.5) * territory.cell_size;
        for (owner, shape) in shapes.iter().enumerate() {
            if point_in_polygon(point, &shape.outer)
                && !shape.holes.iter().any(|hole| point_in_polygon(point, hole))
            {
                territory.owners[index] = (owner + 1) as u8;
                break;
            }
        }
    }
}

#[derive(Clone, Copy)]
struct CellCoord {
    x: u32,
    y: u32,
}

impl CellCoord {
    fn from_index(index: usize, width: u32) -> Self {
        Self {
            x: index as u32 % width,
            y: index as u32 / width,
        }
    }
}

fn point_in_polygon(point: Vec2, polygon: &[Vec2]) -> bool {
    polygon
        .iter()
        .copied()
        .zip(polygon.iter().copied().cycle().skip(1))
        .fold(false, |inside, (a, b)| {
            if (a.y > point.y) != (b.y > point.y)
                && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x
            {
                !inside
            } else {
                inside
            }
        })
}
