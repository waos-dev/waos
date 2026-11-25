//! The code serving `wasdk:platform/views` to a program: the views it has and their changes, a
//! request for one through the shell's placement, every method of a view and of its text input,
//! over the state and the shell's side of each view, a key's reply, and the shell's compositor,
//! where it has one, with the composition layers the program makes on it and the hardware buffers
//! it presents on them (`LayerPresents`). A view's event that holds a resource makes its handle as
//! the program reads it, so an event never read makes none.
//!
//! A handle on a view is the view itself in the store's table, so handles on one view share it
//! and `views()` hands out fresh handles every time. A view's canvas context is the WebGPU
//! host's, over the view's canvas (`buffers`), which every context on the view shares and which
//! `present` hands the frame drawn into to the shell, with the layers the view holds beneath it:
//! the shell's own layer of each composition layer `set-layers` named last, in the order named,
//! and its placement as the program gave it (`view_layers`). A transform that is not a 4×4
//! matrix, 16 values, is the program's mistake, and traps it as it sets the layers, so the shell
//! reads every transform it is handed as one.

use std::sync::{Arc, OnceLock};

use futures::channel::mpsc;

use waos_webgpu_host::CanvasContext;
use wasmtime::bail;
use wasmtime::component::{
    Access, FutureReader, HasData, Linker, Resource, ResourceTable, StreamReader,
};

use crate::bindings::wasdk::platform::views::{
    self, Cursor, EdgeSwipe, HostCompositionLayer, HostCompositionLayerWithStore, HostCompositor,
    HostKeyReply, HostSystemBars, HostTextInput, HostView, HostViewWithStore, HostWithStore,
    KeyInput, Metrics, Mutator, OverlayStyle, PixelRect, PlacedLayer, Placement, RequestId,
    TextEditingValue, TextInputConfiguration, ViewError, ViewEvent, ViewFocusDirection,
    ViewFocusState, ViewId,
};
use crate::gpu::{Finishing, GpuInstance};
use crate::hardware_buffers::{Canvases, ProgramHardwareBuffer, SubmittedWork, ViewCanvas};
use crate::running::RunningProgram;
use crate::screens::{Screen, Screens};
use crate::shell::{
    ShellCompositionLayer, ShellCompositor, ShellSystemBars, ViewPlacement, ViewRequest,
};
use crate::stream::stream_of_holding;
use crate::windows::{ProgramWindow, WindowConfig};

use super::view_text_input::ViewTextInput;
use super::{
    KeyReply, LayerPresents, PlacedShellLayer, ProgramView, QueuedEvent, ViewChange, ViewLayers,
};

/// A program's handle on a view's text input: the view's, which every handle on the view shares.
pub struct TextInput(ViewTextInput);

/// A program's handle on the system's bars over a view: the shell's, which every handle on the
/// view shares.
pub struct SystemBars(Arc<dyn ShellSystemBars>);

/// `views.compositor`: the shell's compositor, as the program reaches it.
pub struct Compositor(Arc<dyn ShellCompositor>);

/// A program's composition layer: the layer the shell's compositor made for it, which a view
/// holding it hands back to the shell with each present, and the presents of the program's
/// hardware buffers on it, started at the first.
pub struct CompositionLayer {
    shell: Arc<dyn ShellCompositionLayer>,
    presents: OnceLock<LayerPresents>,
}

impl CompositionLayer {
    fn of(shell: Arc<dyn ShellCompositionLayer>) -> CompositionLayer {
        CompositionLayer {
            shell,
            presents: OnceLock::new(),
        }
    }

    /// The layer the shell's compositor made for it, which a player is given to draw on.
    pub(crate) fn shell_layer(&self) -> &Arc<dyn ShellCompositionLayer> {
        &self.shell
    }

    /// Shows `buffer` on the layer once `gpu` has finished `work`, what the program submitted and
    /// wrote to draw into it, counted in `in_flight` until then; resolves once the shell is done
    /// with it.
    fn present(
        &self,
        buffer: ProgramHardwareBuffer,
        work: SubmittedWork,
        gpu: &GpuInstance,
        in_flight: Finishing,
    ) -> tokio::sync::oneshot::Receiver<()> {
        self.presents
            .get_or_init(|| LayerPresents::start(Arc::clone(gpu), Arc::clone(&self.shell)))
            .present(buffer, work, in_flight)
    }
}

/// What the views interface reads and writes in a program's store.
pub(crate) struct ViewsCtx<'a> {
    pub(crate) table: &'a mut ResourceTable,
    pub(crate) program: &'a RunningProgram,
    pub(crate) screens: &'a Screens,
    /// What places the program's views, and so whether this instance has them: none in a
    /// worker, which lists no view, hears of none and has its requests refused as `unsupported`.
    pub(crate) placement: Option<&'a dyn ViewPlacement>,
    /// The shell's compositor; none where the shell has none.
    pub(crate) compositor: Option<&'a Arc<dyn ShellCompositor>>,
    pub(crate) gpu: &'a GpuInstance,
    pub(crate) canvases: &'a Canvases,
    /// The layers each view holds beneath its picture.
    pub(crate) layers: &'a ViewLayers,
}

pub(crate) trait ViewsCtxView: Send {
    fn views_ctx(&mut self) -> ViewsCtx<'_>;
}

struct HasViews;

impl HasData for HasViews {
    type Data<'a> = ViewsCtx<'a>;
}

pub(crate) fn add_to_linker<T: ViewsCtxView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    views::add_to_linker::<_, HasViews>(linker, T::views_ctx)
}

impl ViewsCtx<'_> {
    fn view(&self, view: &Resource<ProgramView>) -> wasmtime::Result<&ProgramView> {
        Ok(self.table.get(view)?)
    }

    /// The canvas `view` is drawn into, made at the first context taken on the view.
    fn canvas(&self, view: &ProgramView) -> Arc<ViewCanvas> {
        self.canvases.for_view(view.id(), || {
            ViewCanvas::new(
                view.clone(),
                Arc::clone(self.gpu),
                self.program.shared_stats(),
                self.program.lifecycle().clone(),
                self.program.engine_clock().clone(),
                Arc::clone(self.program.in_flight()),
                self.program.hardware_buffers(),
            )
        })
    }

    /// The canvas the program drew on `view` into, with the layers the view holds beneath it, to
    /// present; none for a view the program took no context on, which has nothing drawn to
    /// present, and whose layers wait.
    fn canvas_to_present(
        &self,
        view: &Resource<ProgramView>,
    ) -> wasmtime::Result<Option<(Arc<ViewCanvas>, Vec<PlacedShellLayer>)>> {
        let id = self.view(view)?.id();
        Ok(self
            .canvases
            .of_view(id)
            .map(|canvas| (canvas, self.layers.of(id))))
    }

    /// The shell's own layer of each of `layers`, with its placement, in the order named.
    fn shell_layers(&self, layers: Vec<PlacedLayer>) -> wasmtime::Result<Vec<PlacedShellLayer>> {
        layers
            .into_iter()
            .map(|placed| {
                check_transforms(&placed.placement)?;
                let layer = Arc::clone(self.table.get(&placed.layer)?.shell_layer());
                Ok((layer, placed.placement))
            })
            .collect()
    }
}

/// Values in a transform: a 4×4 matrix's.
const TRANSFORM_VALUES: usize = 16;

/// Traps a placement whose transform is not a 4×4 matrix.
fn check_transforms(placement: &Placement) -> wasmtime::Result<()> {
    for mutator in &placement.mutators {
        if let Mutator::Transform(values) = mutator
            && values.len() != TRANSFORM_VALUES
        {
            bail!(
                "a layer's transform is a 4×4 matrix, {TRANSFORM_VALUES} values, not {}",
                values.len()
            );
        }
    }
    Ok(())
}

impl views::Host for ViewsCtx<'_> {
    fn views(&mut self) -> wasmtime::Result<Vec<Resource<ProgramView>>> {
        if self.placement.is_none() {
            return Ok(Vec::new());
        }
        self.program
            .views()
            .into_iter()
            .map(|view| Ok(self.table.push(view)?))
            .collect()
    }

    /// The request is given its id and what it attached before the shell is asked, so a view
    /// the shell adds at once finds them; a request the shell refuses is forgotten again.
    fn request_view(
        &mut self,
        config: Resource<WindowConfig>,
        initial_data: Option<String>,
    ) -> wasmtime::Result<Result<RequestId, ViewError>> {
        let Some(placement) = self.placement else {
            return Ok(Err(ViewError::Unsupported));
        };
        let config = self.table.get(&config)?.clone();
        let id = self
            .program
            .program_views()
            .new_request(initial_data, config.title.clone());
        let asked = placement.view_requested(self.program, ViewRequest { id, config });
        Ok(match asked {
            Ok(()) => Ok(id),
            Err(error) => {
                self.program.program_views().request_refused(id);
                Err(error)
            }
        })
    }

    fn get_compositor(&mut self) -> wasmtime::Result<Option<Resource<Compositor>>> {
        let compositor = self.compositor.cloned().map(Compositor);
        Ok(compositor
            .map(|compositor| self.table.push(compositor))
            .transpose()?)
    }
}

impl<T: ViewsCtxView + 'static> HostWithStore<T> for HasViews {
    /// Each view added enters the table as the program reads of it, so a change it never reads
    /// makes no handle. An instance without views hears of none: its stream ends at once.
    fn view_changes(
        mut access: Access<T, Self>,
    ) -> wasmtime::Result<StreamReader<views::ViewChange>> {
        let views = access.get();
        let changes = match views.placement {
            Some(_) => views.program.program_views().changes(),
            None => mpsc::unbounded().1,
        };
        stream_of_holding(access, changes, |data: &mut T, change| {
            Ok(match change {
                ViewChange::Added(view) => {
                    views::ViewChange::Added(data.views_ctx().table.push(view)?)
                }
                ViewChange::Removed(id) => views::ViewChange::Removed(id),
                ViewChange::RequestFailed(failure) => views::ViewChange::RequestFailed(failure),
            })
        })
    }
}

impl HostView for ViewsCtx<'_> {
    fn id(&mut self, view: Resource<ProgramView>) -> wasmtime::Result<ViewId> {
        Ok(self.view(&view)?.id())
    }

    fn request_id(&mut self, view: Resource<ProgramView>) -> wasmtime::Result<RequestId> {
        Ok(self.view(&view)?.request_id())
    }

    fn initial_data(&mut self, view: Resource<ProgramView>) -> wasmtime::Result<Option<String>> {
        Ok(self.view(&view)?.initial_data().map(str::to_owned))
    }

    fn window(
        &mut self,
        view: Resource<ProgramView>,
    ) -> wasmtime::Result<Option<Resource<ProgramWindow>>> {
        let Some(window) = self.view(&view)?.window().cloned() else {
            return Ok(None);
        };
        Ok(Some(self.table.push(window)?))
    }

    /// The screen as the system has it now; none while the view is on no screen the system has.
    fn screen(
        &mut self,
        view: Resource<ProgramView>,
    ) -> wasmtime::Result<Option<Resource<Screen>>> {
        let Some(screen) = self.view(&view)?.screen() else {
            return Ok(None);
        };
        let Some(screen) = self.screens.find(screen) else {
            return Ok(None);
        };
        Ok(Some(self.table.push(screen)?))
    }

    fn context(
        &mut self,
        view: Resource<ProgramView>,
    ) -> wasmtime::Result<Resource<CanvasContext>> {
        let canvas = self.canvas(&self.view(&view)?.clone());
        Ok(self.table.push(CanvasContext::new(canvas))?)
    }

    fn metrics(&mut self, view: Resource<ProgramView>) -> wasmtime::Result<Metrics> {
        Ok(self.view(&view)?.metrics())
    }

    /// The view's canvas hands out textures of `width` by `height` from its next on; a size of
    /// zero traps the program, and one past its device's largest texture traps it once the canvas
    /// is configured at it.
    fn set_canvas_size(
        &mut self,
        view: Resource<ProgramView>,
        width: u32,
        height: u32,
    ) -> wasmtime::Result<()> {
        if width == 0 || height == 0 {
            bail!("a view's canvas is at least a pixel on each side, not {width}×{height}");
        }
        self.view(&view)?.set_canvas_size([width, height]);
        Ok(())
    }

    fn request_frame(&mut self, view: Resource<ProgramView>) -> wasmtime::Result<()> {
        self.view(&view)?.request_frame();
        Ok(())
    }

    /// The view holds `layers` from now on, for its next present.
    fn set_layers(
        &mut self,
        view: Resource<ProgramView>,
        layers: Vec<PlacedLayer>,
    ) -> wasmtime::Result<()> {
        let id = self.view(&view)?.id();
        let layers = self.shell_layers(layers)?;
        self.layers.set(id, layers);
        Ok(())
    }

    fn set_cursor(&mut self, view: Resource<ProgramView>, cursor: Cursor) -> wasmtime::Result<()> {
        self.view(&view)?.shell().set_cursor(cursor);
        Ok(())
    }

    fn text_input(
        &mut self,
        view: Resource<ProgramView>,
    ) -> wasmtime::Result<Option<Resource<TextInput>>> {
        let Some(text_input) = self.view(&view)?.text_input().cloned() else {
            return Ok(None);
        };
        Ok(Some(self.table.push(TextInput(text_input))?))
    }

    fn request_focus_change(
        &mut self,
        view: Resource<ProgramView>,
        state: ViewFocusState,
        direction: ViewFocusDirection,
    ) -> wasmtime::Result<()> {
        self.view(&view)?
            .shell()
            .request_focus_change(state, direction);
        Ok(())
    }

    fn set_handles_back(
        &mut self,
        view: Resource<ProgramView>,
        handles: bool,
    ) -> wasmtime::Result<()> {
        self.view(&view)?.set_handles_back(handles);
        Ok(())
    }

    /// The deprecated way to the bars' style: their `set-overlay-style` where there are bars.
    fn set_overlay_style(
        &mut self,
        view: Resource<ProgramView>,
        style: OverlayStyle,
    ) -> wasmtime::Result<()> {
        if let Some(bars) = self.view(&view)?.system_bars() {
            bars.set_overlay_style(style);
        }
        Ok(())
    }

    fn system_bars(
        &mut self,
        view: Resource<ProgramView>,
    ) -> wasmtime::Result<Option<Resource<SystemBars>>> {
        let Some(bars) = self.view(&view)?.system_bars().cloned() else {
            return Ok(None);
        };
        Ok(Some(self.table.push(SystemBars(bars))?))
    }

    fn drop(&mut self, view: Resource<ProgramView>) -> wasmtime::Result<()> {
        self.table.delete(view)?;
        Ok(())
    }
}

impl<T: ViewsCtxView + 'static> HostViewWithStore<T> for HasViews {
    fn events(
        mut access: Access<T, Self>,
        view: Resource<ProgramView>,
    ) -> wasmtime::Result<Option<StreamReader<ViewEvent>>> {
        let Some(events) = access.get().view(&view)?.events() else {
            return Ok(None);
        };
        stream_of_holding(access, events, |data: &mut T, event| {
            data.views_ctx().view_event(event)
        })
        .map(Some)
    }

    /// Presents what the program drew on the view, with the layers the view holds beneath it,
    /// what it wrote submitted first, which waits while the program is frozen, as a submission
    /// does (`ViewCanvas::present`). The program's store waits with it: to the program this is a
    /// plain call.
    async fn present(
        mut host: Access<'_, T, Self>,
        view: Resource<ProgramView>,
    ) -> wasmtime::Result<()> {
        if let Some((canvas, layers)) = host.get().canvas_to_present(&view)? {
            canvas.present(layers).await?;
        }
        Ok(())
    }
}

impl ViewsCtx<'_> {
    /// `event` as the program reads it, a handle made for what it holds.
    fn view_event(&mut self, event: QueuedEvent) -> wasmtime::Result<ViewEvent> {
        Ok(match event {
            QueuedEvent::Plain(event) => event,
            QueuedEvent::Key(event, reply) => ViewEvent::Key(KeyInput {
                event,
                reply: self.table.push(reply)?,
            }),
            QueuedEvent::Copy(request) => ViewEvent::Copy(self.table.push(request)?),
            QueuedEvent::Cut(request) => ViewEvent::Cut(self.table.push(request)?),
            QueuedEvent::Paste(paste) => ViewEvent::Paste(self.table.push(paste)?),
        })
    }

    fn view_text_input(
        &self,
        text_input: &Resource<TextInput>,
    ) -> wasmtime::Result<&ViewTextInput> {
        Ok(&self.table.get(text_input)?.0)
    }
}

impl HostSystemBars for ViewsCtx<'_> {
    fn set_overlay_style(
        &mut self,
        bars: Resource<SystemBars>,
        style: OverlayStyle,
    ) -> wasmtime::Result<()> {
        self.table.get(&bars)?.0.set_overlay_style(style);
        Ok(())
    }

    fn hide(&mut self, bars: Resource<SystemBars>, swipe: EdgeSwipe) -> wasmtime::Result<()> {
        self.table.get(&bars)?.0.hide(swipe);
        Ok(())
    }

    fn show(&mut self, bars: Resource<SystemBars>) -> wasmtime::Result<()> {
        self.table.get(&bars)?.0.show();
        Ok(())
    }

    fn drop(&mut self, bars: Resource<SystemBars>) -> wasmtime::Result<()> {
        self.table.delete(bars)?;
        Ok(())
    }
}

impl HostTextInput for ViewsCtx<'_> {
    fn set_client(
        &mut self,
        text_input: Resource<TextInput>,
        configuration: TextInputConfiguration,
    ) -> wasmtime::Result<()> {
        self.view_text_input(&text_input)?.set_client(configuration);
        Ok(())
    }

    fn show(&mut self, text_input: Resource<TextInput>) -> wasmtime::Result<()> {
        self.view_text_input(&text_input)?.shell().show();
        Ok(())
    }

    fn update_config(
        &mut self,
        text_input: Resource<TextInput>,
        configuration: TextInputConfiguration,
    ) -> wasmtime::Result<()> {
        self.view_text_input(&text_input)?
            .shell()
            .update_config(configuration);
        Ok(())
    }

    fn set_editing_state(
        &mut self,
        text_input: Resource<TextInput>,
        value: TextEditingValue,
    ) -> wasmtime::Result<()> {
        self.view_text_input(&text_input)?.set_editing_state(value);
        Ok(())
    }

    fn set_caret_rect(
        &mut self,
        text_input: Resource<TextInput>,
        rect: PixelRect,
    ) -> wasmtime::Result<()> {
        self.view_text_input(&text_input)?
            .shell()
            .set_caret_rect(rect);
        Ok(())
    }

    fn set_composing_rect(
        &mut self,
        text_input: Resource<TextInput>,
        rect: PixelRect,
    ) -> wasmtime::Result<()> {
        self.view_text_input(&text_input)?
            .shell()
            .set_composing_rect(rect);
        Ok(())
    }

    fn clear_client(&mut self, text_input: Resource<TextInput>) -> wasmtime::Result<()> {
        self.view_text_input(&text_input)?.clear_client();
        Ok(())
    }

    fn hide(&mut self, text_input: Resource<TextInput>) -> wasmtime::Result<()> {
        self.view_text_input(&text_input)?.shell().hide();
        Ok(())
    }

    fn drop(&mut self, text_input: Resource<TextInput>) -> wasmtime::Result<()> {
        self.table.delete(text_input)?;
        Ok(())
    }
}

impl HostCompositor for ViewsCtx<'_> {
    fn make_layer(
        &mut self,
        compositor: Resource<Compositor>,
        opaque: bool,
    ) -> wasmtime::Result<Resource<CompositionLayer>> {
        let layer = self.table.get(&compositor)?.0.make_layer(opaque);
        Ok(self.table.push(CompositionLayer::of(layer))?)
    }

    fn drop(&mut self, compositor: Resource<Compositor>) -> wasmtime::Result<()> {
        self.table.delete(compositor)?;
        Ok(())
    }
}

impl HostCompositionLayer for ViewsCtx<'_> {
    /// The program lets go of the layer; a view holding it still holds it until its layers are
    /// set again without it, and a frame on screen showing it until the next frame.
    fn drop(&mut self, layer: Resource<CompositionLayer>) -> wasmtime::Result<()> {
        self.table.delete(layer)?;
        Ok(())
    }
}

impl<T: ViewsCtxView + 'static> HostCompositionLayerWithStore<T> for HasViews {
    /// Hands the buffer to the layer's presents, off the program's thread, once what the program
    /// wrote is submitted, which waits while it is frozen, as a submission does; the answer
    /// resolves once the shell is done with it, or as the layer goes with the buffer unshown, and
    /// the buffer is taken back for the program's devices. The
    /// program's store waits for the submission: to the program this is a plain call.
    async fn present(
        mut host: Access<'_, T, Self>,
        layer: Resource<CompositionLayer>,
        buffer: Resource<ProgramHardwareBuffer>,
    ) -> wasmtime::Result<FutureReader<()>> {
        let context = host.get();
        let buffer = context.table.get(&buffer)?.clone();
        let lifecycle = context.program.lifecycle().clone();
        let gpu = Arc::clone(context.gpu);
        let work = buffer.hand_out(&gpu, &lifecycle).await;
        let context = host.get();
        let in_flight = context.program.in_flight().begin();
        let released = context
            .table
            .get(&layer)?
            .present(buffer.clone(), work, &gpu, in_flight);
        FutureReader::new(host, async move {
            // An answer that never comes, as the layer goes, still frees the program's wait.
            let _ = released.await;
            buffer.take_back(&gpu, &lifecycle).await;
            Ok::<_, wasmtime::Error>(())
        })
    }
}

impl HostKeyReply for ViewsCtx<'_> {
    fn handled(&mut self, reply: Resource<KeyReply>, handled: bool) -> wasmtime::Result<()> {
        self.table.get_mut(&reply)?.answer(handled);
        Ok(())
    }

    /// Unanswered, the key was not handled.
    fn drop(&mut self, reply: Resource<KeyReply>) -> wasmtime::Result<()> {
        self.table.delete(reply)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::wasdk::platform::windows::{Point, Rect, Size};

    fn placement(mutators: Vec<Mutator>) -> Placement {
        Placement {
            rect: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 10.0,
                    height: 10.0,
                },
            },
            mutators,
        }
    }

    #[test]
    fn a_transform_of_sixteen_values_passes() {
        let identity = (0..16)
            .map(|i| if i % 5 == 0 { 1.0 } else { 0.0 })
            .collect();
        let placement = placement(vec![Mutator::Opacity(255), Mutator::Transform(identity)]);
        assert!(check_transforms(&placement).is_ok());
    }

    /// The shell reads every transform as a 4×4 matrix, so one that is not traps the program.
    #[test]
    fn a_transform_of_other_than_sixteen_values_traps() {
        for values in [0, 6, 9, 17] {
            let placement = placement(vec![Mutator::Transform(vec![1.0; values])]);
            let trap = check_transforms(&placement).unwrap_err();
            assert!(
                trap.to_string().contains(&format!("not {values}")),
                "{trap}"
            );
        }
    }
}
