use core::panic;
use std::marker::PhantomData;

use bevy::{
    core::{cast_slice, Pod},
    prelude::{Res, ResMut, Resource},
    render::{
        render_resource::{
            encase::{private::WriteInto, StorageBuffer},
            Buffer, ComputePipeline, ShaderType,
        },
        renderer::{RenderDevice, RenderQueue},
    },
    utils::{HashMap, Uuid},
};
use wgpu::{
    BindGroupDescriptor, BindGroupEntry, BufferDescriptor, BufferUsages, CommandEncoder,
    CommandEncoderDescriptor, ComputePassDescriptor,
};

use crate::{
    error::{Error, Result},
    pipeline_cache::{AppPipelineCache, CachedAppComputePipelineId},
    traits::ComputeWorker,
    worker_builder::AppComputeWorkerBuilder,
};

#[derive(PartialEq, Clone, Copy)]
pub enum RunMode {
    Continuous,
    OneShot(bool),
}

#[derive(PartialEq)]
pub enum WorkerState {
    Created,
    Available,
    Working,
    FinishedWorking,
}

#[derive(Clone, Debug)]
pub(crate) enum Step {
    ComputePass(ComputePass),
    Swap(String, String),
}

#[derive(Clone, Debug)]
pub(crate) struct ComputePass {
    pub(crate) workgroups: [u32; 3],
    pub(crate) vars: Vec<String>,
    pub(crate) shader_uuid: Uuid,
}

#[derive(Clone)]
pub(crate) struct StaggingBuffers {
    read: Buffer,
    read_mapped: bool,
    write: Buffer,
    write_mapped: bool,
}

impl StaggingBuffers {
    pub(crate) fn new<'a>(render_device: &'a RenderDevice, size: u64) -> Self {
        Self {
            read: render_device.create_buffer(&BufferDescriptor {
                label: None,
                size,
                usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            read_mapped: false,
            write: render_device.create_buffer(&BufferDescriptor {
                label: None,
                size,
                usage: BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            write_mapped: false,
        }
    }
}

/// Struct to manage data transfers from/to the GPU
/// it also handles the logic of your compute work.
/// By default, the run mode of the workers is set to continuous,
/// meaning it will run every frames. If you want to run it deterministically
/// use the function `one_shot()` in the builder
#[derive(Resource)]
pub struct AppComputeWorker<W: ComputeWorker> {
    pub(crate) state: WorkerState,
    render_device: RenderDevice,
    render_queue: RenderQueue,
    cached_pipeline_ids: HashMap<Uuid, CachedAppComputePipelineId>,
    pipelines: HashMap<Uuid, Option<ComputePipeline>>,
    buffers: HashMap<String, Buffer>,
    staging_buffers: HashMap<String, StaggingBuffers>,
    steps: Vec<Step>,
    command_encoder: Option<CommandEncoder>,
    write_requested: bool,
    run_mode: RunMode,
    _phantom: PhantomData<W>,
}

impl<W: ComputeWorker> From<&AppComputeWorkerBuilder<'_, W>> for AppComputeWorker<W> {
    /// Create a new [`AppComputeWorker<W>`].
    fn from(builder: &AppComputeWorkerBuilder<W>) -> Self {
        let render_device = builder.world.resource::<RenderDevice>().clone();
        let render_queue = builder.world.resource::<RenderQueue>().clone();

        let pipelines = builder
            .cached_pipeline_ids
            .iter()
            .map(|(uuid, _)| (*uuid, None))
            .collect();

        let command_encoder =
            Some(render_device.create_command_encoder(&CommandEncoderDescriptor { label: None }));

        Self {
            state: WorkerState::Created,
            render_device,
            render_queue,
            cached_pipeline_ids: builder.cached_pipeline_ids.clone(),
            pipelines,
            buffers: builder.buffers.clone(),
            staging_buffers: builder.staging_buffers.clone(),
            steps: builder.steps.clone(),
            command_encoder,
            write_requested: false,
            run_mode: builder.run_mode,
            _phantom: PhantomData::default(),
        }
    }
}

impl<W: ComputeWorker> AppComputeWorker<W> {
    #[inline]
    fn dispatch(&mut self, index: usize) -> Result<()> {
        let compute_pass = match &self.steps[index] {
            Step::ComputePass(compute_pass) => compute_pass,
            Step::Swap(_, _) => return Err(Error::InvalidStep(format!("{:?}", self.steps[index]))),
        };

        let mut entries = vec![];
        for (index, var) in compute_pass.vars.iter().enumerate() {
            let Some(buffer) = self
                    .buffers
                    .get(var)
                    else { return Err(Error::BufferNotFound(var.to_owned())) };

            let entry = BindGroupEntry {
                binding: index as u32,
                resource: buffer.as_entire_binding(),
            };

            entries.push(entry);
        }

        let Some(maybe_pipeline) = self
                .pipelines
                .get(&compute_pass.shader_uuid)
                else { return Err(Error::PipelinesEmpty) };

        let Some(pipeline) = maybe_pipeline else {
                eprintln!("Pipeline isn't ready yet."); 
                return Err(Error::PipelineNotReady);
            };

        let bind_group_layout = pipeline.get_bind_group_layout(0);
        let bind_group = self.render_device.create_bind_group(&BindGroupDescriptor {
            label: None,
            layout: &bind_group_layout,
            entries: &entries,
        });

        let Some(encoder) = &mut self.command_encoder else { return Err(Error::EncoderIsNone) };
        {
            let mut cpass = encoder.begin_compute_pass(&ComputePassDescriptor { label: None });
            cpass.set_pipeline(&pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);
            cpass.dispatch_workgroups(
                compute_pass.workgroups[0],
                compute_pass.workgroups[1],
                compute_pass.workgroups[2],
            )
        }

        Ok(())
    }

    #[inline]
    fn swap(&mut self, index: usize) -> Result<()> {
        let (buf_a_name, buf_b_name) = match &self.steps[index] {
            Step::ComputePass(_) => {
                return Err(Error::InvalidStep(format!("{:?}", self.steps[index])))
            }
            Step::Swap(a, b) => (a.as_str(), b.as_str()),
        };

        if !self.buffers.contains_key(buf_a_name) {
            return Err(Error::BufferNotFound(buf_a_name.to_owned()));
        }

        if !self.buffers.contains_key(buf_b_name) {
            return Err(Error::BufferNotFound(buf_b_name.to_owned()));
        }

        let [buffer_a, buffer_b] = self.buffers.get_many_mut([buf_a_name, buf_b_name]).unwrap();
        std::mem::swap(buffer_a, buffer_b);

        Ok(())
    }

    #[inline]
    fn write_staging_buffers(&mut self) -> Result<&mut Self> {
        for (name, staging_buffer) in &self.staging_buffers {
            let Some(encoder) = &mut self.command_encoder else { return Err(Error::EncoderIsNone); };
            let Some(buffer) = self
                .buffers
                .get(name)
                else { return Err(Error::BufferNotFound(name.to_owned()))};

            encoder.copy_buffer_to_buffer(&staging_buffer.write, 0, &buffer, 0, buffer.size());
        }
        Ok(self)
    }

    #[inline]
    fn read_staging_buffers(&mut self) -> Result<&mut Self> {
        for (name, staging_buffer) in &self.staging_buffers {
            let Some(encoder) = &mut self.command_encoder else { return Err(Error::EncoderIsNone); };
            let Some(buffer) = self
                .buffers
                .get(name)
                else { return Err(Error::BufferNotFound(name.to_owned()))};

            encoder.copy_buffer_to_buffer(
                &buffer,
                0,
                &staging_buffer.read,
                0,
                staging_buffer.read.size(),
            );
        }
        Ok(self)
    }

    #[inline]
    fn map_staging_buffers(&mut self) -> &mut Self {
        for (_, staging_buffer) in self.staging_buffers.iter_mut() {
            let read_buffer_slice = staging_buffer.read.slice(..);
            let write_buffer_slice = staging_buffer.write.slice(..);

            read_buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
                let err = result.err();
                if err.is_some() {
                    let some_err = err.unwrap();
                    panic!("{}", some_err.to_string());
                }
            });

            staging_buffer.read_mapped = true;

            if self.write_requested {
                write_buffer_slice.map_async(wgpu::MapMode::Write, move |result| {
                    let err = result.err();
                    if err.is_some() {
                        let some_err = err.unwrap();
                        panic!("{}", some_err.to_string());
                    }
                });
                staging_buffer.write_mapped = true;
            }
        }
        self
    }

    /// Read data from `target` staging buffer, return raw bytes
    #[inline]
    pub fn read_raw(&self, target: &str) -> Result<Vec<u8>> {
        let Some(staging_buffer) = &self
            .staging_buffers
            .get(target)
            else { return Err(Error::StagingBufferNotFound(target.to_owned()))};

        let result = staging_buffer
            .read
            .slice(..)
            .get_mapped_range()
            .as_ref()
            .to_vec();

        Ok(result)
    }

    /// Read data from `target` staging buffer, return a vector of `B: Pod`
    #[inline]
    pub fn read<B: Pod>(&self, target: &str) -> Result<Vec<B>> {
        let Some(staging_buffer) = &self
            .staging_buffers
            .get(target)
            else { return Err(Error::StagingBufferNotFound(target.to_owned())) };

        let buffer_view = staging_buffer.read.slice(..).get_mapped_range();

        let bytes = buffer_view.as_ref();

        Ok(cast_slice(bytes).to_vec())
    }

    /// Read data from `target` staging buffer, return a single `B: Pod`
    #[inline]
    pub fn read_one<B: Pod>(&self, target: &str) -> Result<B> {
        let Some(staging_buffer) = &self
            .staging_buffers
            .get(target)
            else { return Err(Error::StagingBufferNotFound(target.to_owned())) };

        let buffer_view = staging_buffer.read.slice(..).get_mapped_range();

        let bytes = buffer_view.as_ref();

        Ok(cast_slice(bytes).to_vec()[0])
    }

    /// Write data to `target` staging buffer.
    #[inline]
    pub fn write<T: ShaderType + WriteInto>(&mut self, target: &str, data: &T) -> Result<()> {
        let Some(staging_buffer) = &self
            .staging_buffers
            .get(target)
            else { return Err(Error::StagingBufferNotFound(target.to_owned())) };

        let mut buffer = StorageBuffer::new(Vec::new());
        buffer.write::<T>(data).unwrap();

        self.render_queue
            .write_buffer(&staging_buffer.write, 0, &buffer.as_ref());
        self.write_requested = true;

        Ok(())
    }

    fn submit(&mut self) -> &mut Self {
        let encoder = self.command_encoder.take().unwrap();
        self.render_queue.submit(Some(encoder.finish()));
        self.state = WorkerState::Working;
        self
    }

    #[inline]
    fn poll(&self) -> bool {
        self.render_device
            .wgpu_device()
            .poll(wgpu::MaintainBase::Wait)
    }

    /// Check if the worker is ready to be read from.
    #[inline]
    pub fn ready(&self) -> bool {
        self.state == WorkerState::FinishedWorking
    }

    /// Tell the worker to execute the compute shader at the end of the current frame
    #[inline]
    pub fn execute(&mut self) {
        match self.run_mode {
            RunMode::Continuous => {}
            RunMode::OneShot(_) => self.run_mode = RunMode::OneShot(true),
        }
    }

    #[inline]
    fn ready_to_execute(&self) -> bool {
        (self.state != WorkerState::Working) && (self.run_mode != RunMode::OneShot(false))
    }

    pub(crate) fn run(mut worker: ResMut<Self>) {
        if worker.ready() {
            worker.state = WorkerState::Available;
        }

        if worker.ready_to_execute() {
            if worker.write_requested {
                worker.write_staging_buffers().unwrap();
                worker.write_requested = false;
            }

            // Workaround for interior mutability
            for i in 0..worker.steps.len() {
                match worker.steps[i] {
                    Step::ComputePass(_) => worker.dispatch(i),
                    Step::Swap(_, _) => worker.swap(i),
                }
                .ok();
            }

            worker.read_staging_buffers().unwrap();
            worker.submit();
            worker.map_staging_buffers();
        }

        if worker.run_mode != RunMode::OneShot(false) && worker.poll() {
            worker.state = WorkerState::FinishedWorking;
            worker.command_encoder = Some(
                worker
                    .render_device
                    .create_command_encoder(&CommandEncoderDescriptor { label: None }),
            );

            match worker.run_mode {
                RunMode::Continuous => {}
                RunMode::OneShot(_) => worker.run_mode = RunMode::OneShot(false),
            };
        }
    }

    pub(crate) fn unmap_all(mut worker: ResMut<Self>) {
        if !worker.ready_to_execute() {
            return;
        };

        for (_, buffer) in &mut worker.staging_buffers {
            if buffer.read_mapped {
                buffer.read.unmap();
                buffer.read_mapped = false;
            }

            if buffer.write_mapped {
                buffer.write.unmap();
                buffer.write_mapped = false;
            }
        }
    }

    pub(crate) fn extract_pipelines(
        mut worker: ResMut<Self>,
        pipeline_cache: Res<AppPipelineCache>,
    ) {
        for (uuid, cached_id) in &worker.cached_pipeline_ids.clone() {
            let Some(pipeline) = worker.pipelines.get(&uuid) else { continue; };

            if pipeline.is_some() {
                continue;
            };

            let cached_id = cached_id.clone();

            worker.pipelines.insert(
                *uuid,
                pipeline_cache.get_compute_pipeline(cached_id).cloned(),
            );
        }
    }
}
