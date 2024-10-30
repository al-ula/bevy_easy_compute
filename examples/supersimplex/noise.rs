use crate::{TARGET_HEIGHT, TARGET_WIDTH};
use bevy::prelude::*;
use bevy_easy_compute::prelude::*;

#[derive(TypePath)]
struct SimpleShader;

impl ComputeShader for SimpleShader {
    fn shader() -> ShaderRef {
        "shaders/OpenSimplex2SVRange.wgsl".into()
    }
}

#[derive(Resource)]
pub struct SuperSimplexComputeWorker;

impl ComputeWorker for SuperSimplexComputeWorker {
    fn build(world: &mut World) -> AppComputeWorker<Self> {
        let buffer_size = TARGET_WIDTH as usize * TARGET_HEIGHT as usize;
        let initial_output: Vec<f32> = vec![0.0; buffer_size];

        // Calculate workgroup count to cover the entire target size
        let workgroup_size = 8; // Keep 8x8x8 workgroup size
        let workgroup_count_x = (TARGET_WIDTH as usize + workgroup_size - 1) / workgroup_size;
        let workgroup_count_y = (TARGET_HEIGHT as usize + workgroup_size - 1) / workgroup_size;
        let workgroup_count_z = (1usize + workgroup_size - 1) / workgroup_size;

        AppComputeWorkerBuilder::new(world)
            .add_uniform("seed", &12335.0f32)
            .add_uniform("start", &Vec3::new(1.0, 1.0, 1.0))
            .add_uniform("next", &Vec3::new(1.0, 1.0, 1.0))
            .add_uniform("frequency", &4.0f32)
            .add_uniform("lacunarity", &2.0f32)
            .add_uniform("persistence", &0.5f32)
            .add_uniform("octaves", &1u32)
            .add_uniform("useConventional", &0u32)
            .add_uniform(
                "target_dims",
                &UVec3::new(TARGET_WIDTH as u32, TARGET_HEIGHT as u32, 1u32),
            )
            .add_staging("output", &initial_output)
            .add_pass::<SimpleShader>(
                [
                    workgroup_count_x as u32,
                    workgroup_count_y as u32,
                    workgroup_count_z as u32,
                ],
                &[
                    "seed",
                    "start",
                    "next",
                    "frequency",
                    "lacunarity",
                    "persistence",
                    "octaves",
                    "useConventional",
                    "target_dims",
                    "output",
                ],
            )
            .one_shot()
            .build()
    }
}

pub struct NoisePlugin;

impl Plugin for NoisePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(AppComputePlugin)
            .add_plugins(AppComputeWorkerPlugin::<SuperSimplexComputeWorker>::default());
    }
}

#[derive(Resource, Default)]
pub struct NoiseResource {
    pub data: Vec<f32>,
}

#[derive(Default)]
pub struct NoiseGenerator {
    pub seed: f32,
    pub start: Vec3,
    pub target: Vec3,
    pub frequency: f32,
    pub lacunarity: f32,
    pub persistence: f32,
    pub octaves: u32,
    pub use_conventional: u32,
}

pub fn noise_generate(
    compute_worker: &mut ResMut<AppComputeWorker<SuperSimplexComputeWorker>>,
    generator: NoiseGenerator,
) {
    compute_worker.write("seed", &generator.seed);
    compute_worker.write("start", &generator.start);
    compute_worker.write("next", &generator.target);
    compute_worker.write("frequency", &generator.frequency);
    compute_worker.write("lacunarity", &generator.lacunarity);
    compute_worker.write("persistence", &generator.persistence);
    compute_worker.write("octaves", &generator.octaves);
    compute_worker.write("useConventional", &generator.use_conventional);
    compute_worker.execute();
}

pub fn update_resource(
    compute_worker: ResMut<AppComputeWorker<SuperSimplexComputeWorker>>,
    mut noise_res: ResMut<NoiseResource>,
) {
    if !compute_worker.ready() {
        return;
    };

    let result: Vec<f32> = compute_worker.read_vec("output");

    noise_res.data = result;
}
