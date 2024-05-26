use crate::shaders::{octree::settings_plugin::OCTreeSettings, OCTree};
use bevy::{
    prelude::*,
    render::{
        globals::{GlobalsBuffer, GlobalsUniform},
        graph::CameraDriverLabel,
        render_graph::*,
        render_resource::{
            binding_types::{storage_buffer, uniform_buffer},
            BindGroup, BindGroupLayout, *,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        Render, RenderApp, RenderSet,
    },
};
use bytemuck::Zeroable;

use super::{
    octree::settings_plugin::{OCTreeBuffer, OCTreeBufferReady, OCTreeRuntime, OCTreeUniform},
    Voxel,
};

pub struct OCTreeComputePlugin;

impl Plugin for OCTreeComputePlugin {
    #[allow(unused_parens)]
    fn build(&self, app: &mut App) {
        let render_app = app.sub_app_mut(RenderApp);
        render_app.add_systems(
            Render,
            (
                prepare_compute_buffers
                    .in_set(RenderSet::Prepare)
                    .run_if(not(resource_exists::<ComputeBuffers>))
                    .run_if(on_event::<OCTreeBufferReady>()),
                prepare_compute_pipeline
                    .in_set(RenderSet::Prepare)
                    .run_if(not(resource_exists::<ComputePipelines>))
                    .run_if(resource_exists::<ComputeBuffers>),
                prepare_bind_group
                    .in_set(RenderSet::PrepareBindGroups)
                    .run_if(not(resource_exists::<ComputeBindGroups>))
                    .run_if(resource_exists::<ComputePipelines>),
            ),
        );
    }

    fn finish(&self, app: &mut App) {
        let settings = app.world().resource::<OCTreeSettings>().clone();

        let render_app = app.sub_app_mut(RenderApp);

        let mut graph = render_app.world_mut().resource_mut::<RenderGraph>();

        println!("0 runs before {:?}", CameraDriverLabel);
        graph.add_node(ComputeNodeLabel(0), ComputeNode(0));
        graph.add_node(ComputeNodeLabel(1), ComputeNode(1));
        graph.add_node(ComputeNodeLabel(2), ComputeNode(2));
        graph.add_node(ComputeNodeLabel(3), ComputeNode(3));

        graph.add_node_edges((
            ComputeNodeLabel(3),
            ComputeNodeLabel(2),
            ComputeNodeLabel(1),
            ComputeNodeLabel(0),
            CameraDriverLabel,
        ))

        // for i in 1..settings.depth + 1 {
        //     println!("{} runs before {}", i, i - 1);
        //
        //     graph.add_node(ComputeNodeLabel(i), ComputeNode(i));
        //     graph.add_node_edge(ComputeNodeLabel(i), ComputeNodeLabel(i - 1));
        // }
    }
}

#[derive(Resource)]
pub struct ComputeBuffers {
    pub octree_gpu: Buffer,
    pub voxel_gpu: Buffer,
}

impl FromWorld for ComputeBuffers {
    fn from_world(world: &mut World) -> Self {
        let settings = world.resource::<OCTreeBuffer>().buffer.get();
        let render_device = world.resource::<RenderDevice>();

        let depth = settings.depth;

        let max_octree = calculate_full_depth(depth) as usize;
        let max_voxel = calculate_max_voxel(depth) as usize;

        let mut init_data = encase::StorageBuffer::new(Vec::new());
        let data = vec![OCTree::zeroed(); max_octree];
        init_data.write(&data).expect("failed to write buffer");

        // The buffer that will be accessed by the gpu
        let octree_gpu_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("octree_gpu_buffer"),
            contents: init_data.as_ref(),
            usage: BufferUsages::STORAGE,
        });

        let mut init_data = encase::StorageBuffer::new(Vec::new());
        let data = vec![Voxel::zeroed(); max_voxel];
        init_data.write(&data).expect("failed to write buffer");

        // The buffer that will be accessed by the gpu
        let voxel_gpu_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("voxel_gpu_buffer"),
            contents: init_data.as_ref(),
            usage: BufferUsages::STORAGE,
        });

        ComputeBuffers {
            octree_gpu: octree_gpu_buffer,
            voxel_gpu: voxel_gpu_buffer,
        }
    }
}

fn prepare_compute_buffers(world: &mut World) {
    info!(
        "Compute Buffers made, depth is {}",
        world.resource::<OCTreeBuffer>().buffer.get().depth
    );
    let cb = ComputeBuffers::from_world(world);
    world.insert_resource(cb);
}

#[derive(Resource)]
pub struct ComputeBindGroups(Vec<BindGroup>);

fn prepare_bind_group(
    mut commands: Commands,
    pipeline: Res<ComputePipelines>,
    render_device: Res<RenderDevice>,
    buffers: Res<ComputeBuffers>,
    globals: Res<GlobalsBuffer>,
    settings: Res<OCTreeBuffer>,
) {
    info!(
        "Compute bind groups made, depth is {}",
        settings.buffer.get().depth
    );
    commands.insert_resource(ComputeBindGroups(
        (0..settings.buffer.get().depth + 1)
            .into_iter()
            .map(|i| {
                render_device.create_bind_group(
                    None,
                    &pipeline.0[i as usize].layout,
                    &BindGroupEntries::sequential((
                        &globals.buffer,
                        &settings.buffer,
                        &settings.runtime_buffer[i as usize],
                        buffers.octree_gpu.as_entire_binding(),
                        buffers.voxel_gpu.as_entire_binding(),
                    )),
                )
            })
            .collect(),
    ));
}

struct ComputePipeline {
    layout: BindGroupLayout,
    pipeline: CachedComputePipelineId,
}

#[derive(Resource)]
pub struct ComputePipelines(Vec<ComputePipeline>);

fn prepare_compute_pipeline(world: &mut World) {
    info!(
        "Compute Pipeline made, depth is {}",
        world.resource::<OCTreeBuffer>().buffer.get().depth
    );

    let cp = ComputePipelines::from_world(world);
    world.insert_resource(cp);
}

impl FromWorld for ComputePipeline {
    fn from_world(world: &mut World) -> Self {
        ComputePipeline::with_dim(world, 0)
    }
}

impl FromWorld for ComputePipelines {
    fn from_world(world: &mut World) -> Self {
        let settings = world.resource::<OCTreeBuffer>().buffer.get();

        ComputePipelines(
            (0..settings.depth + 1)
                .into_iter()
                .map(|i| ComputePipeline::with_dim(world, i))
                .collect(),
        )
    }
}

impl ComputePipeline {
    fn with_dim(world: &mut World, i: u32) -> Self {
        let settings = world.resource::<OCTreeBuffer>().buffer.get();

        let layout = world.resource::<RenderDevice>().create_bind_group_layout(
            format!("ComputeOCTree depth {}", i).as_str(),
            &BindGroupLayoutEntries::sequential(
                ShaderStages::COMPUTE,
                (
                    uniform_buffer::<GlobalsUniform>(false),
                    uniform_buffer::<OCTreeUniform>(false),
                    uniform_buffer::<OCTreeRuntime>(false),
                    storage_buffer::<Vec<OCTree>>(false),
                    storage_buffer::<Vec<Voxel>>(false),
                ),
            ),
        );

        let shader = world.load_asset("shaders/compute.wgsl"); // TODO rename
        let pipeline_cache = world.resource::<PipelineCache>();

        let entry = if settings.depth == i {
            "init"
        } else {
            "finalize"
        };

        let pipeline = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(format!("octree pipeline depth {}", i).into()),
            layout: vec![layout.clone()],
            push_constant_ranges: Vec::new(),
            shader: shader.clone(),
            shader_defs: vec![],
            entry_point: entry.into(),
        });

        ComputePipeline { layout, pipeline }
    }
}

/// Label to identify the node in the render graph
#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct ComputeNodeLabel(u32);

/// The node that will execute the compute shader
#[derive(Default)]
struct ComputeNode(u32);

impl Node for ComputeNode {
    // fn update(&mut self, world: &mut World) {
    //     world.resource_scope(|world, mut octree_buffer: Mut<OCTreeBuffer>| {
    //         let div = world.resource::<RenderDevice>();
    //         let qu = world.resource::<RenderQueue>();
    //
    //         let buf = octree_buffer.buffer.get_mut();
    //         buf.current_depth = self.0;
    //
    //         println!(
    //             "self.0: {}, buf.current_depth: {}",
    //             self.0, buf.current_depth
    //         );
    //
    //         octree_buffer.buffer.write_buffer(&div, &qu);
    //     });
    // }

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        render_context: &mut RenderContext,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let (Some(bind_groups), Some(pipelines), Some(octree_buffer)) = (
            world.get_resource::<ComputeBindGroups>(),
            world.get_resource::<ComputePipelines>(),
            world.get_resource::<OCTreeBuffer>(),
        ) else {
            return Ok(());
        };

        let pipeline_cache = world.resource::<PipelineCache>();
        let depth = self.0;
        let size = calculate_current_size(depth); // first pass, populate data.

        // if depth > 2 {
        //     println!("!!!!!!!!!!!!");
        // }

        if let Some(target_pipeline) =
            pipeline_cache.get_compute_pipeline(pipelines.0[self.0 as usize].pipeline)
        {
            let mut pass =
                render_context
                    .command_encoder()
                    .begin_compute_pass(&ComputePassDescriptor {
                        label: Some(format!("GPU readback compute pass: {}", self.0).as_str()),
                        ..default()
                    });

            pass.set_bind_group(0, &bind_groups.0[self.0 as usize], &[]);
            pass.set_pipeline(target_pipeline);
            pass.dispatch_workgroups(size, size, size);
        }

        Ok(())
    }
}

pub fn calculate_full_depth(depth: u32) -> u32 {
    ((8_f64.powf((depth + 1) as f64) - 1.) / 7.) as u32
}

pub fn calculate_max_voxel(depth: u32) -> u32 {
    8_f64.powf(depth as f64 + 1.) as u32
}

pub fn calculate_current_size(depth: u32) -> u32 {
    if depth == 0 {
        return 1; // first octree is 1.
    }

    2_f64.powf(depth as f64) as u32
}
