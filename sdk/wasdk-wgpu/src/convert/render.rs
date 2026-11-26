//! Render state: primitives, depth and stencil, blending, multisampling, vertex layouts,
//! attachments' load and store operations, and queries.

use wasdk::gpu_types as wit;

use super::{
    color_to_wit, color_writes_to_wit, enum_conversions, flag_conversions, index_format_to_wit,
    texture_format_to_wit, vertex_format_to_wit,
};

enum_conversions! {
    compare_function_to_wit, compare_function_from_wit: CompareFunction <=> CompareFunction {
        Never, Less, Equal, LessEqual, Greater, NotEqual, GreaterEqual, Always,
    }
}

enum_conversions! {
    stencil_operation_to_wit, stencil_operation_from_wit: StencilOperation <=> StencilOperation {
        Keep, Zero, Replace, Invert, IncrementClamp, DecrementClamp, IncrementWrap, DecrementWrap,
    }
}

enum_conversions! {
    blend_factor_to_wit, blend_factor_from_wit: BlendFactor <=> BlendFactor {
        Zero, One, Src, OneMinusSrc, SrcAlpha, OneMinusSrcAlpha, Dst, OneMinusDst, DstAlpha,
        OneMinusDstAlpha, SrcAlphaSaturated, Constant, OneMinusConstant, Src1, OneMinusSrc1,
        Src1Alpha, OneMinusSrc1Alpha,
    }
}

enum_conversions! {
    blend_operation_to_wit, blend_operation_from_wit: BlendOperation <=> BlendOperation {
        Add, Subtract, ReverseSubtract, Min, Max,
    }
}

enum_conversions! {
    primitive_topology_to_wit, primitive_topology_from_wit: PrimitiveTopology <=> PrimitiveTopology {
        PointList, LineList, LineStrip, TriangleList, TriangleStrip,
    }
}

enum_conversions! {
    front_face_to_wit, front_face_from_wit: FrontFace <=> FrontFace {
        Ccw, Cw,
    }
}

enum_conversions! {
    face_to_wit, face_from_wit: Face <=> Face {
        Front, Back,
    }
}

enum_conversions! {
    polygon_mode_to_wit, polygon_mode_from_wit: PolygonMode <=> PolygonMode {
        Fill, Line, Point,
    }
}

enum_conversions! {
    vertex_step_mode_to_wit, vertex_step_mode_from_wit: VertexStepMode <=> VertexStepMode {
        Vertex, Instance,
    }
}

enum_conversions! {
    store_op_to_wit, store_op_from_wit: StoreOp <=> StoreOp {
        Store, Discard,
    }
}

flag_conversions! {
    pipeline_statistics_types_to_wit, pipeline_statistics_types_from_wit:
        PipelineStatisticsTypes <=> PipelineStatisticsTypes {
        VERTEX_SHADER_INVOCATIONS, CLIPPER_INVOCATIONS, CLIPPER_PRIMITIVES_OUT,
        FRAGMENT_SHADER_INVOCATIONS, COMPUTE_SHADER_INVOCATIONS,
    }
}

pub fn query_type_to_wit(ty: wgpu::QueryType) -> wit::QueryType {
    match ty {
        wgpu::QueryType::Occlusion => wit::QueryType::Occlusion,
        wgpu::QueryType::Timestamp => wit::QueryType::Timestamp,
        wgpu::QueryType::PipelineStatistics(types) => {
            wit::QueryType::PipelineStatistics(pipeline_statistics_types_to_wit(types))
        }
    }
}

pub fn primitive_state_to_wit(state: &wgpu::PrimitiveState) -> wit::PrimitiveState {
    wit::PrimitiveState {
        topology: primitive_topology_to_wit(state.topology),
        strip_index_format: state.strip_index_format.map(index_format_to_wit),
        front_face: front_face_to_wit(state.front_face),
        cull_mode: state.cull_mode.map(face_to_wit),
        unclipped_depth: state.unclipped_depth,
        polygon_mode: polygon_mode_to_wit(state.polygon_mode),
        conservative: state.conservative,
    }
}

fn stencil_face_to_wit(face: &wgpu::StencilFaceState) -> wit::StencilFaceState {
    wit::StencilFaceState {
        compare: compare_function_to_wit(face.compare),
        fail_op: stencil_operation_to_wit(face.fail_op),
        depth_fail_op: stencil_operation_to_wit(face.depth_fail_op),
        pass_op: stencil_operation_to_wit(face.pass_op),
    }
}

pub fn depth_stencil_state_to_wit(state: &wgpu::DepthStencilState) -> wit::DepthStencilState {
    wit::DepthStencilState {
        format: texture_format_to_wit(state.format),
        depth_write_enabled: state.depth_write_enabled,
        depth_compare: state.depth_compare.map(compare_function_to_wit),
        stencil: wit::StencilState {
            front: stencil_face_to_wit(&state.stencil.front),
            back: stencil_face_to_wit(&state.stencil.back),
            read_mask: state.stencil.read_mask,
            write_mask: state.stencil.write_mask,
        },
        bias: wit::DepthBiasState {
            constant: state.bias.constant,
            slope_scale: state.bias.slope_scale,
            clamp: state.bias.clamp,
        },
    }
}

pub fn multisample_state_to_wit(state: &wgpu::MultisampleState) -> wit::MultisampleState {
    wit::MultisampleState {
        count: state.count,
        mask: state.mask,
        alpha_to_coverage_enabled: state.alpha_to_coverage_enabled,
    }
}

fn blend_component_to_wit(component: &wgpu::BlendComponent) -> wit::BlendComponent {
    wit::BlendComponent {
        src_factor: blend_factor_to_wit(component.src_factor),
        dst_factor: blend_factor_to_wit(component.dst_factor),
        operation: blend_operation_to_wit(component.operation),
    }
}

pub fn color_target_to_wit(target: &wgpu::ColorTargetState) -> wit::ColorTargetState {
    wit::ColorTargetState {
        format: texture_format_to_wit(target.format),
        blend: target.blend.as_ref().map(|blend| wit::BlendState {
            color: blend_component_to_wit(&blend.color),
            alpha: blend_component_to_wit(&blend.alpha),
        }),
        write_mask: color_writes_to_wit(target.write_mask),
    }
}

pub fn vertex_buffer_layout_to_wit(
    layout: &wgpu::VertexBufferLayout<'_>,
) -> wit::VertexBufferLayout {
    wit::VertexBufferLayout {
        array_stride: layout.array_stride,
        step_mode: vertex_step_mode_to_wit(layout.step_mode),
        attributes: layout
            .attributes
            .iter()
            .map(|attribute| wit::VertexAttribute {
                format: vertex_format_to_wit(attribute.format),
                offset: attribute.offset,
                shader_location: attribute.shader_location,
            })
            .collect(),
    }
}

pub fn pipeline_compilation_options_to_wit(
    options: &wgpu::PipelineCompilationOptions<'_>,
) -> wit::PipelineCompilationOptions {
    wit::PipelineCompilationOptions {
        constants: (options.constants.iter())
            .map(|&(name, value)| (name.to_owned(), value))
            .collect(),
    }
}

/// `LoadOp::DontCare`, which the WIT leaves out because an undefined attachment could hold another
/// program's pixels, clears instead, as wgpu's backend for the web does: a clear is one of the
/// contents "undefined" allows. So do the depth and stencil operations.
pub fn color_operations_to_wit(ops: &wgpu::Operations<wgpu::Color>) -> wit::ColorOperations {
    wit::ColorOperations {
        load: match ops.load {
            wgpu::LoadOp::Clear(color) => wit::ColorLoadOp::Clear(color_to_wit(color)),
            wgpu::LoadOp::Load => wit::ColorLoadOp::Load,
            wgpu::LoadOp::DontCare(_) => wit::ColorLoadOp::Clear(color_to_wit(wgpu::Color::BLACK)),
        },
        store: store_op_to_wit(ops.store),
    }
}

pub fn depth_operations_to_wit(ops: &wgpu::Operations<f32>) -> wit::DepthOperations {
    wit::DepthOperations {
        load: match ops.load {
            wgpu::LoadOp::Clear(depth) => wit::DepthLoadOp::Clear(depth),
            wgpu::LoadOp::Load => wit::DepthLoadOp::Load,
            wgpu::LoadOp::DontCare(_) => wit::DepthLoadOp::Clear(1.0),
        },
        store: store_op_to_wit(ops.store),
    }
}

pub fn stencil_operations_to_wit(ops: &wgpu::Operations<u32>) -> wit::StencilOperations {
    wit::StencilOperations {
        load: match ops.load {
            wgpu::LoadOp::Clear(stencil) => wit::StencilLoadOp::Clear(stencil),
            wgpu::LoadOp::Load => wit::StencilLoadOp::Load,
            wgpu::LoadOp::DontCare(_) => wit::StencilLoadOp::Clear(0),
        },
        store: store_op_to_wit(ops.store),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_sample_mask_is_carried_whole() {
        let state = wgpu::MultisampleState {
            mask: u64::MAX,
            ..Default::default()
        };
        assert_eq!(multisample_state_to_wit(&state).mask, u64::MAX);
    }
}
