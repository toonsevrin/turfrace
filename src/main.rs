use bevy::{
    asset::AssetPlugin,
    prelude::*,
    window::{PresentMode, WindowResolution},
};
use turfrace::{app_state, match_game, presentation};

fn main() {
    #[allow(unused_mut)]
    let mut window = Window {
        title: "Turfrace".into(),
        resolution: WindowResolution::new(1280, 720),
        present_mode: PresentMode::AutoVsync,
        resizable: true,
        ..default()
    };

    #[cfg(target_arch = "wasm32")]
    {
        window.fit_canvas_to_parent = true;
    }

    App::new()
        .insert_resource(ClearColor(Color::srgb_u8(242, 243, 245)))
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(window),
                    ..default()
                })
                .set(asset_plugin())
                .set(ImagePlugin::default_linear()),
        )
        .add_plugins((
            app_state::AppShellPlugin,
            match_game::MatchPlugin,
            presentation::PresentationPlugin,
        ))
        .run();
}

fn asset_plugin() -> AssetPlugin {
    #[cfg(target_arch = "wasm32")]
    let file_path = "assets".to_owned();
    #[cfg(not(target_arch = "wasm32"))]
    let file_path = format!("{}/assets", env!("CARGO_MANIFEST_DIR"));
    AssetPlugin {
        file_path,
        ..default()
    }
}
