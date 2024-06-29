#![allow(clippy::new_without_default)]

mod buffer;
mod pipeline_cache;
mod samplers;
mod texture;

use std::collections::HashMap;
use std::mem::size_of;
use std::ops::Range;
use std::sync::Arc;

use buffer::Buffer;
use bytemuck::{Pod, Zeroable};
use glam::UVec2;
use thunderdome::{Arena, Index};
use yakui_core::geometry::{Rect, Vec2, Vec4};
use yakui_core::paint::{PaintCall, PaintDom, Pipeline, Texture, TextureChange, TextureFormat};
use yakui_core::{ManagedTextureId, TextureId};

use self::pipeline_cache::PipelineCache;
use self::samplers::Samplers;
use self::texture::{GpuManagedTexture, GpuTexture};

pub trait CallbackTrait<T> {
    fn prepare(&mut self, _custom_resources: &mut T) {}

    fn finish_prepare(
        &mut self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _encoder: &mut wgpu::CommandEncoder,
        _custom_resources: &mut T,
    ) {
    }

    fn paint<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _custom_resources: &'a T,
    );
}

impl CallbackTrait<()> for () {
    fn paint<'a>(
        &'a self,
        _render_pass: &mut wgpu::RenderPass<'a>,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _custom_resources: &'a (),
    ) {
    }
}

pub struct YakuiWgpu<T> {
    main_pipeline: PipelineCache,
    text_pipeline: PipelineCache,
    layout: wgpu::BindGroupLayout,
    default_texture: GpuManagedTexture,
    samplers: Samplers,
    textures: Arena<GpuTexture>,
    managed_textures: HashMap<ManagedTextureId, GpuManagedTexture>,
    bind_groups: Arena<wgpu::BindGroup>,

    commands: Vec<(DrawCommand<T>, Option<Rect>)>,

    vertices: Buffer,
    indices: Buffer,
}

#[derive(Debug, Clone)]
pub struct SurfaceInfo<'a> {
    pub format: wgpu::TextureFormat,
    pub sample_count: u32,
    pub color_attachments: Vec<Option<wgpu::RenderPassColorAttachment<'a>>>,
    pub depth_format: Option<wgpu::TextureFormat>,
    pub depth_attachment: Option<&'a wgpu::TextureView>,
    pub depth_load_op: Option<wgpu::LoadOp<f32>>,
}

#[derive(Debug, Clone, Copy, Zeroable, Pod)]
#[repr(C)]
struct Vertex {
    pos: Vec2,
    texcoord: Vec2,
    color: Vec4,
}

impl Vertex {
    const DESCRIPTOR: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: size_of::<Self>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![
            0 => Float32x2,
            1 => Float32x2,
            2 => Float32x4,
        ],
    };
}

impl<T> YakuiWgpu<T> {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yakui Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yakui Main Pipeline Layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let main_pipeline = PipelineCache::new(pipeline_layout);

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yakui Text Pipeline Layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let text_pipeline = PipelineCache::new(pipeline_layout);

        let samplers = Samplers::new(device);

        let default_texture_data =
            Texture::new(TextureFormat::Rgba8Srgb, UVec2::new(1, 1), vec![255; 4]);
        let default_texture = GpuManagedTexture::new(device, queue, &default_texture_data);

        Self {
            main_pipeline,
            text_pipeline,
            layout,
            default_texture,
            samplers,
            textures: Arena::new(),
            managed_textures: HashMap::new(),
            bind_groups: Arena::new(),

            commands: Vec::new(),

            vertices: Buffer::new(wgpu::BufferUsages::VERTEX),
            indices: Buffer::new(wgpu::BufferUsages::INDEX),
        }
    }

    /// Creates a `TextureId` from an existing wgpu texture that then be used by
    /// any yakui widgets.
    pub fn add_texture(
        &mut self,
        view: impl Into<Arc<wgpu::TextureView>>,
        min_filter: wgpu::FilterMode,
        mag_filter: wgpu::FilterMode,
        mipmap_filter: wgpu::FilterMode,
        address_mode: wgpu::AddressMode,
    ) -> TextureId {
        let index = self.textures.insert(GpuTexture {
            view: view.into(),
            min_filter,
            mag_filter,
            mipmap_filter,
            address_mode,
        });
        TextureId::User(index.to_bits())
    }

    /// Update an existing texture with a new texture view.
    ///
    /// ## Panics
    ///
    /// Will panic if `TextureId` was not created from a previous call to
    /// `add_texture`.
    pub fn update_texture(&mut self, id: TextureId, view: impl Into<Arc<wgpu::TextureView>>) {
        let index = match id {
            TextureId::User(bits) => Index::from_bits(bits).expect("invalid user texture"),
            _ => panic!("invalid user texture"),
        };

        let existing = self
            .textures
            .get_mut(index)
            .expect("user texture does not exist");
        existing.view = view.into();
    }

    #[must_use = "YakuiWgpu::paint returns a command buffer which MUST be submitted to wgpu."]
    pub fn paint<C: CallbackTrait<T> + 'static>(
        &mut self,
        state: &mut yakui_core::Yakui,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface: SurfaceInfo,
        custom_paint_resoucres: &mut T,
    ) -> [wgpu::CommandBuffer; 2] {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("yakui Encoder"),
        });

        let custom_commands = {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("yakui Render Pass"),
                color_attachments: &surface.color_attachments,
                depth_stencil_attachment: surface.depth_attachment.zip(surface.depth_load_op).map(
                    |(view, load_op)| wgpu::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: Some(wgpu::Operations {
                            load: load_op,
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    },
                ),
                ..Default::default()
            });

            self.paint_with::<C>(
                state,
                device,
                queue,
                &mut render_pass,
                surface,
                custom_paint_resoucres,
            )
        };

        [custom_commands, encoder.finish()]
    }

    pub fn paint_with<'a, C: CallbackTrait<T> + 'static>(
        &'a mut self,
        state: &mut yakui_core::Yakui,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        render_pass: &mut wgpu::RenderPass<'a>,
        surface: SurfaceInfo,
        custom_paint_resoucres: &'a mut T,
    ) -> wgpu::CommandBuffer {
        profiling::scope!("yakui-wgpu paint_with_encoder");

        let mut custom_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("yakui Callback Encoder"),
        });

        let paint = state.paint();

        self.update_textures(device, paint, queue);

        let layers = paint.layers();
        if layers.iter().all(|layer| layer.calls.is_empty()) {
            return custom_encoder.finish();
        }

        if paint.surface_size() == Vec2::ZERO {
            return custom_encoder.finish();
        }

        self.update_buffers::<C>(device, paint, custom_paint_resoucres);

        let vertices = self.vertices.upload(device, queue);
        let indices = self.indices.upload(device, queue);

        let main_pipeline = self.main_pipeline.get(
            surface.format,
            surface.depth_format,
            surface.sample_count,
            |layout| {
                make_main_pipeline(
                    device,
                    layout,
                    surface.format,
                    surface.depth_format,
                    surface.sample_count,
                )
            },
        );

        let text_pipeline = self.text_pipeline.get(
            surface.format,
            surface.depth_format,
            surface.sample_count,
            |layout| {
                make_text_pipeline(
                    device,
                    layout,
                    surface.format,
                    surface.depth_format,
                    surface.sample_count,
                )
            },
        );

        let surface = paint.surface_size().as_uvec2();

        for command in &mut self.commands {
            if let (DrawCommand::Custom(command), clip) = command {
                if let Some(clip) = clip {
                    let pos = clip.pos().as_uvec2();
                    let size = clip.size().as_uvec2();

                    let max = (pos + size).min(surface);
                    let size = UVec2::new(max.x.saturating_sub(pos.x), max.y.saturating_sub(pos.y));

                    // If the rect isn't valid, we can skip this
                    // entire draw call.
                    if pos.x > surface.x || pos.y > surface.y || size.x == 0 || size.y == 0 {
                        continue;
                    }

                    render_pass.set_viewport(
                        pos.x as f32,
                        pos.y as f32,
                        size.x as f32,
                        size.y as f32,
                        0.0,
                        1.0,
                    );
                }

                render_pass.set_scissor_rect(0, 0, surface.x, surface.y);

                command.finish_prepare(device, queue, &mut custom_encoder, custom_paint_resoucres);
            }
        }

        for command in &self.commands {
            match command {
                (DrawCommand::Yakui(command), clip) => {
                    render_pass.set_viewport(
                        0.0,
                        0.0,
                        surface.x as f32,
                        surface.y as f32,
                        0.0,
                        1.0,
                    );

                    match clip {
                        Some(clip) => {
                            let pos = clip.pos().as_uvec2();
                            let size = clip.size().as_uvec2();

                            let max = (pos + size).min(surface);
                            let size = UVec2::new(
                                max.x.saturating_sub(pos.x),
                                max.y.saturating_sub(pos.y),
                            );

                            // If the rect isn't valid, we can skip this
                            // entire draw call.
                            if pos.x > surface.x || pos.y > surface.y || size.x == 0 || size.y == 0
                            {
                                continue;
                            }

                            render_pass.set_scissor_rect(pos.x, pos.y, size.x, size.y);
                        }
                        None => {
                            render_pass.set_scissor_rect(0, 0, surface.x, surface.y);
                        }
                    }

                    match command.pipeline {
                        Pipeline::Main => render_pass.set_pipeline(main_pipeline),
                        Pipeline::Text => render_pass.set_pipeline(text_pipeline),
                        _ => continue,
                    }

                    render_pass.set_vertex_buffer(0, vertices.slice(..));
                    render_pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.set_bind_group(
                        0,
                        self.bind_groups.get(command.bind_group).unwrap(),
                        &[],
                    );
                    render_pass.draw_indexed(command.index_range.clone(), 0, 0..1);
                }
                (DrawCommand::Custom(command), clip) => {
                    if let Some(clip) = clip {
                        let pos = clip.pos().as_uvec2();
                        let size = clip.size().as_uvec2();

                        let max = (pos + size).min(surface);
                        let size =
                            UVec2::new(max.x.saturating_sub(pos.x), max.y.saturating_sub(pos.y));

                        // If the rect isn't valid, we can skip this
                        // entire draw call.
                        if pos.x > surface.x || pos.y > surface.y || size.x == 0 || size.y == 0 {
                            continue;
                        }

                        render_pass.set_viewport(
                            pos.x as f32,
                            pos.y as f32,
                            size.x as f32,
                            size.y as f32,
                            0.0,
                            1.0,
                        );
                    }

                    render_pass.set_scissor_rect(0, 0, surface.x, surface.y);

                    command.paint(render_pass, device, queue, custom_paint_resoucres);
                }
            }
        }

        custom_encoder.finish()
    }

    fn update_buffers<C: CallbackTrait<T> + 'static>(
        &mut self,
        device: &wgpu::Device,
        paint: &mut PaintDom,
        custom_resources: &mut T,
    ) {
        profiling::scope!("update_buffers");

        self.vertices.clear();
        self.indices.clear();
        self.bind_groups.clear();

        let layers = paint.take_layers();

        self.commands.clear();
        self.commands.extend(
            layers
                .into_inner()
                .into_iter()
                .flat_map(|layer| layer.calls)
                .map(|call| match call {
                    (PaintCall::Yakui(call), clip) => {
                        let v = call.vertices.iter().map(|vertex| Vertex {
                            pos: vertex.position,
                            texcoord: vertex.texcoord,
                            color: vertex.color,
                        });

                        let base = self.vertices.len() as u32;
                        let i = call.indices.iter().map(|&index| base + index as u32);

                        let start = self.indices.len() as u32;
                        let end = start + i.len() as u32;

                        self.vertices.extend(v);
                        self.indices.extend(i);

                        let (view, min_filter, mag_filter, mipmap_filter, address_mode) = call
                            .texture
                            .and_then(|id| match id {
                                TextureId::Managed(managed) => {
                                    let texture = self.managed_textures.get(&managed)?;
                                    Some((
                                        &texture.view,
                                        texture.min_filter,
                                        texture.mag_filter,
                                        wgpu::FilterMode::Nearest,
                                        texture.address_mode,
                                    ))
                                }
                                TextureId::User(bits) => {
                                    let index = Index::from_bits(bits)?;
                                    let texture = self.textures.get(index)?;
                                    Some((
                                        &texture.view,
                                        texture.min_filter,
                                        texture.mag_filter,
                                        texture.mipmap_filter,
                                        texture.address_mode,
                                    ))
                                }
                            })
                            .unwrap_or((
                                &self.default_texture.view,
                                self.default_texture.min_filter,
                                self.default_texture.mag_filter,
                                wgpu::FilterMode::Nearest,
                                self.default_texture.address_mode,
                            ));

                        let sampler =
                            self.samplers
                                .get(min_filter, mag_filter, mipmap_filter, address_mode);

                        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("yakui Bind Group"),
                            layout: &self.layout,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::TextureView(view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: wgpu::BindingResource::Sampler(sampler),
                                },
                            ],
                        });

                        (
                            DrawCommand::Yakui(YakuiDrawCommand {
                                index_range: start..end,
                                bind_group: self.bind_groups.insert(bind_group),
                                pipeline: call.pipeline,
                            }),
                            clip,
                        )
                    }
                    (PaintCall::Custom(call), clip) => {
                        let mut command = call.callback.downcast::<C>().unwrap();
                        command.prepare(custom_resources);

                        (DrawCommand::Custom(command), clip)
                    }
                }),
        );
    }

    fn update_textures(&mut self, device: &wgpu::Device, paint: &PaintDom, queue: &wgpu::Queue) {
        profiling::scope!("update_textures");

        for (id, texture) in paint.textures() {
            self.managed_textures
                .entry(id)
                .or_insert_with(|| GpuManagedTexture::new(device, queue, texture));
        }

        for (id, change) in paint.texture_edits() {
            match change {
                TextureChange::Added => {
                    let texture = paint.texture(id).unwrap();
                    self.managed_textures
                        .insert(id, GpuManagedTexture::new(device, queue, texture));
                }

                TextureChange::Removed => {
                    self.managed_textures.remove(&id);
                }

                TextureChange::Modified => {
                    if let Some(existing) = self.managed_textures.get_mut(&id) {
                        let texture = paint.texture(id).unwrap();
                        existing.update(device, queue, texture);
                    }
                }
            }
        }
    }
}

pub enum DrawCommand<T> {
    Yakui(YakuiDrawCommand),
    Custom(Box<dyn CallbackTrait<T>>),
}

pub struct YakuiDrawCommand {
    index_range: Range<u32>,
    bind_group: Index,
    pipeline: Pipeline,
}

fn make_main_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    depth_format: Option<wgpu::TextureFormat>,
    samples: u32,
) -> wgpu::RenderPipeline {
    let main_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Main Shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/main.wgsl").into()),
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("yakui Main Pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &main_shader,
            entry_point: "vs_main",
            buffers: &[Vertex::DESCRIPTOR],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &main_shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: depth_format.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::Always,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        },
        multiview: None,
    })
}

fn make_text_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    depth_format: Option<wgpu::TextureFormat>,
    samples: u32,
) -> wgpu::RenderPipeline {
    let text_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Text Shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/text.wgsl").into()),
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("yakui Text Pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &text_shader,
            entry_point: "vs_main",
            buffers: &[Vertex::DESCRIPTOR],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &text_shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: depth_format.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::Always,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        },
        multiview: None,
    })
}
