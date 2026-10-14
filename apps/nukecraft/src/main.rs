//! `nukecraft`: the desktop app.
//!
//! ```text
//! nukecraft [SCRIPT.ncs | --demo] [--control PORT] [-v]
//! ```

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::process::ExitCode;

use eframe::egui;
use nukecraft_engine::Session;
use nukecraft_engine::demo::demo_session;
use nukecraft_ui_egui::NukecraftApp;
use serde_json::json;

/// eframe's view of the app.
struct Shell(NukecraftApp);

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }

    /// Input from the control channel joins the window's own.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.events.extend(self.0.take_injected());
    }
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let verbose = take_flag(&mut args, &["-v", "--verbose"]);
    init_logging(if verbose { log::LevelFilter::Info } else { log::LevelFilter::Warn });
    if take_flag(&mut args, &["-h", "--help"]) {
        println!("usage: nukecraft [SCRIPT.ncs | --demo] [--control PORT] [-v]");
        return ExitCode::SUCCESS;
    }
    let control = match take_value(&mut args, "--control") {
        Ok(None) => None,
        Ok(Some(p)) => match p.parse::<u16>() {
            Ok(port) => Some(port),
            Err(_) => {
                eprintln!("error: --control takes a port number");
                return ExitCode::from(2);
            }
        },
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let demo = take_flag(&mut args, &["--demo"]);
    let session = if demo {
        demo_session()
    } else if let Some(path) = args.first() {
        let mut s = Session::new();
        s.execute("file.open", &json!({ "path": path })).map(|_| s)
    } else {
        Ok(Session::new())
    };
    let session = match session {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("NukeCraft").with_inner_size([1600.0, 1000.0]).with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    // Windowing libraries can panic when the system lacks something (a display, an X11 library);
    // that is an error to report, not a crash.
    std::panic::set_hook(Box::new(|info| log::error!("{info}")));
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        eframe::run_native(
            "NukeCraft",
            options,
            Box::new(move |cc| {
                let mut app = NukecraftApp::new(&cc.egui_ctx, session);
                if let Some(port) = control {
                    match app.start_control(&cc.egui_ctx, port) {
                        Ok(p) => eprintln!("control channel: 127.0.0.1:{p}"),
                        Err(e) => eprintln!("error: {e}"),
                    }
                }
                Ok(Box::new(Shell(app)))
            }),
        )
    }));
    match run {
        Ok(Ok(())) => ExitCode::SUCCESS,
        Ok(Err(e)) => {
            eprintln!("error: could not start the window: {e}");
            ExitCode::FAILURE
        }
        Err(_) => {
            eprintln!("error: the window system failed (details above); on Linux check that a display is available and that libxkbcommon-x11 is installed");
            ExitCode::FAILURE
        }
    }
}

fn take_value(args: &mut Vec<String>, name: &str) -> Result<Option<String>, String> {
    let Some(i) = args.iter().position(|a| a == name) else { return Ok(None) };
    args.remove(i);
    if i < args.len() { Ok(Some(args.remove(i))) } else { Err(format!("{name} needs a value")) }
}

fn take_flag(args: &mut Vec<String>, names: &[&str]) -> bool {
    let before = args.len();
    args.retain(|a| !names.contains(&a.as_str()));
    args.len() != before
}

/// A minimal stderr logger.
struct StderrLog;

impl log::Log for StderrLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::max_level()
    }

    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("[{}] {}", r.level().as_str().to_ascii_lowercase(), r.args());
        }
    }

    fn flush(&self) {}
}

static LOGGER: StderrLog = StderrLog;

fn init_logging(level: log::LevelFilter) {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(level);
    }
}
