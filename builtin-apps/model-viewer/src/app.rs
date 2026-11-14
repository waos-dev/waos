//! The app: one view, with a model on show in it, drawn only when something changed.
//!
//! A frame is asked for by whatever changes the picture: egui's glue for input, the app for a
//! new size, a new model or a running animation. Each frame runs one pass of the window's
//! layout, which moves the camera and answers what the user asked for, then draws the scene
//! where the layout left room for it and the panel over it.

use std::path::Path;
use std::time::Duration;

use egui::{Key, KeyboardShortcut, Modifiers};
use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use wasdk::system::{self, Brightness, OperatingSystem};
use wasdk::views::ViewEvent;
use wasdk_wgpu::SharedView;

use crate::egui_view::EguiView;
use crate::events::{self, AppEvent, Sender};
use crate::gpu::Gpu;
use crate::model::{self, Model};
use crate::render::canvas::Canvas;
use crate::render::globals::{Frame, Ground};
use crate::render::panel_painter::PanelPainter;
use crate::render::scene::{Area, SceneRenderer, Target};
use crate::settings::ViewSettings;
use crate::shown::Shown;
use crate::ui::panel::{Action, Contents, Status};
use crate::ui::{self, stage, theme};
use crate::{open, window};

const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
/// The longest an animation moves on in one frame, so it does not jump after a pause in frames.
const LONGEST_STEP: Duration = Duration::from_millis(100);

pub struct App {
    gpu: Gpu,
    view: SharedView,
    canvas: Canvas,
    egui: EguiView,
    scene: SceneRenderer,
    panel_painter: PanelPainter,
    events: UnboundedReceiver<AppEvent>,
    sender: Sender,
    shown: Shown,
    settings: ViewSettings,
    status: Status,
    theme: egui::Theme,
    /// The file to read once the frame that says so is on screen.
    file_to_read: Option<String>,
    /// When the last frame was for, in nanoseconds; none after a stretch without frames.
    last_frame: Option<u64>,
}

impl App {
    /// The app on `view`, showing the sample model.
    pub fn new(gpu: Gpu, view: SharedView) -> Result<App, String> {
        let canvas = Canvas::new(&gpu, &view)?;
        let (sender, events) = events::queue(&view)?;
        let scene = SceneRenderer::new(&gpu.device, canvas.format());
        let theme = theme_of(system::platform_brightness());
        Ok(App {
            egui: egui_view(&view, theme),
            panel_painter: PanelPainter::new(&gpu.device, canvas.format()),
            shown: Shown::new(&gpu, &scene, model::sample::model()),
            gpu,
            view,
            canvas,
            scene,
            events,
            sender,
            settings: ViewSettings::default(),
            status: Status::Ready,
            theme,
            file_to_read: None,
            last_frame: None,
        })
    }

    /// Answers the app's events until its view is gone.
    pub async fn run(mut self) -> Result<(), String> {
        self.view.request_frame();
        while let Some(event) = self.events.next().await {
            match event {
                AppEvent::View(ViewEvent::Frame(frame)) => self.frame(frame.time),
                AppEvent::View(ViewEvent::Metrics(metrics)) => {
                    self.canvas
                        .resize(&self.gpu.device, metrics.width, metrics.height);
                    self.view.request_frame();
                }
                AppEvent::View(ViewEvent::CloseRequested) => match self.view.window() {
                    Some(window) => window.close(),
                    None => return Ok(()),
                },
                AppEvent::View(input) => self.egui.hear(input),
                AppEvent::ViewGone => return Ok(()),
                AppEvent::FileChosen(path) => self.open_later(path),
                AppEvent::Brightness(brightness) => {
                    self.theme = theme_of(brightness);
                    self.egui.set_theme(self.theme);
                }
            }
        }
        Ok(())
    }

    /// One frame: the animation moves on, the layout is run, the picture is drawn, and what the
    /// user asked for in it is done.
    fn frame(&mut self, time: u64) {
        self.advance_animation(time);
        let Some(texture) = self.canvas.frame() else {
            return;
        };
        let playback_before = self.shown.playback;
        let (layout, painting) = self.run_layout(time);
        if self.shown.playback != playback_before {
            self.shown.pose(&self.gpu.queue);
        }
        let resolved = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let area = self.area(layout.stage, painting.pixels_per_point);
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        let target = Target {
            canvas: &self.canvas,
            resolved: &resolved,
            area,
        };
        let shown = &self.shown;
        self.scene.draw(
            &self.gpu,
            &mut encoder,
            &target,
            &self.scene_frame(area),
            &shown.gpu_model,
            &shown.world,
        );
        self.panel_painter.paint(
            &self.gpu,
            &mut encoder,
            &resolved,
            self.canvas.size(),
            painting,
        );
        self.gpu.queue.submit([encoder.finish()]);
        self.gpu.queue.present(texture);

        if let Some(action) = layout.action {
            self.act(action);
        }
        if let Some(path) = self.file_to_read.take() {
            self.read(&path);
        }
        if self.shown.is_playing() {
            self.view.request_frame();
        } else {
            self.last_frame = None;
        }
    }

    fn advance_animation(&mut self, time: u64) {
        let since_last = self
            .last_frame
            .replace(time)
            .map(|last| time.saturating_sub(last));
        let step = Duration::from_nanos(since_last.unwrap_or(0)).min(LONGEST_STEP);
        self.shown.advance(&self.gpu.queue, step.as_secs_f32());
    }

    /// Runs the window's layout in egui; what it answers, and what it left to paint.
    fn run_layout(&mut self, time: u64) -> (ui::Layout, crate::egui_view::Painting) {
        let mut layout = None;
        let largest_texture = self.gpu.device.limits().max_texture_dimension_2d;
        let shown = &mut self.shown;
        let (settings, status) = (&mut self.settings, &self.status);
        let painting = self.egui.run(time, largest_texture, |ui| {
            let open_pressed = ui.input_mut(|input| input.consume_shortcut(&OPEN));
            let contents = Contents {
                model: &shown.model,
                playback: &mut shown.playback,
                settings,
                status,
                can_open: open::is_offered(),
            };
            let mut laid_out = ui::show(ui, contents, &mut shown.camera);
            if open_pressed {
                laid_out.action = Some(Action::Open);
            }
            layout = Some(laid_out);
        });
        let layout = layout.expect("egui runs the layout in every pass");
        (layout, painting)
    }

    /// The stage's rectangle in the canvas's pixels, kept inside the canvas.
    fn area(&self, stage: egui::Rect, pixels_per_point: f32) -> Area {
        let [width, height] = self.canvas.size();
        let pixel = |point: f32, limit: u32| {
            ((point * pixels_per_point).round().max(0.0) as u32).min(limit)
        };
        let (left, top) = (pixel(stage.min.x, width), pixel(stage.min.y, height));
        Area {
            x: left,
            y: top,
            width: pixel(stage.max.x, width) - left,
            height: pixel(stage.max.y, height) - top,
        }
    }

    /// The scene's settings for this frame, drawn in `area`.
    fn scene_frame(&self, area: Area) -> Frame {
        let (camera, bounds) = (&self.shown.camera, &self.shown.bounds);
        let colours = theme::stage_colours(self.theme);
        let aspect = area.width.max(1) as f32 / area.height.max(1) as f32;
        let centre = bounds.centre();
        Frame {
            view_projection: camera.view_projection(aspect),
            camera: camera.position(),
            exposure: self.settings.exposure_scale(),
            light: self.settings.light(),
            backdrop: colours.backdrop,
            ground: Ground {
                centre: glam::Vec3::new(centre.x, bounds.min.y, centre.z),
                cell: ground_cell(bounds.radius()),
                reach: bounds.radius() * 7.0,
                ink: colours.ground_ink,
                opacity: if self.settings.ground { 0.4 } else { 0.0 },
            },
            shading: self.settings.shading,
        }
    }

    fn act(&mut self, action: Action) {
        match action {
            Action::Open => open::ask(self.view.clone(), self.sender.clone()),
            Action::ShowSample => {
                self.status = Status::Ready;
                self.show(model::sample::model());
            }
            Action::FrameModel => self.shown.frame(),
        }
        self.view.request_frame();
    }

    /// Says that the file at `path` is being opened, and reads it once that is on screen: reading
    /// keeps the app's one thread until it is done.
    fn open_later(&mut self, path: String) {
        let name = Path::new(&path).file_name().and_then(|name| name.to_str());
        self.status = Status::Opening(name.unwrap_or(&path).to_owned());
        self.file_to_read = Some(path);
        self.view.request_frame();
    }

    fn read(&mut self, path: &str) {
        match model::import::load(path) {
            Ok(model) => {
                self.status = Status::Ready;
                self.show(model);
            }
            Err(reason) => self.status = Status::Failed(reason),
        }
        self.view.request_frame();
    }

    fn show(&mut self, model: Model) {
        window::set_title(&self.view, &model.name);
        self.shown = Shown::new(&self.gpu, &self.scene, model);
        self.last_frame = None;
    }
}

/// The side of a cell of the ground's grid under a model of `radius`: the round length, one, two
/// or five of a power of ten, nearest a quarter of the radius.
fn ground_cell(radius: f32) -> f32 {
    let wanted = radius * 0.25;
    let decade = 10f32.powf(wanted.log10().floor());
    let round_lengths = [1.0, 2.0, 5.0, 10.0].map(|step| step * decade);
    let nearest = |one: &f32, other: &f32| (one - wanted).abs().total_cmp(&(other - wanted).abs());
    round_lengths.into_iter().min_by(nearest).unwrap_or(decade)
}

/// egui on `view`, in the app's look, told the chords the app answers itself.
fn egui_view(view: &SharedView, theme: egui::Theme) -> EguiView {
    let mut egui = EguiView::new(view.clone(), system::os() == OperatingSystem::Macos);
    theme::apply(egui.context());
    egui.set_theme(theme);
    let stage_keys = stage::KEYS.map(|(modifiers, key)| KeyboardShortcut::new(modifiers, key));
    egui.claim_shortcuts(stage_keys.into_iter().chain([OPEN]).collect());
    egui
}

fn theme_of(brightness: Brightness) -> egui::Theme {
    match brightness {
        Brightness::Light => egui::Theme::Light,
        Brightness::Dark => egui::Theme::Dark,
    }
}
