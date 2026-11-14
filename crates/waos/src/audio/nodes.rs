//! The nodes of a program's graph as its handles hold them: a kind's own handle, `gain-node` say,
//! and every `audio-node` it hands out reach the same web-audio-api node, which lives while any of
//! them does, and after, while the graph still plays it.
//!
//! The invariant: a node is locked only from the program's thread, one call at a time, so two
//! nodes locked together, a connection's ends, never wait on each other; a node connected to
//! itself is locked once. A lock is taken around a call into web-audio-api that may panic at the
//! program's mistake, so a poisoned lock is a node whose program has trapped, and is used as it is.

use std::ops::Deref;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use waos_capabilities::web_audio_api::node::AudioNode;

/// One node of the graph, of kind `N`: the value behind that kind's handle.
pub struct Node<N>(Arc<Mutex<N>>);

impl<N: AudioNode + Send + 'static> Node<N> {
    pub(super) fn new(node: N) -> Node<N> {
        Node(Arc::new(Mutex::new(node)))
    }

    /// The node as an `audio-node`.
    pub(super) fn any(&self) -> AnyNode {
        AnyNode(Arc::clone(&self.0) as Arc<dyn Graphed>)
    }

    /// `call` with the node.
    pub(super) fn with<T>(&self, call: impl FnOnce(&mut N) -> T) -> T {
        call(&mut lock(&self.0))
    }
}

/// Any node of the graph: the value behind `audio-node`.
#[derive(Clone)]
pub struct AnyNode(Arc<dyn Graphed>);

impl AnyNode {
    /// `call` with the node.
    pub(super) fn with<T>(&self, call: impl FnOnce(&dyn AudioNode) -> T) -> T {
        call(&*self.0.locked())
    }

    /// `call` with this node and `other`, which may be this one.
    pub(super) fn with_both<T>(
        &self,
        other: &AnyNode,
        call: impl FnOnce(&dyn AudioNode, &dyn AudioNode) -> T,
    ) -> T {
        let this = self.0.locked();
        if self.is(other) {
            return call(&*this, &*this);
        }
        call(&*this, &*other.0.locked())
    }

    fn is(&self, other: &AnyNode) -> bool {
        std::ptr::addr_eq(Arc::as_ptr(&self.0), Arc::as_ptr(&other.0))
    }
}

/// A node of any kind, locked to be used as what every node is.
trait Graphed: Send + Sync {
    fn locked(&self) -> Box<dyn Deref<Target = dyn AudioNode> + '_>;
}

impl<N: AudioNode + Send + 'static> Graphed for Mutex<N> {
    fn locked(&self) -> Box<dyn Deref<Target = dyn AudioNode> + '_> {
        Box::new(Locked(lock(self)))
    }
}

/// A node's lock, held as what every node is.
struct Locked<'a, N>(MutexGuard<'a, N>);

impl<N: AudioNode + 'static> Deref for Locked<'_, N> {
    type Target = dyn AudioNode;

    fn deref(&self) -> &(dyn AudioNode + 'static) {
        &*self.0
    }
}

fn lock<N>(node: &Mutex<N>) -> MutexGuard<'_, N> {
    node.lock().unwrap_or_else(PoisonError::into_inner)
}
