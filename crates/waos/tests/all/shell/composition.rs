//! The system's compositor as the test shell keeps it: it makes layers that show nothing, each
//! numbered in the order made and keeping whether it was made opaque, so a test reads which layers
//! a view's present named, in which order and where; and each keeps the hardware buffers presented
//! to it, as a compositor would show them, letting the one it showed go at the next present, and
//! answers the last as what it shows, or, until the next present, a video's frame a test shows on
//! it as a player's picture, or nothing, as a layer whose player's picture went elsewhere.

use std::any::Any;
use std::sync::{Arc, Mutex};

use waos::bindings::wasdk::platform::views::Placement;
use waos::{HardwareBuffer, LayerPicture, ShellCompositionLayer, ShellCompositor};

use super::hardware_buffer::HeldHardwareBuffer;
use super::video_frames::HeldFrame;

/// Makes layers that show nothing, numbered from 0 in the order made.
#[derive(Default)]
pub struct TestCompositor {
    made: Mutex<Vec<Arc<TestLayer>>>,
}

impl TestCompositor {
    /// Each layer made so far, in order.
    pub fn layers_made(&self) -> Vec<Arc<TestLayer>> {
        self.made.lock().unwrap().clone()
    }
}

impl ShellCompositor for TestCompositor {
    fn make_layer(&self, opaque: bool) -> Arc<dyn ShellCompositionLayer> {
        let mut made = self.made.lock().unwrap();
        let layer = Arc::new(TestLayer {
            number: made.len(),
            opaque,
            shown: Mutex::default(),
        });
        made.push(Arc::clone(&layer));
        layer
    }
}

/// A layer the test compositor made.
pub struct TestLayer {
    /// Where it comes among the layers made, from 0.
    pub number: usize,
    pub opaque: bool,
    shown: Mutex<Shown>,
}

/// The buffers presented to a layer, what lets the one shown now go, and what it shows.
#[derive(Default)]
struct Shown {
    presented: Vec<HeldHardwareBuffer>,
    on_screen: Option<Box<dyn FnOnce() + Send + Sync>>,
    content: Content,
}

/// What a layer shows.
#[derive(Default)]
enum Content {
    /// The buffer presented last, if any was.
    #[default]
    Presented,
    /// A video's frame, as a player's picture on it.
    Frame(HeldFrame),
    Nothing,
}

impl TestLayer {
    /// Each buffer presented to the layer, in order.
    pub fn buffers_presented(&self) -> Vec<HeldHardwareBuffer> {
        self.shown.lock().unwrap().presented.clone()
    }

    /// Shows `frame` until the next present, as a player shows its picture on the layer.
    pub fn show_frame(&self, frame: HeldFrame) {
        self.shown.lock().unwrap().content = Content::Frame(frame);
    }

    /// Shows nothing until the next present, as a layer a player's picture left.
    pub fn show_nothing(&self) {
        self.shown.lock().unwrap().content = Content::Nothing;
    }
}

impl std::fmt::Debug for TestLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestLayer")
            .field("number", &self.number)
            .field("opaque", &self.opaque)
            .finish_non_exhaustive()
    }
}

impl ShellCompositionLayer for TestLayer {
    /// Keeps `buffer` as shown, and lets the one shown before go, outside the lock.
    fn present(&self, buffer: HardwareBuffer, released: Box<dyn FnOnce() + Send + Sync>) {
        let replaced = {
            let mut shown = self.shown.lock().unwrap();
            shown.presented.push(HeldHardwareBuffer(buffer));
            shown.content = Content::Presented;
            shown.on_screen.replace(released)
        };
        if let Some(replaced) = replaced {
            replaced();
        }
    }

    fn current_picture(&self) -> Option<LayerPicture> {
        let shown = self.shown.lock().unwrap();
        match &shown.content {
            Content::Presented => {
                let last = shown.presented.last()?;
                Some(LayerPicture::Buffer(last.clone().into_buffer()))
            }
            Content::Frame(frame) => Some(LayerPicture::Frame(frame.clone().into_frame())),
            Content::Nothing => None,
        }
    }
}

/// A layer a present named, as a test reads it: which one, and where.
#[derive(Clone, Debug)]
pub struct PresentedLayer {
    /// The layer's [`TestLayer::number`].
    pub number: usize,
    pub placement: Placement,
}

/// `layers` as a present hands them to the shell, read as the test compositor's.
pub fn presented(layers: Vec<(Arc<dyn ShellCompositionLayer>, Placement)>) -> Vec<PresentedLayer> {
    layers
        .into_iter()
        .map(|(layer, placement)| {
            let layer: Arc<dyn Any + Send + Sync> = layer;
            let layer = layer
                .downcast::<TestLayer>()
                .expect("a layer the test compositor made");
            PresentedLayer {
                number: layer.number,
                placement,
            }
        })
        .collect()
}
