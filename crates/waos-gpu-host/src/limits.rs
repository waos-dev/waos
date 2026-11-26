//! `limits`: wgpu-types' `Limits` itself, a getter and a setter per limit the package names, and
//! the limits a set handed out carries.
//!
//! A device has the limits the program asked for, as wgpu-core gives them: a limit past the
//! adapter's is refused (`RequestDeviceError::LimitsExceeded`, worded as wgpu-core words it,
//! [`refusal`]), and none is lowered. A program that runs where an adapter falls below wgpu's
//! defaults, as the iOS simulator's Metal does with 15 inter-stage shader variables against 16,
//! asks for what the adapter has, as on native wgpu.
//!
//! A set the host hands out, an adapter's or a device's, carries only the limits the package
//! names, the rest at wgpu's defaults ([`shown`]): a program could neither read nor lower a limit
//! it is not shown, and passed on to another adapter, it would be refused on it.
//!
//! The invariant: every limit the package names is a row of `limits!`, which writes its getter,
//! its setter and its part in [`shown`]; a limit wgpu adds and the package takes is one row more.

use wasmtime::component::Resource;
use wgpu_types::Limits;

use crate::GpuCtx;
use crate::wasdk::gpu::types::{self, HostLimits};

macro_rules! limits {
    ($($getter:ident, $setter:ident => $field:ident: $ty:ty;)*) => {
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

        /// `limits` as the package shows them: the limits it names, and wgpu's defaults for the
        /// limits it leaves out, the experimental features' ones, so a set the program cannot
        /// read asks for none of them past what a set made with `limits`' constructor does.
        pub(crate) fn shown(limits: &Limits) -> Limits {
            Limits {
                $($field: limits.$field,)*
                ..Limits::default()
            }
        }
    };
}

limits! {
    max_texture_dimension1d, set_max_texture_dimension1d => max_texture_dimension_1d: u32;
    max_texture_dimension2d, set_max_texture_dimension2d => max_texture_dimension_2d: u32;
    max_texture_dimension3d, set_max_texture_dimension3d => max_texture_dimension_3d: u32;
    max_texture_array_layers, set_max_texture_array_layers => max_texture_array_layers: u32;
    max_bind_groups, set_max_bind_groups => max_bind_groups: u32;
    max_bind_groups_plus_vertex_buffers, set_max_bind_groups_plus_vertex_buffers =>
        max_bind_groups_plus_vertex_buffers: u32;
    max_bindings_per_bind_group, set_max_bindings_per_bind_group =>
        max_bindings_per_bind_group: u32;
    max_dynamic_uniform_buffers_per_pipeline_layout,
        set_max_dynamic_uniform_buffers_per_pipeline_layout =>
        max_dynamic_uniform_buffers_per_pipeline_layout: u32;
    max_dynamic_storage_buffers_per_pipeline_layout,
        set_max_dynamic_storage_buffers_per_pipeline_layout =>
        max_dynamic_storage_buffers_per_pipeline_layout: u32;
    max_sampled_textures_per_shader_stage, set_max_sampled_textures_per_shader_stage =>
        max_sampled_textures_per_shader_stage: u32;
    max_samplers_per_shader_stage, set_max_samplers_per_shader_stage =>
        max_samplers_per_shader_stage: u32;
    max_storage_buffers_per_shader_stage, set_max_storage_buffers_per_shader_stage =>
        max_storage_buffers_per_shader_stage: u32;
    max_storage_textures_per_shader_stage, set_max_storage_textures_per_shader_stage =>
        max_storage_textures_per_shader_stage: u32;
    max_uniform_buffers_per_shader_stage, set_max_uniform_buffers_per_shader_stage =>
        max_uniform_buffers_per_shader_stage: u32;
    max_binding_array_elements_per_shader_stage, set_max_binding_array_elements_per_shader_stage =>
        max_binding_array_elements_per_shader_stage: u32;
    max_binding_array_sampler_elements_per_shader_stage,
        set_max_binding_array_sampler_elements_per_shader_stage =>
        max_binding_array_sampler_elements_per_shader_stage: u32;
    max_uniform_buffer_binding_size, set_max_uniform_buffer_binding_size =>
        max_uniform_buffer_binding_size: u64;
    max_storage_buffer_binding_size, set_max_storage_buffer_binding_size =>
        max_storage_buffer_binding_size: u64;
    max_vertex_buffers, set_max_vertex_buffers => max_vertex_buffers: u32;
    max_buffer_size, set_max_buffer_size => max_buffer_size: u64;
    max_vertex_attributes, set_max_vertex_attributes => max_vertex_attributes: u32;
    max_vertex_buffer_array_stride, set_max_vertex_buffer_array_stride =>
        max_vertex_buffer_array_stride: u32;
    max_inter_stage_shader_variables, set_max_inter_stage_shader_variables =>
        max_inter_stage_shader_variables: u32;
    min_uniform_buffer_offset_alignment, set_min_uniform_buffer_offset_alignment =>
        min_uniform_buffer_offset_alignment: u32;
    min_storage_buffer_offset_alignment, set_min_storage_buffer_offset_alignment =>
        min_storage_buffer_offset_alignment: u32;
    max_color_attachments, set_max_color_attachments => max_color_attachments: u32;
    max_color_attachment_bytes_per_sample, set_max_color_attachment_bytes_per_sample =>
        max_color_attachment_bytes_per_sample: u32;
    max_compute_workgroup_storage_size, set_max_compute_workgroup_storage_size =>
        max_compute_workgroup_storage_size: u32;
    max_compute_invocations_per_workgroup, set_max_compute_invocations_per_workgroup =>
        max_compute_invocations_per_workgroup: u32;
    max_compute_workgroup_size_x, set_max_compute_workgroup_size_x =>
        max_compute_workgroup_size_x: u32;
    max_compute_workgroup_size_y, set_max_compute_workgroup_size_y =>
        max_compute_workgroup_size_y: u32;
    max_compute_workgroup_size_z, set_max_compute_workgroup_size_z =>
        max_compute_workgroup_size_z: u32;
    max_compute_workgroups_per_dimension, set_max_compute_workgroups_per_dimension =>
        max_compute_workgroups_per_dimension: u32;
    max_immediate_size, set_max_immediate_size => max_immediate_size: u32;
    max_non_sampler_bindings, set_max_non_sampler_bindings =>
        max_non_sampler_bindings: u32;
    max_buffers_and_acceleration_structures_per_shader_stage,
        set_max_buffers_and_acceleration_structures_per_shader_stage =>
        max_buffers_and_acceleration_structures_per_shader_stage: u32;
    max_multiview_view_count, set_max_multiview_view_count =>
        max_multiview_view_count: u32;
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
    fn wgpus_defaults_asked_of_an_adapter_below_them_are_refused() {
        assert!(refusal(&Limits::default(), &simulator()).is_some());
    }

    #[test]
    fn a_set_handed_out_carries_the_defaults_of_the_limits_left_out() {
        let adapter = Limits {
            max_bind_groups: 8,
            max_mesh_output_vertices: 256,
            max_blas_primitive_count: 1 << 20,
            ..Limits::default()
        };
        let shown = shown(&adapter);
        assert_eq!(shown.max_bind_groups, 8, "a limit the package names");
        assert_eq!(
            shown.max_mesh_output_vertices,
            Limits::default().max_mesh_output_vertices
        );
        assert_eq!(
            shown.max_blas_primitive_count,
            Limits::default().max_blas_primitive_count
        );
    }
}
