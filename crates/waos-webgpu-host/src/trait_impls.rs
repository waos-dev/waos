// Modified for WaOS by the waos Authors from the wasi-gfx runtime, as NOTICE says.
use core::slice;
use futures::channel::oneshot;
use std::{borrow::Cow, collections::HashMap, sync::Arc};
use wasmtime::component::{Access, Accessor, FutureReader, Resource, StreamReader};
use wasmtime_wasi::runtime::spawn_blocking;

use crate::{
    buffer_mapping::MappableBuffer,
    compilation_info::{CompilationInfo, ShaderModule},
    device_loss::{DeviceLoss, LostInfoProducer},
    device_request::{self, device_descriptor},
    gpu_object::GpuObject,
    labels::ObjectLabel,
    stream_pipe::StreamPipeMap,
    to_core_conversions::{bind_group_layout_descriptor, ToCore},
    types::{
        Buffer, CommandEncoder, ComputePipeline, Device, ErrorHandler, QuerySet, Queue,
        RenderBundleEncoder, RenderPipeline, Texture, TextureAttributes, TextureView,
        WgslLanguageFeatures,
    },
    wasi::webgpu::webgpu,
    WasiWebGpuCtx, WasiWebGpuCtxView, PREFERRED_CANVAS_FORMAT,
};

impl<'a> webgpu::Host for WasiWebGpuCtx<'a> {
    fn get_gpu(&mut self) -> wasmtime::Result<Resource<webgpu::Gpu>> {
        Ok(Resource::new_own(0))
    }
}

impl<'a> webgpu::HostRecordGpuPipelineConstantValue for WasiWebGpuCtx<'a> {
    fn new(&mut self) -> wasmtime::Result<Resource<webgpu::RecordGpuPipelineConstantValue>> {
        Ok(self.table.push(HashMap::new())?)
    }

    fn add(
        &mut self,
        record: Resource<webgpu::RecordGpuPipelineConstantValue>,
        key: String,
        value: webgpu::GpuPipelineConstantValue,
    ) -> wasmtime::Result<()> {
        let record = self.table.get_mut(&record)?;
        record.insert(key, value);
        Ok(())
    }

    fn get(
        &mut self,
        record: Resource<webgpu::RecordGpuPipelineConstantValue>,
        key: String,
    ) -> wasmtime::Result<Option<webgpu::GpuPipelineConstantValue>> {
        let record = self.table.get(&record)?;
        let value = record.get(&key).copied();
        Ok(value)
    }

    fn has(
        &mut self,
        record: Resource<webgpu::RecordGpuPipelineConstantValue>,
        key: String,
    ) -> wasmtime::Result<bool> {
        let record = self.table.get(&record)?;
        Ok(record.contains_key(&key))
    }

    fn remove(
        &mut self,
        record: Resource<webgpu::RecordGpuPipelineConstantValue>,
        key: String,
    ) -> wasmtime::Result<()> {
        let record = self.table.get_mut(&record)?;
        record.remove(&key);
        Ok(())
    }

    fn keys(
        &mut self,
        record: Resource<webgpu::RecordGpuPipelineConstantValue>,
    ) -> wasmtime::Result<Vec<String>> {
        let record = self.table.get(&record)?;
        let keys = record.keys().cloned().collect();
        Ok(keys)
    }

    fn values(
        &mut self,
        record: Resource<webgpu::RecordGpuPipelineConstantValue>,
    ) -> wasmtime::Result<Vec<webgpu::GpuPipelineConstantValue>> {
        let record = self.table.get(&record)?;
        let values = record.values().copied().collect();
        Ok(values)
    }

    fn entries(
        &mut self,
        record: Resource<webgpu::RecordGpuPipelineConstantValue>,
    ) -> wasmtime::Result<Vec<(String, webgpu::GpuPipelineConstantValue)>> {
        let record = self.table.get(&record)?;
        let entries = record.iter().map(|(k, v)| (k.clone(), *v)).collect();
        Ok(entries)
    }

    fn drop(
        &mut self,
        record: Resource<webgpu::RecordGpuPipelineConstantValue>,
    ) -> wasmtime::Result<()> {
        self.table.delete(record)?;
        Ok(())
    }
}

impl<'a> webgpu::HostRecordOptionGpuSize64 for WasiWebGpuCtx<'a> {
    fn new(&mut self) -> wasmtime::Result<Resource<webgpu::RecordOptionGpuSize64>> {
        let record = std::collections::HashMap::new();
        Ok(self.table.push(record)?)
    }
    fn add(
        &mut self,
        record: Resource<webgpu::RecordOptionGpuSize64>,
        key: String,
        value: Option<webgpu::GpuSize64>,
    ) -> wasmtime::Result<()> {
        let record = self.table.get_mut(&record)?;
        record.insert(key, value);
        Ok(())
    }
    fn get(
        &mut self,
        record: Resource<webgpu::RecordOptionGpuSize64>,
        key: String,
    ) -> wasmtime::Result<Option<Option<webgpu::GpuSize64>>> {
        let record = self.table.get(&record)?;
        Ok(record.get(&key).copied())
    }
    fn has(
        &mut self,
        record: Resource<webgpu::RecordOptionGpuSize64>,
        key: String,
    ) -> wasmtime::Result<bool> {
        let record = self.table.get(&record)?;
        Ok(record.contains_key(&key))
    }
    fn remove(
        &mut self,
        record: Resource<webgpu::RecordOptionGpuSize64>,
        key: String,
    ) -> wasmtime::Result<()> {
        let record = self.table.get_mut(&record)?;
        record.remove(&key);
        Ok(())
    }
    fn keys(
        &mut self,
        record: Resource<webgpu::RecordOptionGpuSize64>,
    ) -> wasmtime::Result<Vec<String>> {
        let record = self.table.get(&record)?;
        Ok(record.keys().cloned().collect())
    }
    fn values(
        &mut self,
        record: Resource<webgpu::RecordOptionGpuSize64>,
    ) -> wasmtime::Result<Vec<Option<webgpu::GpuSize64>>> {
        let record = self.table.get(&record)?;
        Ok(record.values().cloned().collect())
    }
    fn entries(
        &mut self,
        record: Resource<webgpu::RecordOptionGpuSize64>,
    ) -> wasmtime::Result<Vec<(String, Option<webgpu::GpuSize64>)>> {
        let record = self.table.get(&record)?;
        Ok(record.iter().map(|(k, v)| (k.clone(), *v)).collect())
    }
    fn drop(&mut self, record: Resource<webgpu::RecordOptionGpuSize64>) -> wasmtime::Result<()> {
        self.table.delete(record)?;
        Ok(())
    }
}

impl<'a> webgpu::HostGpuDevice for WasiWebGpuCtx<'a> {
    fn adapter_info(
        &mut self,
        device: Resource<Device>,
    ) -> wasmtime::Result<Resource<webgpu::GpuAdapterInfo>> {
        let adapter_id = self.table.get(&device)?.adapter.id();
        let info = self.instance.adapter_get_info(adapter_id);
        let info = self.table.push(info)?;
        Ok(info)
    }

    fn create_command_encoder(
        &mut self,
        device: Resource<Device>,
        descriptor: Option<webgpu::GpuCommandEncoderDescriptor>,
    ) -> wasmtime::Result<Resource<CommandEncoder>> {
        let device = self.table.get(&device)?;
        let device_id = device.device_id();
        let error_handler = Arc::clone(&device.error_handler);

        let descriptor = descriptor
            .map(|d| d.to_core(self.table))
            .unwrap_or(wgpu_types::CommandEncoderDescriptor::default());
        let (command_encoder_id, err) =
            self.instance
                .device_create_command_encoder(device_id, &descriptor, None);

        error_handler.handle_possible_error(err);

        let command_encoder = self.table.push(CommandEncoder {
            command_encoder: GpuObject::new(command_encoder_id, self.instance)
                .labelled(descriptor.label.as_deref()),
            error_handler,
        })?;
        Ok(command_encoder)
    }

    fn create_shader_module(
        &mut self,
        device: Resource<Device>,
        descriptor: webgpu::GpuShaderModuleDescriptor,
    ) -> wasmtime::Result<Resource<webgpu::GpuShaderModule>> {
        let device = self.table.get(&device)?;
        let device_id = device.device_id();
        let error_handler = Arc::clone(&device.error_handler);

        let code =
            wgpu_core::pipeline::ShaderModuleSource::Wgsl(Cow::Owned(descriptor.code.to_owned()));
        let descriptor = descriptor.to_core(self.table);
        let (shader, err) =
            self.instance
                .device_create_shader_module(device_id, &descriptor, code, None);
        let compilation = CompilationInfo::of(err.as_ref());

        error_handler.handle_possible_error(err);

        Ok(self.table.push(ShaderModule {
            module: GpuObject::new(shader, self.instance).labelled(descriptor.label.as_deref()),
            compilation,
        })?)
    }

    fn create_render_pipeline(
        &mut self,
        device: Resource<Device>,
        descriptor: webgpu::GpuRenderPipelineDescriptor,
    ) -> wasmtime::Result<Resource<RenderPipeline>> {
        let device = self.table.get(&device)?;
        let device_id = device.device_id();
        let error_handler = Arc::clone(&device.error_handler);

        let descriptor = descriptor.to_core(self.table);
        let (render_pipeline_id, err) =
            self.instance
                .device_create_render_pipeline(device_id, &descriptor, None);

        error_handler.handle_possible_error(err);

        let render_pipeline = self.table.push(RenderPipeline {
            render_pipeline: GpuObject::new(render_pipeline_id, self.instance)
                .labelled(descriptor.label.as_deref()),
            error_handler,
        })?;
        Ok(render_pipeline)
    }

    fn queue(&mut self, device: Resource<Device>) -> wasmtime::Result<Resource<webgpu::GpuQueue>> {
        let device = self.table.get(&device)?;
        let queue = Queue {
            queue: Arc::clone(&device.queue),
            device: Arc::clone(&device.device),
            error_handler: Arc::clone(&device.error_handler),
        };
        Ok(self.table.push(queue)?)
    }

    fn features(
        &mut self,
        device: Resource<webgpu::GpuDevice>,
    ) -> wasmtime::Result<Resource<webgpu::GpuSupportedFeatures>> {
        let device = self.table.get(&device)?.device_id();
        let features = self.instance.device_features(device);
        Ok(self.table.push(features)?)
    }

    fn limits(
        &mut self,
        device: Resource<webgpu::GpuDevice>,
    ) -> wasmtime::Result<Resource<webgpu::GpuSupportedLimits>> {
        let device = self.table.get(&device)?.device_id();
        let limits = self.instance.device_limits(device);
        Ok(self.table.push(limits)?)
    }

    fn destroy(&mut self, device: Resource<webgpu::GpuDevice>) -> wasmtime::Result<()> {
        // WebGPU unmaps the device's buffers, then loses the device at once:
        // https://www.w3.org/TR/webgpu/#dom-gpudevice-destroy
        let device = self.table.get(&device)?;
        device.mappable_buffers.unmap_all(self.instance);
        self.instance.device_destroy(device.device_id());
        device.loss.lose(
            wgpu_types::DeviceLostReason::Destroyed,
            "the device was destroyed".to_owned(),
        );
        // wgpu-core frees what the device held at a poll that finds its queue empty, which
        // this one does unless work is still on the GPU.
        if let Err(error) = self
            .instance
            .device_poll(device.device_id(), wgpu_types::PollType::Poll)
        {
            tracing::warn!("polling a destroyed device: {error}");
        }
        Ok(())
    }

    fn create_buffer(
        &mut self,
        device: Resource<webgpu::GpuDevice>,
        descriptor: webgpu::GpuBufferDescriptor,
    ) -> wasmtime::Result<Resource<webgpu::GpuBuffer>> {
        let device = self.table.get(&device)?.clone();
        let device_id = device.device_id();
        let error_handler = Arc::clone(&device.error_handler);
        let descriptor = descriptor.to_core(self.table);

        let size = descriptor.size;
        let usage = descriptor.usage;
        let map_state = match descriptor.mapped_at_creation {
            true => webgpu::GpuBufferMapState::Mapped,
            false => webgpu::GpuBufferMapState::Unmapped,
        };

        let (buffer_id, err) = self
            .instance
            .device_create_buffer(device_id, &descriptor, None);

        error_handler.handle_possible_error(err);

        let object = Arc::new(MappableBuffer::new(
            GpuObject::new(buffer_id, self.instance).labelled(descriptor.label.as_deref()),
            map_state,
        ));
        let mappable = descriptor.mapped_at_creation
            || usage.intersects(
                wgpu_types::BufferUsages::MAP_READ | wgpu_types::BufferUsages::MAP_WRITE,
            );
        if mappable {
            device.mappable_buffers.add(&object);
        }
        let buffer = Buffer {
            object,
            device,
            size,
            usage,
        };

        Ok(self.table.push(buffer)?)
    }

    fn create_texture(
        &mut self,
        device: Resource<webgpu::GpuDevice>,
        descriptor: webgpu::GpuTextureDescriptor,
    ) -> wasmtime::Result<Resource<webgpu::GpuTexture>> {
        let device = self.table.get(&device)?;
        let device_id = device.device_id();
        let error_handler = Arc::clone(&device.error_handler);

        let core_descriptor = descriptor.clone().to_core(self.table);
        let (texture_id, err) =
            self.instance
                .device_create_texture(device_id, &core_descriptor, None);

        error_handler.handle_possible_error(err);

        Ok(self.table.push(Texture {
            texture: Arc::new(
                GpuObject::new(texture_id, self.instance)
                    .labelled(core_descriptor.label.as_deref()),
            ),
            attributes: TextureAttributes::of(&descriptor, core_descriptor.size),
            error_handler,
        })?)
    }

    fn create_sampler(
        &mut self,
        device: Resource<webgpu::GpuDevice>,
        descriptor: Option<webgpu::GpuSamplerDescriptor>,
    ) -> wasmtime::Result<Resource<webgpu::GpuSampler>> {
        let device = self.table.get(&device)?;
        let device_id = device.device_id();
        let error_handler = Arc::clone(&device.error_handler);

        let descriptor = descriptor
            .map(|d| d.to_core(self.table))
            // https://www.w3.org/TR/webgpu/#dictdef-gpusamplerdescriptor
            .unwrap_or_else(|| wgpu_core::resource::SamplerDescriptor {
                label: None,
                address_modes: [wgpu_types::AddressMode::ClampToEdge; 3],
                mag_filter: wgpu_types::FilterMode::Nearest,
                min_filter: wgpu_types::FilterMode::Nearest,
                mipmap_filter: wgpu_types::MipmapFilterMode::Nearest,
                lod_min_clamp: 0.0,
                lod_max_clamp: 32.0,
                compare: None,
                // TODO: make sure that anisotropy_clamp actually corresponds to maxAnisotropy
                anisotropy_clamp: 1,
                // border_color is not present in WebGPU
                border_color: None,
            });

        let (sampler, err) = self
            .instance
            .device_create_sampler(device_id, &descriptor, None);

        error_handler.handle_possible_error(err);

        Ok(self
            .table
            .push(GpuObject::new(sampler, self.instance).labelled(descriptor.label.as_deref()))?)
    }

    fn create_bind_group_layout(
        &mut self,
        device: Resource<webgpu::GpuDevice>,
        descriptor: webgpu::GpuBindGroupLayoutDescriptor,
    ) -> wasmtime::Result<Resource<webgpu::GpuBindGroupLayout>> {
        let device = self.table.get(&device)?;
        let device_id = device.device_id();
        let error_handler = Arc::clone(&device.error_handler);

        let label = descriptor.label.clone();
        let bind_group_layout = match bind_group_layout_descriptor(descriptor, self.table) {
            Ok(descriptor) => {
                let (bind_group_layout, err) =
                    self.instance
                        .device_create_bind_group_layout(device_id, &descriptor, None);
                error_handler.handle_possible_error(err);
                bind_group_layout
            }
            Err(refusal) => {
                error_handler.report_validation_error(refusal);
                invalid_bind_group_layout(self.instance, device_id)
            }
        };

        Ok(self
            .table
            .push(GpuObject::new(bind_group_layout, self.instance).labelled(label.as_deref()))?)
    }

    fn create_pipeline_layout(
        &mut self,
        device: Resource<webgpu::GpuDevice>,
        descriptor: webgpu::GpuPipelineLayoutDescriptor,
    ) -> wasmtime::Result<Resource<webgpu::GpuPipelineLayout>> {
        let device = self.table.get(&device)?;
        let device_id = device.device_id();
        let error_handler = Arc::clone(&device.error_handler);

        let descriptor = descriptor.to_core(self.table);
        let (pipeline_layout, err) =
            self.instance
                .device_create_pipeline_layout(device_id, &descriptor, None);

        error_handler.handle_possible_error(err);

        Ok(self.table.push(
            GpuObject::new(pipeline_layout, self.instance).labelled(descriptor.label.as_deref()),
        )?)
    }

    fn create_bind_group(
        &mut self,
        device: Resource<webgpu::GpuDevice>,
        descriptor: webgpu::GpuBindGroupDescriptor,
    ) -> wasmtime::Result<Resource<webgpu::GpuBindGroup>> {
        let device = self.table.get(&device)?;
        let device_id = device.device_id();
        let error_handler = Arc::clone(&device.error_handler);

        // A texture bound directly is bound through its default view, which the bind group keeps
        // a reference to; the host's own handle on the view goes once the bind group is made.
        let mut default_views = Vec::new();
        // not using to_core for conversion since we need instance or self for `GpuBindingResource::GpuTexture`
        let descriptor = wgpu_core::binding_model::BindGroupDescriptor {
            label: descriptor.label.map(|l| l.into()),
            layout: descriptor.layout.to_core(self.table),
            entries: descriptor
                .entries
                .into_iter()
                .map(|entry| wgpu_core::binding_model::BindGroupEntry {
                    binding: entry.binding,
                    resource: match entry.resource {
                        webgpu::GpuBindingResource::GpuBuffer(buffer) => {
                            let binding = webgpu::GpuBufferBinding {
                                buffer,
                                offset: None,
                                size: None,
                            };
                            wgpu_core::binding_model::BindingResource::Buffer(
                                binding.to_core(self.table),
                            )
                        }
                        webgpu::GpuBindingResource::GpuBufferBinding(buffer) => {
                            wgpu_core::binding_model::BindingResource::Buffer(
                                buffer.to_core(self.table),
                            )
                        }
                        webgpu::GpuBindingResource::GpuSampler(sampler) => {
                            wgpu_core::binding_model::BindingResource::Sampler(
                                sampler.to_core(self.table),
                            )
                        }
                        webgpu::GpuBindingResource::GpuTexture(texture) => {
                            let view = view_of(
                                self.instance,
                                self.table.get(&texture).unwrap(),
                                &wgpu_core::resource::TextureViewDescriptor::default(),
                            );
                            let view_id = view.id();
                            default_views.push(view);
                            wgpu_core::binding_model::BindingResource::TextureView(view_id)
                        }
                        webgpu::GpuBindingResource::GpuTextureView(texture_view) => {
                            wgpu_core::binding_model::BindingResource::TextureView(
                                texture_view.to_core(self.table),
                            )
                        }
                    },
                })
                .collect(),
        };

        let (bind_group, err) =
            self.instance
                .device_create_bind_group(device_id, &descriptor, None);
        drop(default_views);

        error_handler.handle_possible_error(err);

        Ok(self.table.push(
            GpuObject::new(bind_group, self.instance).labelled(descriptor.label.as_deref()),
        )?)
    }

    fn create_compute_pipeline(
        &mut self,
        device: Resource<webgpu::GpuDevice>,
        descriptor: webgpu::GpuComputePipelineDescriptor,
    ) -> wasmtime::Result<Resource<webgpu::GpuComputePipeline>> {
        let device = self.table.get(&device)?;
        let device_id = device.device_id();
        let error_handler = Arc::clone(&device.error_handler);

        let descriptor = descriptor.to_core(self.table);
        let (compute_pipeline_id, err) =
            self.instance
                .device_create_compute_pipeline(device_id, &descriptor, None);

        error_handler.handle_possible_error(err);

        Ok(self.table.push(ComputePipeline {
            compute_pipeline: GpuObject::new(compute_pipeline_id, self.instance)
                .labelled(descriptor.label.as_deref()),
            error_handler,
        })?)
    }

    fn create_render_bundle_encoder(
        &mut self,
        device: Resource<webgpu::GpuDevice>,
        descriptor: webgpu::GpuRenderBundleEncoderDescriptor,
    ) -> wasmtime::Result<Resource<webgpu::GpuRenderBundleEncoder>> {
        let device = self.table.get(&device)?;
        let device_id = device.device_id();
        let error_handler = Arc::clone(&device.error_handler);
        let descriptor = descriptor.to_core(self.table);
        let (render_bundle_encoder, err) = self
            .instance
            .device_create_render_bundle_encoder(device_id, &descriptor);
        error_handler.handle_possible_error(err);
        Ok(self.table.push(RenderBundleEncoder {
            encoder: *render_bundle_encoder,
            error_handler,
            label: ObjectLabel::of(descriptor.label.as_deref()),
        })?)
    }

    fn create_query_set(
        &mut self,
        device: Resource<webgpu::GpuDevice>,
        descriptor: webgpu::GpuQuerySetDescriptor,
    ) -> wasmtime::Result<Result<Resource<webgpu::GpuQuerySet>, webgpu::CreateQuerySetError>> {
        let device = self.table.get(&device)?;
        let device_id = device.device_id();
        let error_handler = Arc::clone(&device.error_handler);

        let type_ = descriptor.type_;
        let descriptor = descriptor.to_core(self.table);
        let (query_set, err) = self
            .instance
            .device_create_query_set(device_id, &descriptor, None);

        error_handler.handle_possible_error(err);

        Ok(Ok(self.table.push(QuerySet {
            query_set: GpuObject::new(query_set, self.instance)
                .labelled(descriptor.label.as_deref()),
            type_,
            count: descriptor.count,
        })?))
    }

    fn label(&mut self, object: Resource<webgpu::GpuDevice>) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<webgpu::GpuDevice>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn push_error_scope(
        &mut self,
        device: Resource<webgpu::GpuDevice>,
        filter: webgpu::GpuErrorFilter,
    ) -> wasmtime::Result<()> {
        let device = self.table.get(&device)?;
        device.error_handler.push_scope(filter);
        Ok(())
    }

    fn drop(&mut self, device: Resource<webgpu::GpuDevice>) -> wasmtime::Result<()> {
        self.table.delete(device)?;
        Ok(())
    }
}

impl<T: Send + WasiWebGpuCtxView> webgpu::HostGpuDeviceWithStore<T> for crate::HasWasiWebGpuCtx {
    async fn create_compute_pipeline_async(
        accessor: &Accessor<T, Self>,
        device: Resource<webgpu::GpuDevice>,
        descriptor: webgpu::GpuComputePipelineDescriptor,
    ) -> wasmtime::Result<Result<Resource<webgpu::GpuComputePipeline>, webgpu::CreatePipelineError>>
    {
        accessor.with(|mut access| {
            let ctx = access.get();
            let device = ctx.table.get(&device)?;
            let device_id = device.device_id();
            let error_handler = Arc::clone(&device.error_handler);

            let descriptor = descriptor.to_core(ctx.table);
            let (compute_pipeline_id, err) =
                ctx.instance
                    .device_create_compute_pipeline(device_id, &descriptor, None);

            error_handler.handle_possible_error(err);

            Ok(Ok(ctx.table.push(ComputePipeline {
                compute_pipeline: GpuObject::new(compute_pipeline_id, ctx.instance)
                    .labelled(descriptor.label.as_deref()),
                error_handler,
            })?))
        })
    }

    async fn create_render_pipeline_async(
        accessor: &Accessor<T, Self>,
        device: Resource<webgpu::GpuDevice>,
        descriptor: webgpu::GpuRenderPipelineDescriptor,
    ) -> wasmtime::Result<Result<Resource<webgpu::GpuRenderPipeline>, webgpu::CreatePipelineError>>
    {
        accessor.with(|mut access| {
            let ctx = access.get();
            let device = ctx.table.get(&device)?;
            let device_id = device.device_id();
            let error_handler = Arc::clone(&device.error_handler);

            let descriptor = descriptor.to_core(ctx.table);
            let (render_pipeline_id, err) =
                ctx.instance
                    .device_create_render_pipeline(device_id, &descriptor, None);

            error_handler.handle_possible_error(err);

            let render_pipeline = ctx.table.push(RenderPipeline {
                render_pipeline: GpuObject::new(render_pipeline_id, ctx.instance)
                    .labelled(descriptor.label.as_deref()),
                error_handler,
            })?;
            Ok(Ok(render_pipeline))
        })
    }

    async fn pop_error_scope(
        accessor: &Accessor<T, Self>,
        device: Resource<webgpu::GpuDevice>,
    ) -> wasmtime::Result<Result<Option<Resource<webgpu::GpuError>>, webgpu::PopErrorScopeError>>
    {
        accessor.with(|mut access| {
            let ctx = access.get();
            let device = ctx.table.get(&device)?;
            Ok(match device.error_handler.pop_scope() {
                Ok(Some(error)) => Ok(Some(ctx.table.push(error)?)),
                Ok(None) => Ok(None),
                Err(refusal) => Err(refusal),
            })
        })
    }

    fn on_uncaptured_error(
        mut access: Access<T, Self>,
        device: Resource<webgpu::GpuDevice>,
    ) -> wasmtime::Result<StreamReader<Resource<webgpu::GpuError>>> {
        let ctx = access.get();
        let receiver = ctx.table.get(&device)?.error_handler.new_error_receiver();
        StreamReader::new(
            access,
            StreamPipeMap(receiver, |data: &mut T, err| {
                Ok(data.webgpu_ctx().table.push(err)?)
            }),
        )
    }

    fn lost(
        mut access: Access<T, Self>,
        device: Resource<webgpu::GpuDevice>,
    ) -> wasmtime::Result<FutureReader<Resource<webgpu::GpuDeviceLostInfo>>> {
        let info = access.get().table.get(&device)?.loss.info();
        FutureReader::new(access, LostInfoProducer::<T>::new(info))
    }
}

/// A bind group layout wgpu-core holds as invalid, for a descriptor WebGPU refuses before
/// wgpu-core can see it. wgpu-core's `create_bind_group_layout_error` is meant for this, but does
/// not answer the id it makes, so the host asks for a layout wgpu-core refuses, two entries on one
/// binding, and drops wgpu-core's error: the program has been told WebGPU's.
fn invalid_bind_group_layout(
    instance: &wgpu_core::global::Global,
    device: wgpu_core::id::DeviceId,
) -> wgpu_core::id::BindGroupLayoutId {
    let entry = wgpu_types::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu_types::ShaderStages::FRAGMENT,
        ty: wgpu_types::BindingType::Sampler(wgpu_types::SamplerBindingType::Filtering),
        count: None,
    };
    let conflicting = wgpu_core::binding_model::BindGroupLayoutDescriptor {
        label: None,
        entries: vec![entry, entry].into(),
    };
    let (invalid, _refusal) = instance.device_create_bind_group_layout(device, &conflicting, None);
    invalid
}

/// A view of `texture` as `descriptor` describes it, on the instance the texture is on.
fn view_of(
    instance: &Arc<wgpu_core::global::Global>,
    texture: &Texture,
    descriptor: &wgpu_core::resource::TextureViewDescriptor,
) -> TextureView {
    let (view, error) = instance.texture_create_view(texture.id(), descriptor, None);
    texture.error_handler.handle_possible_error(error);
    GpuObject::new(view, instance).labelled(descriptor.label.as_deref())
}

impl<'a> webgpu::HostGpuTexture for WasiWebGpuCtx<'a> {
    fn create_view(
        &mut self,
        texture: Resource<Texture>,
        descriptor: Option<webgpu::GpuTextureViewDescriptor>,
    ) -> wasmtime::Result<Resource<webgpu::GpuTextureView>> {
        let descriptor = descriptor
            .map(|d| d.to_core(self.table))
            .unwrap_or(wgpu_core::resource::TextureViewDescriptor::default());
        let view = view_of(self.instance, self.table.get(&texture)?, &descriptor);
        Ok(self.table.push(view)?)
    }

    fn destroy(&mut self, texture: Resource<webgpu::GpuTexture>) -> wasmtime::Result<()> {
        let texture = self.table.get(&texture)?.id();
        self.instance.texture_destroy(texture);
        Ok(())
    }

    fn width(
        &mut self,
        texture: Resource<webgpu::GpuTexture>,
    ) -> wasmtime::Result<webgpu::GpuIntegerCoordinateOut> {
        Ok(self.table.get(&texture)?.attributes.size.width)
    }

    fn height(
        &mut self,
        texture: Resource<webgpu::GpuTexture>,
    ) -> wasmtime::Result<webgpu::GpuIntegerCoordinateOut> {
        Ok(self.table.get(&texture)?.attributes.size.height)
    }

    fn depth_or_array_layers(
        &mut self,
        texture: Resource<webgpu::GpuTexture>,
    ) -> wasmtime::Result<webgpu::GpuIntegerCoordinateOut> {
        Ok(self
            .table
            .get(&texture)?
            .attributes
            .size
            .depth_or_array_layers)
    }

    fn mip_level_count(
        &mut self,
        texture: Resource<webgpu::GpuTexture>,
    ) -> wasmtime::Result<webgpu::GpuIntegerCoordinateOut> {
        Ok(self.table.get(&texture)?.attributes.mip_level_count)
    }

    fn sample_count(
        &mut self,
        texture: Resource<webgpu::GpuTexture>,
    ) -> wasmtime::Result<webgpu::GpuSize32Out> {
        Ok(self.table.get(&texture)?.attributes.sample_count)
    }

    fn dimension(
        &mut self,
        texture: Resource<webgpu::GpuTexture>,
    ) -> wasmtime::Result<webgpu::GpuTextureDimension> {
        Ok(self.table.get(&texture)?.attributes.dimension)
    }

    fn format(
        &mut self,
        texture: Resource<webgpu::GpuTexture>,
    ) -> wasmtime::Result<webgpu::GpuTextureFormat> {
        Ok(self.table.get(&texture)?.attributes.format)
    }

    fn usage(
        &mut self,
        texture: Resource<webgpu::GpuTexture>,
    ) -> wasmtime::Result<webgpu::GpuTextureUsage> {
        Ok(self.table.get(&texture)?.attributes.usage)
    }

    fn texture_binding_view_dimension(
        &mut self,
        texture: Resource<webgpu::GpuTexture>,
    ) -> wasmtime::Result<Option<webgpu::GpuTextureViewDimension>> {
        Ok(self
            .table
            .get(&texture)?
            .attributes
            .texture_binding_view_dimension)
    }

    fn label(&mut self, object: Resource<webgpu::GpuTexture>) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<webgpu::GpuTexture>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn drop(&mut self, texture: Resource<webgpu::GpuTexture>) -> wasmtime::Result<()> {
        self.table.delete(texture)?;
        Ok(())
    }
}

impl<'a> webgpu::HostGpuTextureView for WasiWebGpuCtx<'a> {
    fn label(&mut self, object: Resource<webgpu::GpuTextureView>) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<webgpu::GpuTextureView>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn drop(&mut self, view: Resource<webgpu::GpuTextureView>) -> wasmtime::Result<()> {
        self.table.delete(view)?;
        Ok(())
    }
}

impl<'a> webgpu::HostGpuCommandBuffer for WasiWebGpuCtx<'a> {
    fn label(&mut self, object: Resource<webgpu::GpuCommandBuffer>) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<webgpu::GpuCommandBuffer>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn drop(&mut self, command_buffer: Resource<webgpu::GpuCommandBuffer>) -> wasmtime::Result<()> {
        self.table.delete(command_buffer)?;
        Ok(())
    }
}

impl<'a> webgpu::HostGpuShaderModule for WasiWebGpuCtx<'a> {
    fn label(&mut self, object: Resource<webgpu::GpuShaderModule>) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<webgpu::GpuShaderModule>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn drop(&mut self, shader: Resource<webgpu::GpuShaderModule>) -> wasmtime::Result<()> {
        self.table.delete(shader)?;
        Ok(())
    }
}

impl<'a> webgpu::HostGpuRenderPipeline for WasiWebGpuCtx<'a> {
    fn label(&mut self, object: Resource<RenderPipeline>) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<RenderPipeline>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn get_bind_group_layout(
        &mut self,
        pipeline: Resource<RenderPipeline>,
        index: u32,
    ) -> wasmtime::Result<Resource<webgpu::GpuBindGroupLayout>> {
        let pipeline = self.table.get(&pipeline)?;
        let pipeline_id = pipeline.id();
        let error_handler = Arc::clone(&pipeline.error_handler);
        let (layout, err) =
            self.instance
                .render_pipeline_get_bind_group_layout(pipeline_id, index, None);
        error_handler.handle_possible_error(err);
        Ok(self.table.push(GpuObject::new(layout, self.instance))?)
    }

    fn drop(&mut self, pipeline: Resource<webgpu::GpuRenderPipeline>) -> wasmtime::Result<()> {
        self.table.delete(pipeline)?;
        Ok(())
    }
}

impl<'a> webgpu::HostGpuAdapter for WasiWebGpuCtx<'a> {
    fn features(
        &mut self,
        adapter: Resource<webgpu::GpuAdapter>,
    ) -> wasmtime::Result<Resource<webgpu::GpuSupportedFeatures>> {
        let adapter = self.table.get(&adapter)?.id();
        let features = self.instance.adapter_features(adapter);
        Ok(self.table.push(features)?)
    }

    fn limits(
        &mut self,
        adapter: Resource<webgpu::GpuAdapter>,
    ) -> wasmtime::Result<Resource<webgpu::GpuSupportedLimits>> {
        let adapter = self.table.get(&adapter)?.id();
        let limits = self.instance.adapter_limits(adapter);
        Ok(self.table.push(limits)?)
    }

    fn info(
        &mut self,
        adapter: Resource<webgpu::GpuAdapter>,
    ) -> wasmtime::Result<Resource<webgpu::GpuAdapterInfo>> {
        let adapter_id = self.table.get(&adapter)?.id();
        let info = self.instance.adapter_get_info(adapter_id);
        Ok(self.table.push(info)?)
    }

    fn drop(&mut self, adapter: Resource<webgpu::GpuAdapter>) -> wasmtime::Result<()> {
        self.table.delete(adapter)?;
        Ok(())
    }
}

impl<T: Send> webgpu::HostGpuAdapterWithStore<T> for crate::HasWasiWebGpuCtx {
    async fn request_device(
        accessor: &Accessor<T, Self>,
        adapter: Resource<webgpu::GpuAdapter>,
        descriptor: Option<webgpu::GpuDeviceDescriptor>,
    ) -> wasmtime::Result<Result<Resource<webgpu::GpuDevice>, webgpu::RequestDeviceError>> {
        accessor.with(|mut access| {
            let ctx = access.get();

            let adapter = Arc::clone(ctx.table.get(&adapter)?);
            let adapter_limits = ctx.instance.adapter_limits(adapter.id());
            let queue_label = descriptor
                .as_ref()
                .and_then(|descriptor| descriptor.default_queue.as_ref())
                .and_then(|queue| queue.label.clone());
            let descriptor = match device_descriptor(descriptor, ctx.table, &adapter_limits) {
                Ok(descriptor) => descriptor,
                Err(refusal) => return Ok(Err(refusal)),
            };
            let opened = match ctx.device_opener {
                Some(opener) => opener.open(ctx.instance, adapter.id(), &descriptor),
                None => ctx
                    .instance
                    .adapter_request_device(adapter.id(), &descriptor, None, None),
            };
            let (device_id, queue_id) = match opened {
                Ok(device_and_queue) => device_and_queue,
                Err(error) => return Ok(Err(device_request::refusal(error))),
            };
            let loss = DeviceLoss::new();
            ctx.instance
                .device_set_device_lost_closure(device_id, loss.reported_by_wgpu());
            let device = ctx.table.push(Device {
                device: Arc::new(
                    GpuObject::new(device_id, ctx.instance).labelled(descriptor.label.as_deref()),
                ),
                queue: Arc::new(
                    GpuObject::new(queue_id, ctx.instance).labelled(queue_label.as_deref()),
                ),
                adapter,
                error_handler: Arc::new(ErrorHandler::new(Arc::clone(&loss))),
                loss,
                mappable_buffers: Arc::default(),
            })?;
            if let Some(devices) = ctx.devices {
                devices.opened(ctx.table.get(&device)?);
            }
            Ok(Ok(device))
        })
    }
}

impl<'a> WasiWebGpuCtx<'a> {
    /// Submits `command_buffers` on `queue` now.
    fn submit(
        &mut self,
        queue: &Resource<webgpu::GpuQueue>,
        command_buffers: &[Resource<webgpu::GpuCommandBuffer>],
    ) -> wasmtime::Result<()> {
        let command_buffers = command_buffers
            .iter()
            .map(|buffer| {
                self.table
                    .get(buffer)
                    .map(|command_buffer| command_buffer.id())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let queue = self.table.get(queue)?;
        // A submission's validation errors are the device's to report, through its error
        // scopes or its uncaptured-error event: https://www.w3.org/TR/webgpu/#dom-gpuqueue-submit
        if let Err((_, error)) = self.instance.queue_submit(queue.id(), &command_buffers) {
            queue.error_handler.handle_possible_error(Some(error));
        }
        Ok(())
    }
}

impl<'a> webgpu::HostGpuQueue for WasiWebGpuCtx<'a> {
    fn write_buffer_with_copy(
        &mut self,
        queue: Resource<webgpu::GpuQueue>,
        buffer: Resource<webgpu::GpuBuffer>,
        buffer_offset: webgpu::GpuSize64,
        data: Vec<u8>,
        data_offset: Option<webgpu::GpuSize64>,
        size: Option<webgpu::GpuSize64>,
    ) -> wasmtime::Result<Result<(), webgpu::WriteBufferError>> {
        let queue = self.table.get(&queue)?;
        let buffer_id = self.table.get(&buffer)?.id();
        let Some(data) = bytes_to_write(&data, data_offset, size) else {
            // https://www.w3.org/TR/webgpu/#dom-gpuqueue-writebuffer
            return Ok(Err(webgpu::WriteBufferError {
                kind: webgpu::WriteBufferErrorKind::OperationError,
                message: "the data offset and size fall outside the data".to_owned(),
            }));
        };
        if let Err(error) =
            self.instance
                .queue_write_buffer(queue.id(), buffer_id, buffer_offset, data)
        {
            queue.error_handler.handle_possible_error(Some(error));
        }
        Ok(Ok(()))
    }

    fn write_texture_with_copy(
        &mut self,
        queue: Resource<webgpu::GpuQueue>,
        destination: webgpu::GpuTexelCopyTextureInfo,
        data: Vec<u8>,
        data_layout: webgpu::GpuTexelCopyBufferLayout,
        size: webgpu::GpuExtent3D,
    ) -> wasmtime::Result<()> {
        let queue = self.table.get(&queue)?;
        let written = self.instance.queue_write_texture(
            queue.id(),
            &destination.to_core(self.table),
            &data,
            &data_layout.to_core(self.table),
            &size.to_core(self.table),
        );
        if let Err(error) = written {
            queue.error_handler.handle_possible_error(Some(error));
        }
        Ok(())
    }

    fn label(&mut self, object: Resource<webgpu::GpuQueue>) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<webgpu::GpuQueue>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn drop(&mut self, queue: Resource<webgpu::GpuQueue>) -> wasmtime::Result<()> {
        self.table.delete(queue)?;
        Ok(())
    }
}

/// The part of `data` a `write-buffer` writes: from `data_offset`, `size` bytes, or `None` when
/// that falls outside the data.
fn bytes_to_write(
    data: &[u8],
    data_offset: Option<webgpu::GpuSize64>,
    size: Option<webgpu::GpuSize64>,
) -> Option<&[u8]> {
    let data = data.get(usize::try_from(data_offset.unwrap_or(0)).ok()?..)?;
    match size {
        Some(size) => data.get(..usize::try_from(size).ok()?),
        None => Some(data),
    }
}

impl<T: Send> webgpu::HostGpuQueueWithStore<T> for crate::HasWasiWebGpuCtx {
    /// Waits while the embedder holds submissions, then submits inside the gate, so a hold
    /// that came meanwhile refuses the submission and the wait starts again. The program's
    /// store waits with it: to the program this is a plain call.
    async fn submit(
        mut host: Access<'_, T, Self>,
        queue: Resource<webgpu::GpuQueue>,
        command_buffers: Vec<Resource<webgpu::GpuCommandBuffer>>,
    ) -> wasmtime::Result<()> {
        loop {
            let allowed = host
                .get()
                .submissions
                .map(|submissions| submissions.until_allowed());
            if let Some(allowed) = allowed {
                allowed.await;
            }
            let mut ctx = host.get();
            let Some(gate) = ctx.submissions else {
                return ctx.submit(&queue, &command_buffers);
            };
            let mut submitted = Ok(());
            if gate.while_allowed(&mut || submitted = ctx.submit(&queue, &command_buffers)) {
                return submitted;
            }
        }
    }

    async fn on_submitted_work_done(
        accessor: &Accessor<T, Self>,
        queue: Resource<webgpu::GpuQueue>,
    ) -> wasmtime::Result<()> {
        // Resolves once everything submitted so far is done. Nothing else polls the device for
        // the program, so it is waited on here, off the program's thread.
        let (instance, device_id, done) = accessor.with(|mut access| -> wasmtime::Result<_> {
            let ctx = access.get();
            let instance = Arc::clone(ctx.instance);
            let queue = ctx.table.get(&queue)?;
            let (done, told_done) = oneshot::channel();
            instance.queue_on_submitted_work_done(
                queue.id(),
                Box::new(move || {
                    // A program that stopped waiting dropped the other end.
                    let _ = done.send(());
                }),
            );
            Ok((instance, queue.device.id(), told_done))
        })?;
        wait_for_device(instance, device_id, "a queue's submitted work").await;
        // Never an error: the wait above ran the callback, or the device is lost and dropped it.
        let _ = done.await;
        Ok(())
    }
}

impl<'a> webgpu::HostGpuUncapturedErrorEvent for WasiWebGpuCtx<'a> {
    // fn new(
    //     &mut self,
    //     _type_: String,
    //     _gpu_uncaptured_error_event_init_dict: webgpu::GpuUncapturedErrorEventInit,
    // ) -> Resource<webgpu::GpuUncapturedErrorEvent> {
    //     todo!()
    // }

    fn error(
        &mut self,
        _self_: Resource<webgpu::GpuUncapturedErrorEvent>,
    ) -> wasmtime::Result<Resource<webgpu::GpuError>> {
        todo!()
    }

    fn drop(&mut self, _error: Resource<webgpu::GpuUncapturedErrorEvent>) -> wasmtime::Result<()> {
        todo!()
    }
}
// impl<'a> webgpu::HostGpuInternalError for WasiWebGpu<'a> {
//     fn new(&mut self, _message: String) -> Resource<webgpu::GpuInternalError> {
//         todo!()
//     }

//     fn message(&mut self, _self_: Resource<webgpu::GpuInternalError>) -> String {
//         todo!()
//     }

//     fn drop(&mut self, error: Resource<webgpu::GpuInternalError>) -> wasmtime::Result<()> {
//         self.table.delete(error)?;
//         Ok(())
//     }
// }
// impl<'a> webgpu::HostGpuOutOfMemoryError for WasiWebGpu<'a> {
//     fn new(&mut self, _message: String) -> Resource<webgpu::GpuOutOfMemoryError> {
//         todo!()
//     }

//     fn message(&mut self, _self_: Resource<webgpu::GpuOutOfMemoryError>) -> String {
//         todo!()
//     }

//     fn drop(&mut self, error: Resource<webgpu::GpuOutOfMemoryError>) -> wasmtime::Result<()> {
//         self.table.delete(error)?;
//         Ok(())
//     }
// }
// impl<'a> webgpu::HostGpuValidationError for WasiWebGpu<'a> {
//     fn new(&mut self, _message: String) -> Resource<webgpu::GpuValidationError> {
//         todo!()
//     }

//     fn message(&mut self, _self_: Resource<webgpu::GpuValidationError>) -> String {
//         todo!()
//     }

//     fn drop(&mut self, error: Resource<webgpu::GpuValidationError>) -> wasmtime::Result<()> {
//         self.table.delete(error)?;
//         Ok(())
//     }
// }
impl<'a> webgpu::HostGpuError for WasiWebGpuCtx<'a> {
    fn message(&mut self, error: Resource<webgpu::GpuError>) -> wasmtime::Result<String> {
        let error = self.table.get(&error)?;
        Ok(error.message.clone())
    }

    fn kind(
        &mut self,
        error: Resource<webgpu::GpuError>,
    ) -> wasmtime::Result<webgpu::GpuErrorKind> {
        let error = self.table.get(&error)?;
        Ok(error.kind)
    }

    fn drop(&mut self, _error: Resource<webgpu::GpuError>) -> wasmtime::Result<()> {
        self.table.delete(_error)?;
        Ok(())
    }
}
impl<'a> webgpu::HostGpuDeviceLostInfo for WasiWebGpuCtx<'a> {
    fn reason(
        &mut self,
        info: Resource<webgpu::GpuDeviceLostInfo>,
    ) -> wasmtime::Result<webgpu::GpuDeviceLostReason> {
        Ok(self.table.get(&info)?.reason.into())
    }

    fn message(&mut self, info: Resource<webgpu::GpuDeviceLostInfo>) -> wasmtime::Result<String> {
        Ok(self.table.get(&info)?.message.clone())
    }

    fn drop(&mut self, info: Resource<webgpu::GpuDeviceLostInfo>) -> wasmtime::Result<()> {
        self.table.delete(info)?;
        Ok(())
    }
}
impl<'a> webgpu::HostGpuRenderBundle for WasiWebGpuCtx<'a> {
    fn label(&mut self, object: Resource<webgpu::GpuRenderBundle>) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<webgpu::GpuRenderBundle>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn drop(&mut self, bundle: Resource<webgpu::GpuRenderBundle>) -> wasmtime::Result<()> {
        self.table.delete(bundle)?;
        Ok(())
    }
}
// impl<'a> webgpu::HostGpuPipelineError for WasiWebGpu<'a> {
//     fn new(
//         &mut self,
//         _message: Option<String>,
//         _options: webgpu::GpuPipelineErrorInit,
//     ) -> Resource<webgpu::GpuPipelineError> {
//         todo!()
//     }

//     fn reason(
//         &mut self,
//         _self_: Resource<webgpu::GpuPipelineError>,
//     ) -> webgpu::GpuPipelineErrorReason {
//         todo!()
//     }

//     fn drop(&mut self, error: Resource<webgpu::GpuPipelineError>) -> wasmtime::Result<()> {
//         self.table.delete(error)?;
//         Ok(())
//     }
// }
impl<'a> webgpu::HostGpuQuerySet for WasiWebGpuCtx<'a> {
    fn destroy(&mut self, query_set: Resource<webgpu::GpuQuerySet>) -> wasmtime::Result<()> {
        let query_set = self.table.get(&query_set)?.id();
        self.instance.query_set_destroy(query_set);
        Ok(())
    }

    fn type_(
        &mut self,
        query_set: Resource<webgpu::GpuQuerySet>,
    ) -> wasmtime::Result<webgpu::GpuQueryType> {
        Ok(self.table.get(&query_set)?.type_)
    }

    fn count(
        &mut self,
        query_set: Resource<webgpu::GpuQuerySet>,
    ) -> wasmtime::Result<webgpu::GpuSize32Out> {
        Ok(self.table.get(&query_set)?.count)
    }

    fn label(&mut self, object: Resource<webgpu::GpuQuerySet>) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<webgpu::GpuQuerySet>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn drop(&mut self, query_set: Resource<webgpu::GpuQuerySet>) -> wasmtime::Result<()> {
        self.table.delete(query_set)?;
        Ok(())
    }
}
impl<'a> webgpu::HostGpuComputePipeline for WasiWebGpuCtx<'a> {
    fn label(&mut self, object: Resource<webgpu::GpuComputePipeline>) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<webgpu::GpuComputePipeline>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn get_bind_group_layout(
        &mut self,
        compute_pipeline: Resource<webgpu::GpuComputePipeline>,
        index: u32,
    ) -> wasmtime::Result<Resource<webgpu::GpuBindGroupLayout>> {
        let pipeline = self.table.get(&compute_pipeline)?;
        let pipeline_id = pipeline.id();
        let error_handler = Arc::clone(&pipeline.error_handler);
        let (bind_group_layout, err) =
            self.instance
                .compute_pipeline_get_bind_group_layout(pipeline_id, index, None);
        error_handler.handle_possible_error(err);
        Ok(self
            .table
            .push(GpuObject::new(bind_group_layout, self.instance))?)
    }

    fn drop(&mut self, pipeline: Resource<webgpu::GpuComputePipeline>) -> wasmtime::Result<()> {
        self.table.delete(pipeline)?;
        Ok(())
    }
}
impl<'a> webgpu::HostGpuBindGroup for WasiWebGpuCtx<'a> {
    fn label(&mut self, object: Resource<webgpu::GpuBindGroup>) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<webgpu::GpuBindGroup>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn drop(&mut self, bind_group: Resource<webgpu::GpuBindGroup>) -> wasmtime::Result<()> {
        self.table.delete(bind_group)?;
        Ok(())
    }
}
impl<'a> webgpu::HostGpuPipelineLayout for WasiWebGpuCtx<'a> {
    fn label(&mut self, object: Resource<webgpu::GpuPipelineLayout>) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<webgpu::GpuPipelineLayout>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn drop(&mut self, layout: Resource<webgpu::GpuPipelineLayout>) -> wasmtime::Result<()> {
        self.table.delete(layout)?;
        Ok(())
    }
}
impl<'a> webgpu::HostGpuBindGroupLayout for WasiWebGpuCtx<'a> {
    fn label(&mut self, object: Resource<webgpu::GpuBindGroupLayout>) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<webgpu::GpuBindGroupLayout>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn drop(&mut self, layout: Resource<webgpu::GpuBindGroupLayout>) -> wasmtime::Result<()> {
        self.table.delete(layout)?;
        Ok(())
    }
}

impl<'a> webgpu::HostGpuSampler for WasiWebGpuCtx<'a> {
    fn label(&mut self, object: Resource<webgpu::GpuSampler>) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<webgpu::GpuSampler>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn drop(&mut self, sampler: Resource<webgpu::GpuSampler>) -> wasmtime::Result<()> {
        self.table.delete(sampler)?;
        Ok(())
    }
}

impl<'a> webgpu::HostGpuBuffer for WasiWebGpuCtx<'a> {
    fn size(
        &mut self,
        buffer: Resource<webgpu::GpuBuffer>,
    ) -> wasmtime::Result<webgpu::GpuSize64Out> {
        let buffer = self.table.get(&buffer)?;
        Ok(buffer.size)
    }

    fn usage(
        &mut self,
        buffer: Resource<webgpu::GpuBuffer>,
    ) -> wasmtime::Result<webgpu::GpuBufferUsage> {
        let buffer = self.table.get(&buffer)?;
        // The usage was the program's, so every flag of it has a WebGPU name.
        buffer.usage.try_into()
    }

    fn map_state(
        &mut self,
        buffer: Resource<webgpu::GpuBuffer>,
    ) -> wasmtime::Result<webgpu::GpuBufferMapState> {
        let buffer = self.table.get(&buffer)?;
        Ok(buffer.map_state())
    }

    fn get_mapped_range_get_with_copy(
        &mut self,
        buffer: Resource<webgpu::GpuBuffer>,
        offset: Option<webgpu::GpuSize64>,
        size: Option<webgpu::GpuSize64>,
    ) -> wasmtime::Result<Result<Vec<u8>, webgpu::GetMappedRangeError>> {
        let buffer = self.table.get(&buffer)?;
        let (ptr, len) = match mapped_range(self.instance, buffer, offset, size) {
            Ok(range) => range,
            Err(error) => return Ok(Err(error)),
        };
        let data = unsafe { slice::from_raw_parts(ptr.as_ptr(), len as usize) };
        let mut output = vec![0; len as usize];
        output.copy_from_slice(data);
        Ok(Ok(output))
    }

    fn get_mapped_range_set_with_copy(
        &mut self,
        buffer: Resource<webgpu::GpuBuffer>,
        data: Vec<u8>,
        offset: Option<webgpu::GpuSize64>,
        size: Option<webgpu::GpuSize64>,
    ) -> wasmtime::Result<Result<(), webgpu::GetMappedRangeError>> {
        let buffer = self.table.get(&buffer)?;
        let (ptr, len) = match mapped_range(self.instance, buffer, offset, size) {
            Ok(range) => range,
            Err(error) => return Ok(Err(error)),
        };
        if data.len() as u64 != len {
            return Ok(Err(webgpu::GetMappedRangeError {
                kind: webgpu::GetMappedRangeErrorKind::RangeError,
                message: format!(
                    "{} bytes were given for a mapped range of {len}",
                    data.len()
                ),
            }));
        }
        let buffer = unsafe { slice::from_raw_parts_mut(ptr.as_ptr(), len as usize) };
        buffer.copy_from_slice(&data);
        Ok(Ok(()))
    }

    fn unmap(
        &mut self,
        buffer: Resource<webgpu::GpuBuffer>,
    ) -> wasmtime::Result<Result<(), webgpu::UnmapError>> {
        let buffer = self.table.get_mut(&buffer)?;
        if let Err(error) = self.instance.buffer_unmap(buffer.id()) {
            buffer
                .device
                .error_handler
                .handle_possible_error(Some(error));
        }
        buffer.set_map_state(webgpu::GpuBufferMapState::Unmapped);
        Ok(Ok(()))
    }

    fn destroy(&mut self, buffer: Resource<webgpu::GpuBuffer>) -> wasmtime::Result<()> {
        let buffer_id = self.table.get_mut(&buffer)?.id();
        self.instance.buffer_destroy(buffer_id);
        Ok(())
    }

    fn label(&mut self, object: Resource<webgpu::GpuBuffer>) -> wasmtime::Result<String> {
        self.label_of(&object)
    }

    fn set_label(
        &mut self,
        object: Resource<webgpu::GpuBuffer>,
        label: String,
    ) -> wasmtime::Result<()> {
        self.relabel(&object, label)
    }

    fn drop(&mut self, buffer: Resource<webgpu::GpuBuffer>) -> wasmtime::Result<()> {
        self.table.delete(buffer)?;
        Ok(())
    }
}

/// The range of `buffer` the program asked for, as mapped, or the `OperationError` WebGPU throws
/// when it cannot have it: the buffer is not mapped, the range is not within the mapping, or the
/// buffer is destroyed, as destroying its device destroys it.
/// https://www.w3.org/TR/webgpu/#dom-gpubuffer-getmappedrange
fn mapped_range(
    instance: &wgpu_core::global::Global,
    buffer: &Buffer,
    offset: Option<webgpu::GpuSize64>,
    size: Option<webgpu::GpuSize64>,
) -> Result<(std::ptr::NonNull<u8>, u64), webgpu::GetMappedRangeError> {
    let unavailable = |message: String| webgpu::GetMappedRangeError {
        kind: webgpu::GetMappedRangeErrorKind::OperationError,
        message,
    };
    if buffer.map_state() != webgpu::GpuBufferMapState::Mapped {
        return Err(unavailable("the buffer is not mapped".to_owned()));
    }
    instance
        .buffer_get_mapped_range(buffer.id(), offset.unwrap_or(0), size)
        .map_err(|error| unavailable(error.to_string()))
}

/// Why a mapping failed, as WebGPU rejects it: `AbortError` when no mapped data can be given,
/// because the buffer's device is lost, the buffer is invalid, or `unmap` cancelled the mapping;
/// otherwise `OperationError`, with the validation error reported to the buffer's device.
/// https://www.w3.org/TR/webgpu/#dom-gpubuffer-mapasync
fn map_failure(
    buffer: &Buffer,
    error: wgpu_core::resource::BufferAccessError,
) -> webgpu::MapAsyncError {
    use wgpu_core::resource::BufferAccessError;
    use wgpu_types::error::{ErrorType, WebGpuError};

    let message = error.to_string();
    let aborted = buffer.device.is_lost()
        || error.webgpu_error_type() == ErrorType::DeviceLost
        || matches!(
            error,
            BufferAccessError::InvalidResource(_) | BufferAccessError::MapAborted
        );
    let kind = if aborted {
        webgpu::MapAsyncErrorKind::AbortError
    } else {
        buffer
            .device
            .error_handler
            .handle_possible_error(Some(error));
        webgpu::MapAsyncErrorKind::OperationError
    };
    webgpu::MapAsyncError { kind, message }
}

impl<T: Send> webgpu::HostGpuBufferWithStore<T> for crate::HasWasiWebGpuCtx {
    async fn map_async(
        accessor: &Accessor<T, Self>,
        buffer: Resource<webgpu::GpuBuffer>,
        mode: webgpu::GpuMapMode,
        offset: Option<webgpu::GpuSize64>,
        size: Option<webgpu::GpuSize64>,
    ) -> wasmtime::Result<Result<(), webgpu::MapAsyncError>> {
        // source: https://www.w3.org/TR/webgpu/#typedefdef-gpumapmodeflags
        // from the spec
        // > 3. If any of the following conditions are unsatisfied:
        // >     - mode contains exactly one of READ or WRITE.
        // >   Then:
        // >     3. Generate a validation error.

        let mode = if mode == webgpu::GpuMapMode::READ {
            wgpu_core::device::HostMap::Read
        } else if mode == webgpu::GpuMapMode::WRITE {
            wgpu_core::device::HostMap::Write
        } else {
            return accessor.with(|mut access| -> wasmtime::Result<_> {
                let message = format!("{mode:?} is not exactly one of READ and WRITE");
                let buffer = access.get().table.get(&buffer)?;
                buffer
                    .device
                    .error_handler
                    .report_validation_error(message.clone());
                Ok(Err(webgpu::MapAsyncError {
                    kind: webgpu::MapAsyncErrorKind::OperationError,
                    message,
                }))
            });
        };

        // https://www.w3.org/TR/webgpu/#gpubuffer
        let offset = offset.unwrap_or(0);

        let (instance, device_id, mapped) = accessor.with(|mut access| -> wasmtime::Result<_> {
            let ctx = access.get();
            let instance = Arc::clone(ctx.instance);
            let buffer = ctx.table.get_mut(&buffer)?;
            let device_id = buffer.device.device_id();
            buffer.set_map_state(webgpu::GpuBufferMapState::Pending);
            let (answer, mapped) = oneshot::channel();
            let op = wgpu_core::resource::BufferMapOperation {
                host: mode,
                callback: Some(Box::new(move |result| {
                    // A program that stopped waiting dropped the other end.
                    let _ = answer.send(result);
                })),
            };
            // A mapping that cannot start says why through its callback as well.
            let _ = instance.buffer_map_async(buffer.id(), offset, size, op);
            Ok((instance, device_id, mapped))
        })?;
        wait_for_device(instance, device_id, "a buffer's mapping").await;
        // The wait above ran the callback; a device lost meanwhile dropped it unanswered.
        let mapped = mapped
            .await
            .unwrap_or(Err(wgpu_core::resource::BufferAccessError::MapAborted));

        accessor.with(|mut access| -> wasmtime::Result<_> {
            let ctx = access.get();
            let buffer = ctx.table.get_mut(&buffer)?;
            Ok(match mapped {
                Ok(()) => {
                    buffer.set_map_state(webgpu::GpuBufferMapState::Mapped);
                    Ok(())
                }
                Err(error) => {
                    buffer.set_map_state(webgpu::GpuBufferMapState::Unmapped);
                    Err(map_failure(buffer, error))
                }
            })
        })
    }
}

impl<'a> webgpu::HostGpu for WasiWebGpuCtx<'a> {
    fn get_preferred_canvas_format(
        &mut self,
        _gpu: Resource<webgpu::Gpu>,
    ) -> wasmtime::Result<webgpu::GpuTextureFormat> {
        Ok(PREFERRED_CANVAS_FORMAT)
    }

    fn wgsl_language_features(
        &mut self,
        _self_: Resource<webgpu::Gpu>,
    ) -> wasmtime::Result<Resource<webgpu::WgslLanguageFeatures>> {
        Ok(self.table.push(WgslLanguageFeatures::new())?)
    }

    fn drop(&mut self, _gpu: Resource<webgpu::Gpu>) -> wasmtime::Result<()> {
        // not actually a resource in the table
        Ok(())
    }
}

impl<T: Send> webgpu::HostGpuWithStore<T> for crate::HasWasiWebGpuCtx {
    async fn request_adapter(
        accessor: &Accessor<T, Self>,
        _gpu: Resource<webgpu::Gpu>,
        options: Option<webgpu::GpuRequestAdapterOptions>,
    ) -> wasmtime::Result<Option<Resource<webgpu::GpuAdapter>>> {
        accessor.with(|mut access: Access<'_, T, crate::HasWasiWebGpuCtx>| {
            let ctx = access.get();
            let options = options
                .map(|o| o.to_core(ctx.table))
                .unwrap_or(wgpu_types::RequestAdapterOptions::default());
            let adapter = match ctx.adapters {
                Some(adapters) => adapters.choose(ctx.instance, &options),
                None => ctx
                    .instance
                    .request_adapter(&options, wgpu_types::Backends::all(), None)
                    .map_err(|error| log::warn!("no GPU adapter: {error}"))
                    .ok(),
            };
            // WebGPU answers null when no adapter can be had, whatever the reason:
            // https://www.w3.org/TR/webgpu/#dom-gpu-requestadapter
            match adapter {
                Some(adapter) => {
                    let adapter = Arc::new(GpuObject::new(adapter, ctx.instance));
                    Ok(Some(ctx.table.push(adapter)?))
                }
                None => Ok(None),
            }
        })
    }
}

impl<'a> webgpu::HostGpuAdapterInfo for WasiWebGpuCtx<'a> {
    // TODO: more real values here
    // take ideas from https://bugzilla.mozilla.org/show_bug.cgi?id=1831994
    // keep an eye on https://github.com/gfx-rs/wgpu/issues/8649
    fn vendor(
        &mut self,
        adapter_info: Resource<webgpu::GpuAdapterInfo>,
    ) -> wasmtime::Result<String> {
        let adapter_info = self.table.get(&adapter_info)?;
        Ok(adapter_info.vendor.to_string())
    }

    fn architecture(
        &mut self,
        _adapter_info: Resource<webgpu::GpuAdapterInfo>,
    ) -> wasmtime::Result<String> {
        // TODO: implement real architecture
        Ok(String::new())
    }

    fn device(
        &mut self,
        adapter_info: Resource<webgpu::GpuAdapterInfo>,
    ) -> wasmtime::Result<String> {
        let adapter_info = self.table.get(&adapter_info)?;
        Ok(adapter_info.device.to_string())
    }

    fn description(
        &mut self,
        _adapter_info: Resource<webgpu::GpuAdapterInfo>,
    ) -> wasmtime::Result<String> {
        // TODO: implement real description
        Ok(String::new())
    }

    fn subgroup_min_size(
        &mut self,
        adapter_info: Resource<webgpu::GpuAdapterInfo>,
    ) -> wasmtime::Result<u32> {
        let adapter_info = self.table.get(&adapter_info)?;
        Ok(adapter_info.subgroup_min_size)
    }

    fn subgroup_max_size(
        &mut self,
        adapter_info: Resource<webgpu::GpuAdapterInfo>,
    ) -> wasmtime::Result<u32> {
        let adapter_info = self.table.get(&adapter_info)?;
        Ok(adapter_info.subgroup_max_size)
    }

    fn is_fallback_adapter(
        &mut self,
        adapter_info: Resource<webgpu::GpuAdapterInfo>,
    ) -> wasmtime::Result<bool> {
        let adapter_info = self.table.get(&adapter_info)?;
        // wgpu in browser treats only cpu as fallback
        // https://github.com/gfx-rs/wgpu/blob/0d32f7e75604feeff976445576c234da377fa3df/wgpu/src/backend/webgpu.rs#L889-L893
        let is_fallback = match adapter_info.device_type {
            wgpu_types::DeviceType::IntegratedGpu
            | wgpu_types::DeviceType::DiscreteGpu
            | wgpu_types::DeviceType::VirtualGpu
            | wgpu_types::DeviceType::Other => false,
            wgpu_types::DeviceType::Cpu => true,
        };
        Ok(is_fallback)
    }

    fn drop(&mut self, info: Resource<webgpu::GpuAdapterInfo>) -> wasmtime::Result<()> {
        self.table.delete(info)?;
        Ok(())
    }
}
impl<'a> webgpu::HostWgslLanguageFeatures for WasiWebGpuCtx<'a> {
    fn has(
        &mut self,
        features: Resource<webgpu::WgslLanguageFeatures>,
        key: String,
    ) -> wasmtime::Result<bool> {
        let features = self.table.get(&features)?;
        Ok(features.has(&key))
    }

    fn drop(&mut self, features: Resource<webgpu::WgslLanguageFeatures>) -> wasmtime::Result<()> {
        self.table.delete(features)?;
        Ok(())
    }
}
impl<'a> webgpu::HostGpuSupportedFeatures for WasiWebGpuCtx<'a> {
    fn has(
        &mut self,
        features: Resource<webgpu::GpuSupportedFeatures>,
        query: String,
    ) -> wasmtime::Result<bool> {
        let features = self.table.get(&features)?;
        // TODO: disable the ones not present in the wgpu yet.
        Ok(match query.as_str() {
            "core-features-and-limits" => {
                // TODO: enable once wgpu does
                // features.contains(wgpu_types::Features::CORE_FEATURES_AND_LIMITS)
                false
            }
            "depth-clip-control" => features.contains(wgpu_types::Features::DEPTH_CLIP_CONTROL),
            "depth32float-stencil8" => {
                features.contains(wgpu_types::Features::DEPTH32FLOAT_STENCIL8)
            }
            "texture-compression-bc" => {
                features.contains(wgpu_types::Features::TEXTURE_COMPRESSION_BC)
            }
            "texture-compression-bc-sliced-3d" => {
                features.contains(wgpu_types::Features::TEXTURE_COMPRESSION_BC_SLICED_3D)
            }
            "texture-compression-etc2" => {
                features.contains(wgpu_types::Features::TEXTURE_COMPRESSION_ETC2)
            }
            "texture-compression-astc" => {
                features.contains(wgpu_types::Features::TEXTURE_COMPRESSION_ASTC)
            }
            "texture-compression-astc-sliced-3d" => {
                features.contains(wgpu_types::Features::TEXTURE_COMPRESSION_ASTC_SLICED_3D)
            }
            "timestamp-query" => features.contains(wgpu_types::Features::TIMESTAMP_QUERY),
            "indirect-first-instance" => {
                features.contains(wgpu_types::Features::INDIRECT_FIRST_INSTANCE)
            }
            "shader-f16" => features.contains(wgpu_types::Features::SHADER_F16),
            "rg11b10ufloat-renderable" => {
                features.contains(wgpu_types::Features::RG11B10UFLOAT_RENDERABLE)
            }
            "bgra8unorm-storage" => features.contains(wgpu_types::Features::BGRA8UNORM_STORAGE),
            "float32-filterable" => features.contains(wgpu_types::Features::FLOAT32_FILTERABLE),
            "float32-blendable" => features.contains(wgpu_types::Features::FLOAT32_BLENDABLE),
            "clip-distances" => features.contains(wgpu_types::Features::CLIP_DISTANCES),
            "dual-source-blending" => features.contains(wgpu_types::Features::DUAL_SOURCE_BLENDING),
            "subgroups" => {
                // TODO: enable once wgpu does
                // features.contains(wgpu_types::Features::SUBGROUPS)
                false
            }
            "texture-formats-tier1" => {
                // TODO: enable once wgpu does
                // features.contains(wgpu_types::Features::TEXTURE_FORMATS_TIER1)
                false
            }
            "texture-formats-tier2" => {
                // TODO: enable once wgpu does
                // features.contains(wgpu_types::Features::TEXTURE_FORMATS_TIER2)
                false
            }
            "primitive-index" => features.contains(wgpu_types::Features::PRIMITIVE_INDEX),
            "texture-component-swizzle" => {
                // TODO: enable once wgpu does
                // features.contains(wgpu_types::Features::TEXTURE_COMPONENT_SWIZZLE)
                false
            }
            name => {
                log::warn!("unknown feature name: {}", name);
                false
            }
        })
    }

    fn drop(&mut self, features: Resource<webgpu::GpuSupportedFeatures>) -> wasmtime::Result<()> {
        self.table.delete(features)?;
        Ok(())
    }
}
impl<'a> webgpu::HostGpuSupportedLimits for WasiWebGpuCtx<'a> {
    fn max_texture_dimension1_d(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_texture_dimension_1d)
    }

    fn max_texture_dimension2_d(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_texture_dimension_2d)
    }

    fn max_texture_dimension3_d(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_texture_dimension_3d)
    }

    fn max_texture_array_layers(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_texture_array_layers)
    }

    fn max_bind_groups(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_bind_groups)
    }

    fn max_bind_groups_plus_vertex_buffers(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_bind_groups_plus_vertex_buffers)
    }

    fn max_bindings_per_bind_group(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_bindings_per_bind_group)
    }

    fn max_dynamic_uniform_buffers_per_pipeline_layout(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_dynamic_uniform_buffers_per_pipeline_layout)
    }

    fn max_dynamic_storage_buffers_per_pipeline_layout(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_dynamic_storage_buffers_per_pipeline_layout)
    }

    fn max_sampled_textures_per_shader_stage(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_sampled_textures_per_shader_stage)
    }

    fn max_samplers_per_shader_stage(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_samplers_per_shader_stage)
    }

    fn max_storage_buffers_per_shader_stage(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_storage_buffers_per_shader_stage)
    }

    fn max_storage_textures_per_shader_stage(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_storage_textures_per_shader_stage)
    }

    fn max_uniform_buffers_per_shader_stage(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_uniform_buffers_per_shader_stage)
    }

    fn max_uniform_buffer_binding_size(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u64> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_uniform_buffer_binding_size)
    }

    fn max_storage_buffer_binding_size(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u64> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_storage_buffer_binding_size)
    }

    fn min_uniform_buffer_offset_alignment(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.min_uniform_buffer_offset_alignment)
    }

    fn min_storage_buffer_offset_alignment(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.min_storage_buffer_offset_alignment)
    }

    fn max_vertex_buffers(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_vertex_buffers)
    }

    fn max_buffer_size(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u64> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_buffer_size)
    }

    fn max_vertex_attributes(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_vertex_attributes)
    }

    fn max_vertex_buffer_array_stride(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_vertex_buffer_array_stride)
    }

    fn max_inter_stage_shader_variables(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_inter_stage_shader_variables)
    }

    fn max_color_attachments(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_color_attachments)
    }

    fn max_color_attachment_bytes_per_sample(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_color_attachment_bytes_per_sample)
    }

    fn max_compute_workgroup_storage_size(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_compute_workgroup_storage_size)
    }

    fn max_compute_invocations_per_workgroup(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_compute_invocations_per_workgroup)
    }

    fn max_compute_workgroup_size_x(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_compute_workgroup_size_x)
    }

    fn max_compute_workgroup_size_y(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_compute_workgroup_size_y)
    }

    fn max_compute_workgroup_size_z(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_compute_workgroup_size_z)
    }

    fn max_compute_workgroups_per_dimension(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_compute_workgroups_per_dimension)
    }

    fn max_immediate_size(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        let limits = self.table.get(&limits)?;
        Ok(limits.max_immediate_size)
    }

    fn max_storage_buffers_in_vertex_stage(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        // wgpu has no limit per stage; in core WebGPU it is the limit for every stage.
        // https://www.w3.org/TR/webgpu/#dom-supported-limits-maxstoragebuffersinvertexstage
        let limits = self.table.get(&limits)?;
        Ok(limits.max_storage_buffers_per_shader_stage)
    }

    fn max_storage_buffers_in_fragment_stage(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        // wgpu has no limit per stage; in core WebGPU it is the limit for every stage.
        // https://www.w3.org/TR/webgpu/#dom-supported-limits-maxstoragebuffersinfragmentstage
        let limits = self.table.get(&limits)?;
        Ok(limits.max_storage_buffers_per_shader_stage)
    }

    fn max_storage_textures_in_vertex_stage(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        // wgpu has no limit per stage; in core WebGPU it is the limit for every stage.
        // https://www.w3.org/TR/webgpu/#dom-supported-limits-maxstoragetexturesinvertexstage
        let limits = self.table.get(&limits)?;
        Ok(limits.max_storage_textures_per_shader_stage)
    }

    fn max_storage_textures_in_fragment_stage(
        &mut self,
        limits: Resource<webgpu::GpuSupportedLimits>,
    ) -> wasmtime::Result<u32> {
        // wgpu has no limit per stage; in core WebGPU it is the limit for every stage.
        // https://www.w3.org/TR/webgpu/#dom-supported-limits-maxstoragetexturesinfragmentstage
        let limits = self.table.get(&limits)?;
        Ok(limits.max_storage_textures_per_shader_stage)
    }

    fn drop(&mut self, limits: Resource<webgpu::GpuSupportedLimits>) -> wasmtime::Result<()> {
        self.table.delete(limits)?;
        Ok(())
    }
}

/// Waits until `device` has done the work submitted to it, which runs the callbacks that work
/// answers, a mapping's and submitted work's: `what` is waited for, for the log. The wait is on a
/// thread of its own, so the program's thread goes on with its other work meanwhile, as it does
/// for every other wait of the engine's; nothing else polls a program's device.
async fn wait_for_device(
    instance: Arc<wgpu_core::global::Global>,
    device: wgpu_core::id::DeviceId,
    what: &'static str,
) {
    let waited = spawn_blocking(move || {
        instance.device_poll(device, wgpu_types::PollType::wait_indefinitely())
    })
    .await;
    if let Err(error) = waited {
        tracing::warn!("waiting for {what}: {error}");
    }
}
