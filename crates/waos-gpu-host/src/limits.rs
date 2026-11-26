//! `limits`: wgpu-types' `Limits` itself, a getter and a setter per limit the package names, and
//! the limits a device is opened with.
//!
//! A device has the limits asked, as wgpu gives them, but for one adapter that is not wgpu's: one
//! that falls below wgpu's defaults, as the iOS simulator's Metal does with 15 inter-stage shader
//! variables against 16. A limit asked no better than the default, where the adapter falls short
//! of what is asked, is the adapter's own, so a program that asks for wgpu's defaults, as most
//! wgpu programs do, still draws there; one whose shader needs more fails at its pipeline with
//! wgpu-core's validation error. A limit asked past the default and past the adapter's is refused.
//!
//! The invariant: every limit the package names is a row of `limits!`, which writes its getter,
//! its setter and its part in [`fitted`]; a limit wgpu adds and the package takes is one row more.

use std::cmp::Ordering;

use wasmtime::component::Resource;
use wgpu_types::Limits;

use crate::GpuCtx;
use crate::wasdk::gpu::types::{self, HostLimits};

/// Whether a larger value of a limit is better, or a smaller one: an alignment.
#[derive(Clone, Copy)]
enum Better {
    Larger,
    Smaller,
}

impl Better {
    /// Whether `value` is better than `than`.
    fn is<T: Ord>(self, value: T, than: T) -> bool {
        let better = match self {
            Better::Larger => Ordering::Greater,
            Better::Smaller => Ordering::Less,
        };
        value.cmp(&than) == better
    }
}

macro_rules! limits {
    ($($getter:ident, $setter:ident => $field:ident: $ty:ty, $better:ident;)*) => {
        impl types::Host for GpuCtx<'_> {}

        impl HostLimits for GpuCtx<'_> {
            fn new(&mut self) -> wasmtime::Result<Resource<Limits>> {
                Ok(self.table.push(Limits::default())?)
            }

            $(
                fn $getter(&mut self, limits: Resource<Limits>) -> wasmtime::Result<$ty> {
                    Ok(self.table.get(&limits)?.$field)
                }

                fn $setter(&mut self, limits: Resource<Limits>, value: $ty) -> wasmtime::Result<()> {
                    self.table.get_mut(&limits)?.$field = value;
                    Ok(())
                }
            )*

            fn drop(&mut self, limits: Resource<Limits>) -> wasmtime::Result<()> {
                self.table.delete(limits)?;
                Ok(())
            }
        }

        /// The limits a device opened on an adapter with `adapter` asks with `asked` gets: those
        /// asked, but for each asked no better than wgpu's default where the adapter falls
        /// short, which is the adapter's.
        pub(crate) fn fitted(asked: &Limits, adapter: &Limits) -> Limits {
            let defaults = Limits::default();
            let mut fitted = asked.clone();
            $(
                let better = Better::$better;
                if !better.is(asked.$field, defaults.$field) && better.is(asked.$field, adapter.$field) {
                    tracing::warn!(
                        limit = stringify!($field),
                        asked = asked.$field,
                        adapter = adapter.$field,
                        "the adapter falls below wgpu's default limit; the device has the adapter's"
                    );
                    fitted.$field = adapter.$field;
                }
            )*
            fitted
        }
    };
}

limits! {
    max_texture_dimension1d, set_max_texture_dimension1d => max_texture_dimension_1d: u32, Larger;
    max_texture_dimension2d, set_max_texture_dimension2d => max_texture_dimension_2d: u32, Larger;
    max_texture_dimension3d, set_max_texture_dimension3d => max_texture_dimension_3d: u32, Larger;
    max_texture_array_layers, set_max_texture_array_layers => max_texture_array_layers: u32, Larger;
    max_bind_groups, set_max_bind_groups => max_bind_groups: u32, Larger;
    max_bind_groups_plus_vertex_buffers, set_max_bind_groups_plus_vertex_buffers =>
        max_bind_groups_plus_vertex_buffers: u32, Larger;
    max_bindings_per_bind_group, set_max_bindings_per_bind_group =>
        max_bindings_per_bind_group: u32, Larger;
    max_dynamic_uniform_buffers_per_pipeline_layout,
        set_max_dynamic_uniform_buffers_per_pipeline_layout =>
        max_dynamic_uniform_buffers_per_pipeline_layout: u32, Larger;
    max_dynamic_storage_buffers_per_pipeline_layout,
        set_max_dynamic_storage_buffers_per_pipeline_layout =>
        max_dynamic_storage_buffers_per_pipeline_layout: u32, Larger;
    max_sampled_textures_per_shader_stage, set_max_sampled_textures_per_shader_stage =>
        max_sampled_textures_per_shader_stage: u32, Larger;
    max_samplers_per_shader_stage, set_max_samplers_per_shader_stage =>
        max_samplers_per_shader_stage: u32, Larger;
    max_storage_buffers_per_shader_stage, set_max_storage_buffers_per_shader_stage =>
        max_storage_buffers_per_shader_stage: u32, Larger;
    max_storage_textures_per_shader_stage, set_max_storage_textures_per_shader_stage =>
        max_storage_textures_per_shader_stage: u32, Larger;
    max_uniform_buffers_per_shader_stage, set_max_uniform_buffers_per_shader_stage =>
        max_uniform_buffers_per_shader_stage: u32, Larger;
    max_binding_array_elements_per_shader_stage, set_max_binding_array_elements_per_shader_stage =>
        max_binding_array_elements_per_shader_stage: u32, Larger;
    max_binding_array_sampler_elements_per_shader_stage,
        set_max_binding_array_sampler_elements_per_shader_stage =>
        max_binding_array_sampler_elements_per_shader_stage: u32, Larger;
    max_uniform_buffer_binding_size, set_max_uniform_buffer_binding_size =>
        max_uniform_buffer_binding_size: u64, Larger;
    max_storage_buffer_binding_size, set_max_storage_buffer_binding_size =>
        max_storage_buffer_binding_size: u64, Larger;
    max_vertex_buffers, set_max_vertex_buffers => max_vertex_buffers: u32, Larger;
    max_buffer_size, set_max_buffer_size => max_buffer_size: u64, Larger;
    max_vertex_attributes, set_max_vertex_attributes => max_vertex_attributes: u32, Larger;
    max_vertex_buffer_array_stride, set_max_vertex_buffer_array_stride =>
        max_vertex_buffer_array_stride: u32, Larger;
    max_inter_stage_shader_variables, set_max_inter_stage_shader_variables =>
        max_inter_stage_shader_variables: u32, Larger;
    min_uniform_buffer_offset_alignment, set_min_uniform_buffer_offset_alignment =>
        min_uniform_buffer_offset_alignment: u32, Smaller;
    min_storage_buffer_offset_alignment, set_min_storage_buffer_offset_alignment =>
        min_storage_buffer_offset_alignment: u32, Smaller;
    max_color_attachments, set_max_color_attachments => max_color_attachments: u32, Larger;
    max_color_attachment_bytes_per_sample, set_max_color_attachment_bytes_per_sample =>
        max_color_attachment_bytes_per_sample: u32, Larger;
    max_compute_workgroup_storage_size, set_max_compute_workgroup_storage_size =>
        max_compute_workgroup_storage_size: u32, Larger;
    max_compute_invocations_per_workgroup, set_max_compute_invocations_per_workgroup =>
        max_compute_invocations_per_workgroup: u32, Larger;
    max_compute_workgroup_size_x, set_max_compute_workgroup_size_x =>
        max_compute_workgroup_size_x: u32, Larger;
    max_compute_workgroup_size_y, set_max_compute_workgroup_size_y =>
        max_compute_workgroup_size_y: u32, Larger;
    max_compute_workgroup_size_z, set_max_compute_workgroup_size_z =>
        max_compute_workgroup_size_z: u32, Larger;
    max_compute_workgroups_per_dimension, set_max_compute_workgroups_per_dimension =>
        max_compute_workgroups_per_dimension: u32, Larger;
    max_immediate_size, set_max_immediate_size => max_immediate_size: u32, Larger;
    max_non_sampler_bindings, set_max_non_sampler_bindings =>
        max_non_sampler_bindings: u32, Larger;
    max_buffers_and_acceleration_structures_per_shader_stage,
        set_max_buffers_and_acceleration_structures_per_shader_stage =>
        max_buffers_and_acceleration_structures_per_shader_stage: u32, Larger;
    max_multiview_view_count, set_max_multiview_view_count =>
        max_multiview_view_count: u32, Larger;
}

/// Why `asked` cannot be had on an adapter with `adapter`, as wgpu-core words it; none where it
/// can. The host checks before an embedder's opener, which does not.
pub(crate) fn refusal(asked: &Limits, adapter: &Limits) -> Option<String> {
    let mut refusals = Vec::new();
    asked.check_limits_with_fail_fn(adapter, false, |name, asked, allowed| {
        refusals.push(format!(
            "Limit '{name}' value {asked} is better than allowed {allowed}"
        ));
    });
    (!refusals.is_empty()).then(|| refusals.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The iOS simulator's Metal adapter: wgpu's defaults but for 15 inter-stage shader
    /// variables.
    fn simulator() -> Limits {
        Limits {
            max_inter_stage_shader_variables: 15,
            ..Limits::default()
        }
    }

    #[test]
    fn wgpus_defaults_asked_of_an_adapter_below_them_give_the_adapters_value() {
        assert_eq!(fitted(&Limits::default(), &simulator()), simulator());
        assert!(refusal(&fitted(&Limits::default(), &simulator()), &simulator()).is_none());
    }

    #[test]
    fn a_limit_asked_past_the_default_and_the_adapters_is_refused() {
        let asked = Limits {
            max_inter_stage_shader_variables: 17,
            ..Limits::default()
        };
        let fitted = fitted(&asked, &simulator());
        assert_eq!(
            fitted.max_inter_stage_shader_variables, 17,
            "kept, to be refused"
        );
        assert!(refusal(&fitted, &simulator()).is_some());
    }

    #[test]
    fn an_adapter_past_the_defaults_gives_what_is_asked() {
        let generous = Limits {
            max_inter_stage_shader_variables: 31,
            ..Limits::default()
        };
        assert_eq!(fitted(&Limits::default(), &generous), Limits::default());
    }
}
