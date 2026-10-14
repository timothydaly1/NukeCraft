//! The render thread behind the Viewer. Requests carry a copy of the script; only the newest
//! request waiting is rendered (older ones are dropped), so scrubbing and dragging sliders never
//! queue up stale work. Results come back with the script revision they were made for.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use nukecraft_graph::{NodeId, Script};
use nukecraft_nodes::ImageInfo;
use nukecraft_raster::Image;
use nukecraft_render::{RENDER_STACK, Renderer};
use nukecraft_time::FrameTime;

/// What the Viewer shows: a node at a time, at one script revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub revision: u64,
    pub node: NodeId,
    pub time: FrameTime,
}

/// A rendered frame: the node's info and its pixels over the format.
pub struct Frame {
    pub info: ImageInfo,
    pub image: Image,
    pub millis: f64,
}

pub struct Done {
    pub key: Key,
    pub result: Result<Arc<Frame>, String>,
}

struct Request {
    key: Key,
    script: Script,
}

pub struct Worker {
    tx: Sender<Request>,
    rx: Receiver<Done>,
    in_flight: usize,
}

impl Worker {
    /// Starts the render thread; `None` where threads can't start (the caller renders inline).
    pub fn spawn(ctx: egui::Context) -> Option<Worker> {
        let (tx, requests) = channel::<Request>();
        let (results, rx) = channel::<Done>();
        std::thread::Builder::new()
            .name("viewer-render".into())
            .stack_size(RENDER_STACK)
            .spawn(move || {
                let registry = nukecraft_io::full_registry();
                while let Ok(mut req) = requests.recv() {
                    // Latest wins: skip everything but the newest waiting request.
                    let mut skipped = 0;
                    while let Ok(newer) = requests.try_recv() {
                        req = newer;
                        skipped += 1;
                    }
                    for _ in 0..skipped {
                        let _ = results.send(Done { key: req.key, result: Err(String::new()) });
                    }
                    let result = render(&req.script, &registry, req.key);
                    if results.send(Done { key: req.key, result }).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .ok()?;
        Some(Worker { tx, rx, in_flight: 0 })
    }

    pub fn request(&mut self, key: Key, script: Script) {
        if self.tx.send(Request { key, script }).is_ok() {
            self.in_flight += 1;
        }
    }

    /// Finished renders (skipped requests come back as empty errors and are not returned).
    pub fn poll(&mut self) -> Vec<Done> {
        let mut out = Vec::new();
        while let Ok(done) = self.rx.try_recv() {
            self.in_flight = self.in_flight.saturating_sub(1);
            if !matches!(&done.result, Err(e) if e.is_empty()) {
                out.push(done);
            }
        }
        out
    }

    pub fn busy(&self) -> bool {
        self.in_flight > 0
    }
}

/// Renders one frame, turning errors and panics into messages.
pub fn render(script: &Script, registry: &nukecraft_nodes::Registry, key: Key) -> Result<Arc<Frame>, String> {
    let start = web_time_now();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Renderer::new(script, registry).render_format(key.node, key.time)));
    match r {
        Ok(Ok((info, image))) => Ok(Arc::new(Frame { info, image, millis: elapsed_ms(start) })),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("internal error while rendering (the script is unchanged)".into()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn web_time_now() -> Option<std::time::Instant> {
    Some(std::time::Instant::now())
}

#[cfg(target_arch = "wasm32")]
fn web_time_now() -> Option<std::time::Instant> {
    None
}

fn elapsed_ms(start: Option<std::time::Instant>) -> f64 {
    start.map_or(0.0, |s| s.elapsed().as_secs_f64() * 1000.0)
}
