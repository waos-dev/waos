//! Where every node of a model is, in the model's own space, for a set of the nodes' transforms.

use glam::Mat4;

use super::{Model, Transform};

/// The transforms the model's nodes rest at, before any animation.
pub fn rest(model: &Model) -> Vec<Transform> {
    model.nodes.iter().map(|node| node.transform).collect()
}

/// Each node's matrix from its own space to the model's, with `transforms` as the nodes' own. A
/// node outside the scene shown keeps the identity.
pub fn world_matrices(model: &Model, transforms: &[Transform]) -> Vec<Mat4> {
    let mut matrices = vec![Mat4::IDENTITY; model.nodes.len()];
    for node in model.scene_nodes() {
        let parent = node
            .parent
            .map_or(Mat4::IDENTITY, |parent| matrices[parent]);
        matrices[node.index] = parent * transforms[node.index].matrix();
    }
    matrices
}
