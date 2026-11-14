//! Render state: primitive, depth-stencil, blend, multisample, vertex layouts and load/store ops.

use wasdk::webgpu::{
    GpuBlendComponent, GpuBlendFactor, GpuBlendOperation, GpuBlendState, GpuColorTargetState,
    GpuCompareFunction, GpuCullMode, GpuDepthStencilState, GpuFrontFace, GpuLoadOp,
    GpuMultisampleState, GpuPrimitiveState, GpuPrimitiveTopology, GpuStencilFaceState,
    GpuStencilOperation, GpuStoreOp, GpuVertexAttribute, GpuVertexBufferLayout, GpuVertexStepMode,
};
use wgpu::{
    BlendComponent, BlendFactor, BlendOperation, BlendState, ColorTargetState, CompareFunction,
    DepthStencilState, Face, Features, FrontFace, LoadOp, MultisampleState, Operations,
    PolygonMode, PrimitiveState, PrimitiveTopology, StencilFaceState, StencilOperation, StoreOp,
    VertexAttribute, VertexBufferLayout, VertexStepMode,
};

use super::{
    color_writes_to_wasi, index_format_to_wasi, texture_format_to_wasi, vertex_format_to_wasi,
};
use crate::gap::feature_gap;

pub fn compare_function_to_wasi(function: CompareFunction) -> GpuCompareFunction {
    match function {
        CompareFunction::Never => GpuCompareFunction::Never,
        CompareFunction::Less => GpuCompareFunction::Less,
        CompareFunction::Equal => GpuCompareFunction::Equal,
        CompareFunction::LessEqual => GpuCompareFunction::LessEqual,
        CompareFunction::Greater => GpuCompareFunction::Greater,
        CompareFunction::NotEqual => GpuCompareFunction::NotEqual,
        CompareFunction::GreaterEqual => GpuCompareFunction::GreaterEqual,
        CompareFunction::Always => GpuCompareFunction::Always,
    }
}

pub fn stencil_operation_to_wasi(operation: StencilOperation) -> GpuStencilOperation {
    match operation {
        StencilOperation::Keep => GpuStencilOperation::Keep,
        StencilOperation::Zero => GpuStencilOperation::Zero,
        StencilOperation::Replace => GpuStencilOperation::Replace,
        StencilOperation::Invert => GpuStencilOperation::Invert,
        StencilOperation::IncrementClamp => GpuStencilOperation::IncrementClamp,
        StencilOperation::DecrementClamp => GpuStencilOperation::DecrementClamp,
        StencilOperation::IncrementWrap => GpuStencilOperation::IncrementWrap,
        StencilOperation::DecrementWrap => GpuStencilOperation::DecrementWrap,
    }
}

pub fn stencil_face_to_wasi(face: &StencilFaceState) -> GpuStencilFaceState {
    GpuStencilFaceState {
        compare: Some(compare_function_to_wasi(face.compare)),
        fail_op: Some(stencil_operation_to_wasi(face.fail_op)),
        depth_fail_op: Some(stencil_operation_to_wasi(face.depth_fail_op)),
        pass_op: Some(stencil_operation_to_wasi(face.pass_op)),
    }
}

pub fn depth_stencil_state_to_wasi(state: &DepthStencilState) -> GpuDepthStencilState {
    GpuDepthStencilState {
        format: texture_format_to_wasi(state.format),
        depth_write_enabled: state.depth_write_enabled,
        depth_compare: state.depth_compare.map(compare_function_to_wasi),
        stencil_front: Some(stencil_face_to_wasi(&state.stencil.front)),
        stencil_back: Some(stencil_face_to_wasi(&state.stencil.back)),
        stencil_read_mask: Some(state.stencil.read_mask),
        stencil_write_mask: Some(state.stencil.write_mask),
        depth_bias: Some(state.bias.constant),
        depth_bias_slope_scale: Some(state.bias.slope_scale),
        depth_bias_clamp: Some(state.bias.clamp),
    }
}

pub fn blend_factor_to_wasi(factor: BlendFactor) -> GpuBlendFactor {
    match factor {
        BlendFactor::Zero => GpuBlendFactor::Zero,
        BlendFactor::One => GpuBlendFactor::One,
        BlendFactor::Src => GpuBlendFactor::Src,
        BlendFactor::OneMinusSrc => GpuBlendFactor::OneMinusSrc,
        BlendFactor::SrcAlpha => GpuBlendFactor::SrcAlpha,
        BlendFactor::OneMinusSrcAlpha => GpuBlendFactor::OneMinusSrcAlpha,
        BlendFactor::Dst => GpuBlendFactor::Dst,
        BlendFactor::OneMinusDst => GpuBlendFactor::OneMinusDst,
        BlendFactor::DstAlpha => GpuBlendFactor::DstAlpha,
        BlendFactor::OneMinusDstAlpha => GpuBlendFactor::OneMinusDstAlpha,
        BlendFactor::SrcAlphaSaturated => GpuBlendFactor::SrcAlphaSaturated,
        BlendFactor::Constant => GpuBlendFactor::Constant,
        BlendFactor::OneMinusConstant => GpuBlendFactor::OneMinusConstant,
        BlendFactor::Src1 => GpuBlendFactor::Src1,
        BlendFactor::OneMinusSrc1 => GpuBlendFactor::OneMinusSrc1,
        BlendFactor::Src1Alpha => GpuBlendFactor::Src1Alpha,
        BlendFactor::OneMinusSrc1Alpha => GpuBlendFactor::OneMinusSrc1Alpha,
    }
}

pub fn blend_operation_to_wasi(operation: BlendOperation) -> GpuBlendOperation {
    match operation {
        BlendOperation::Add => GpuBlendOperation::Add,
        BlendOperation::Subtract => GpuBlendOperation::Subtract,
        BlendOperation::ReverseSubtract => GpuBlendOperation::ReverseSubtract,
        BlendOperation::Min => GpuBlendOperation::Min,
        BlendOperation::Max => GpuBlendOperation::Max,
    }
}

pub fn blend_component_to_wasi(component: &BlendComponent) -> GpuBlendComponent {
    GpuBlendComponent {
        operation: Some(blend_operation_to_wasi(component.operation)),
        src_factor: Some(blend_factor_to_wasi(component.src_factor)),
        dst_factor: Some(blend_factor_to_wasi(component.dst_factor)),
    }
}

pub fn blend_state_to_wasi(state: &BlendState) -> GpuBlendState {
    GpuBlendState {
        color: blend_component_to_wasi(&state.color),
        alpha: blend_component_to_wasi(&state.alpha),
    }
}

pub fn color_target_to_wasi(target: &ColorTargetState) -> GpuColorTargetState {
    GpuColorTargetState {
        format: texture_format_to_wasi(target.format),
        blend: target.blend.as_ref().map(blend_state_to_wasi),
        write_mask: Some(color_writes_to_wasi(target.write_mask)),
    }
}

pub fn primitive_topology_to_wasi(topology: PrimitiveTopology) -> GpuPrimitiveTopology {
    match topology {
        PrimitiveTopology::PointList => GpuPrimitiveTopology::PointList,
        PrimitiveTopology::LineList => GpuPrimitiveTopology::LineList,
        PrimitiveTopology::LineStrip => GpuPrimitiveTopology::LineStrip,
        PrimitiveTopology::TriangleList => GpuPrimitiveTopology::TriangleList,
        PrimitiveTopology::TriangleStrip => GpuPrimitiveTopology::TriangleStrip,
    }
}

pub fn front_face_to_wasi(front_face: FrontFace) -> GpuFrontFace {
    match front_face {
        FrontFace::Ccw => GpuFrontFace::Ccw,
        FrontFace::Cw => GpuFrontFace::Cw,
    }
}

pub fn cull_mode_to_wasi(cull_mode: Option<Face>) -> GpuCullMode {
    match cull_mode {
        None => GpuCullMode::None,
        Some(Face::Front) => GpuCullMode::Front,
        Some(Face::Back) => GpuCullMode::Back,
    }
}

pub fn primitive_state_to_wasi(state: &PrimitiveState) -> GpuPrimitiveState {
    match state.polygon_mode {
        PolygonMode::Fill => {}
        PolygonMode::Line => feature_gap(Features::POLYGON_MODE_LINE, "PolygonMode::Line"),
        PolygonMode::Point => feature_gap(Features::POLYGON_MODE_POINT, "PolygonMode::Point"),
    }
    if state.conservative {
        feature_gap(
            Features::CONSERVATIVE_RASTERIZATION,
            "PrimitiveState::conservative",
        );
    }
    GpuPrimitiveState {
        topology: Some(primitive_topology_to_wasi(state.topology)),
        strip_index_format: state.strip_index_format.map(index_format_to_wasi),
        front_face: Some(front_face_to_wasi(state.front_face)),
        cull_mode: Some(cull_mode_to_wasi(state.cull_mode)),
        unclipped_depth: Some(state.unclipped_depth),
    }
}

pub fn multisample_state_to_wasi(state: &MultisampleState) -> GpuMultisampleState {
    GpuMultisampleState {
        count: Some(state.count),
        mask: Some(sample_mask_to_wasi(state.mask)),
        alpha_to_coverage_enabled: Some(state.alpha_to_coverage_enabled),
    }
}

/// WebGPU's sample mask is 32 bits wide, and no sample count reaches 32: the bits wgpu's
/// 64-bit mask has above them select no sample and are dropped.
fn sample_mask_to_wasi(mask: u64) -> u32 {
    mask as u32
}

pub fn vertex_step_mode_to_wasi(mode: VertexStepMode) -> GpuVertexStepMode {
    match mode {
        VertexStepMode::Vertex => GpuVertexStepMode::Vertex,
        VertexStepMode::Instance => GpuVertexStepMode::Instance,
    }
}

pub fn vertex_attribute_to_wasi(attribute: &VertexAttribute) -> GpuVertexAttribute {
    GpuVertexAttribute {
        format: vertex_format_to_wasi(attribute.format),
        offset: attribute.offset,
        shader_location: attribute.shader_location,
    }
}

pub fn vertex_buffer_layout_to_wasi(layout: &VertexBufferLayout<'_>) -> GpuVertexBufferLayout {
    GpuVertexBufferLayout {
        array_stride: layout.array_stride,
        step_mode: Some(vertex_step_mode_to_wasi(layout.step_mode)),
        attributes: layout
            .attributes
            .iter()
            .map(vertex_attribute_to_wasi)
            .collect(),
    }
}

pub fn store_op_to_wasi(op: StoreOp) -> GpuStoreOp {
    match op {
        StoreOp::Store => GpuStoreOp::Store,
        StoreOp::Discard => GpuStoreOp::Discard,
    }
}

/// An attachment's operations as WebGPU states them: a load op plus the value a clear uses.
#[derive(Debug, Clone, Copy)]
pub struct AttachmentOps<V> {
    pub load: GpuLoadOp,
    pub clear_value: Option<V>,
    pub store: GpuStoreOp,
}

/// `LoadOp::DontCare` leaves the contents undefined, which WebGPU cannot say; it clears to
/// `undefined_value`, as wgpu's web backend does.
pub fn operations_to_wasi<V: Clone>(ops: &Operations<V>, undefined_value: V) -> AttachmentOps<V> {
    let (load, clear_value) = match &ops.load {
        LoadOp::Clear(value) => (GpuLoadOp::Clear, Some(value.clone())),
        LoadOp::Load => (GpuLoadOp::Load, None),
        LoadOp::DontCare(_) => (GpuLoadOp::Clear, Some(undefined_value)),
    };
    AttachmentOps {
        load,
        clear_value,
        store: store_op_to_wasi(ops.store),
    }
}
