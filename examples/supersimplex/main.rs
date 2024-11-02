mod noise;

use std::sync::OnceLock;
use std::time::SystemTime;

use bevy::prelude::*;
use bevy::render::camera::ScalingMode;
use bevy::window::{WindowPlugin, WindowResolution};
use bevy_easy_compute::prelude::*;
use noise::*;

const TARGET_WIDTH: f32 = 1280.0;
const TARGET_HEIGHT: f32 = 720.0;
static STARTUP_TIME: OnceLock<SystemTime> = OnceLock::new();

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "SuperSimplex Noise".to_string(),
                resolution: WindowResolution::new(TARGET_WIDTH, TARGET_HEIGHT),
                resizable: true,
                ..Default::default()
            }),
            ..Default::default()
        }))
        .add_plugins(NoisePlugin)
        .init_resource::<NoiseResource>()
        .add_systems(Startup, start)
        .add_systems(Update, update_texture)
        .add_systems(Update, update_resource)
        .run();
}

fn start(mut commands: Commands) {
    STARTUP_TIME
        .set(SystemTime::now())
        .expect("Failed to set startup time");

    // Camera with WindowSize scaling
    commands.spawn(Camera2dBundle {
        transform: Transform::from_xyz(0.0, 0.0, 999.9), // This z position is crucial
        projection: OrthographicProjection {
            scaling_mode: ScalingMode::AutoMax {
                max_width: TARGET_WIDTH,
                max_height: TARGET_HEIGHT,
            },
            ..default()
        },
        ..default()
    });
    commands.spawn(NoiseView {
        is_noise_generating: IsNoiseGenerating(false),
        sprite: SpriteBundle {
            sprite: Sprite {
                color: Color::WHITE,
                custom_size: Some(Vec2::new(TARGET_WIDTH, TARGET_HEIGHT)),
                ..default()
            },
            transform: Transform::from_xyz(0.0, 0.0, 0.0),
            ..default()
        },
    });
}

#[derive(Component)]
struct IsNoiseGenerating(bool);

#[derive(Bundle)]
struct NoiseView {
    is_noise_generating: IsNoiseGenerating,
    sprite: SpriteBundle,
}

fn generate_noise(
    compute_worker: &mut ResMut<AppComputeWorker<SuperSimplexComputeWorker>>,
    seed: f32,
    start: Vec3,
    target: Vec3,
) {
    let generator = NoiseGenerator {
        seed,
        start,
        target,
        frequency: 0.006,
        lacunarity: 2.0,
        persistence: 0.5,
        octaves: 8,
        use_conventional: 0,
    };
    noise_generate(compute_worker, generator)
}

fn generate_noise_image(
    mut compute_worker: ResMut<AppComputeWorker<SuperSimplexComputeWorker>>,
    noise_res: &ResMut<NoiseResource>,
) -> Image {
    let width = TARGET_WIDTH as usize;
    let height = TARGET_HEIGHT as usize;
    let z = STARTUP_TIME
        .get()
        .expect("Failed to get startup time")
        .elapsed()
        .unwrap()
        .as_millis() as f32
        / 10.0;
    info!("z: {}", z);
    let unix_epoch = std::time::UNIX_EPOCH;
    let seed = STARTUP_TIME
        .get()
        .unwrap()
        .duration_since(unix_epoch)
        .unwrap()
        .as_secs() as f32;

    let start = Vec3::new(0.5, 0.5, z + 0.5);
    let target = Vec3::new(width as f32 + 0.5, height as f32 + 0.5, z + 0.5);

    // Generate noise values
    generate_noise(&mut compute_worker, seed, start, target);
    let noise_values: Vec<f32> = noise_res.data.clone();

    let mut texture_data = vec![0u8; width * height * 4];
    // Convert noise values to RGBA pixels
    for (i, noise_value) in noise_values.iter().enumerate() {
        // Map noise value from [-1,1] to [0,255] for pixel intensity
        let pixel_intensity = ((noise_value + 1.0) / 2.0 * 255.0) as u8;

        let pixel_start = i * 4;
        // Set RGB channels to the same intensity for grayscale
        texture_data[pixel_start] = pixel_intensity; // Red
        texture_data[pixel_start + 1] = pixel_intensity; // Green
        texture_data[pixel_start + 2] = pixel_intensity; // Blue
        texture_data[pixel_start + 3] = 255; // Alpha (fully opaque)
    }

    Image::new_fill(
        bevy::render::render_resource::Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        &texture_data,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        Default::default(),
    )
}

fn update_texture(
    mut query: Query<(Entity, &mut IsNoiseGenerating, &mut Handle<Image>), With<IsNoiseGenerating>>,
    mut assets: ResMut<Assets<Image>>,
    compute_worker: ResMut<AppComputeWorker<SuperSimplexComputeWorker>>,
    noise_res: ResMut<NoiseResource>,
) {
    let value = generate_noise_image(compute_worker, &noise_res);
    for (_entity, mut is_generating, mut texture_handle) in query.iter_mut() {
        let start = SystemTime::now();
        let textureimage = value.clone();
        let asset = assets.add(textureimage);

        // Skip if already generating
        if is_generating.0 {
            continue;
        }

        // Set to generating
        is_generating.0 = true;

        // Update the texture handle
        *texture_handle = asset.clone();

        // Set back to not generating
        is_generating.0 = false;
        let time = start.elapsed().unwrap();
        info!("Generated texture in {:?}", time);
    }
}
