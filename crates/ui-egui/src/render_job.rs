//! Render… in the background: Write nodes render frame by frame on their own thread from a copy of
//! the script, so the window stays responsive, shows progress and can cancel. Edits made while it
//! runs don't change what it renders (it has its own copy), as with a Nuke background render.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use nukecraft_graph::{NodeId, Script};
use nukecraft_render::RENDER_STACK;

/// What a render job has done so far.
#[derive(Clone, Debug, Default)]
pub struct Progress {
    /// Frames rendered (counting each Write's frame).
    pub done: usize,
    pub total: usize,
    /// What is rendering now (`Write1 frame 12`).
    pub current: String,
    pub written: Vec<String>,
    pub error: Option<String>,
    pub finished: bool,
    pub cancelled: bool,
}

pub struct RenderJob {
    progress: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
}

impl RenderJob {
    /// Starts rendering `writes` at `frames` (frame by frame, every Write per frame, as Nuke
    /// does). Where no thread can start, renders here and returns the finished job.
    pub fn start(script: Script, writes: Vec<NodeId>, frames: Vec<i64>, repaint: Option<egui::Context>) -> RenderJob {
        let total = writes.len().saturating_mul(frames.len());
        let progress = Arc::new(Mutex::new(Progress { total, ..Progress::default() }));
        let cancel = Arc::new(AtomicBool::new(false));
        let job = RenderJob { progress: progress.clone(), cancel: cancel.clone() };
        let work = move || {
            run(&script, &writes, &frames, &progress, &cancel, repaint.as_ref());
        };
        let spawned = std::thread::Builder::new().name("render-job".into()).stack_size(RENDER_STACK).spawn(work);
        if let Err(e) = spawned {
            log::warn!("no render thread ({e})");
            job.update(|p| {
                p.error = Some(format!("could not start a render thread: {e}"));
                p.finished = true;
            });
        }
        job
    }

    pub fn progress(&self) -> Progress {
        self.progress.lock().map(|p| p.clone()).unwrap_or_else(|e| e.into_inner().clone())
    }

    /// Asks the job to stop after the frame it is rendering.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    fn update(&self, f: impl FnOnce(&mut Progress)) {
        update(&self.progress, f);
    }
}

fn update(progress: &Mutex<Progress>, f: impl FnOnce(&mut Progress)) {
    match progress.lock() {
        Ok(mut p) => f(&mut p),
        Err(e) => f(&mut e.into_inner()),
    }
}

fn run(script: &Script, writes: &[NodeId], frames: &[i64], progress: &Mutex<Progress>, cancel: &AtomicBool, repaint: Option<&egui::Context>) {
    let registry = nukecraft_io::full_registry();
    'frames: for &frame in frames {
        for &w in writes {
            if cancel.load(Ordering::Relaxed) {
                update(progress, |p| p.cancelled = true);
                break 'frames;
            }
            let name = script.name_of(w).unwrap_or("?").to_string();
            update(progress, |p| p.current = format!("{name} frame {frame}"));
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| nukecraft_io::execute_write_frame(script, &registry, w, frame)));
            let failed = match r {
                Ok(Ok(path)) => {
                    update(progress, |p| {
                        p.done += 1;
                        p.written.extend(path.map(|p| p.display().to_string()));
                    });
                    None
                }
                Ok(Err(e)) => Some(format!("{name} frame {frame}: {e}")),
                Err(_) => Some(format!("{name} frame {frame}: internal error while rendering")),
            };
            if let Some(e) = failed {
                update(progress, |p| p.error = Some(e));
                break 'frames;
            }
            if let Some(ctx) = repaint {
                ctx.request_repaint();
            }
        }
    }
    update(progress, |p| p.finished = true);
    if let Some(ctx) = repaint {
        ctx.request_repaint();
    }
}

/// The progress window while a job runs; when it ends, a status message and the job is dropped.
pub fn show(app: &mut crate::NukecraftApp, ctx: &egui::Context) {
    let Some(job) = &app.render_job else { return };
    let p = job.progress();
    if p.finished {
        let msg = match (&p.error, p.cancelled) {
            (Some(e), _) => (format!("Render failed: {e}"), true),
            (None, true) => (format!("Render cancelled after {} of {} frames", p.done, p.total), false),
            (None, false) => (format!("Rendered {} files", p.written.len()), false),
        };
        app.render_job = None;
        app.set_status(msg.0, msg.1);
        return;
    }
    let mut cancel = false;
    egui::Window::new("Rendering")
        .id(egui::Id::new("render.progress"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -40.0))
        .show(ctx, |ui| {
            ui.label(&p.current);
            let frac = if p.total > 0 { p.done as f32 / p.total as f32 } else { 0.0 };
            let bar = ui.add(egui::ProgressBar::new(frac).desired_width(240.0).text(format!("{} / {}", p.done, p.total)));
            app.automation.add("render.progress", bar.rect, "render progress");
            let b = ui.button("Cancel");
            app.automation.add("render.cancel", b.rect, "Cancel");
            cancel = b.clicked();
        });
    if cancel && let Some(job) = &app.render_job {
        job.cancel();
    }
}
