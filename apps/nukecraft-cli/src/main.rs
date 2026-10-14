//! `nukecraft-cli`: NukeCraft from the command line. Every subcommand goes through the same
//! engine commands as the UI and the MCP server; results are JSON on stdout, errors a message
//! on stderr and a non-zero exit code.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::process::ExitCode;

use nukecraft_automation::Server;
use nukecraft_engine::Session;
use nukecraft_engine::demo::demo_session;
use serde_json::{Value, json};

const USAGE: &str = "\
nukecraft-cli — NukeCraft from the command line

USAGE:
  nukecraft-cli demo OUT.ncs                          write the demo script
  nukecraft-cli new OUT.ncs [--format NAME|WxH]       write an empty script
  nukecraft-cli exec COMMAND [JSON] [SCRIPT.ncs] [--save]
                                                      run one engine command (on SCRIPT if given)
  nukecraft-cli render SCRIPT.ncs [--write W1,W2] [--frames 1-10]
                                                      render Write nodes
  nukecraft-cli frame SCRIPT.ncs --node NAME --out FILE.png|.exr [--frame N]
                                                      render one node at one frame
  nukecraft-cli info SCRIPT.ncs --node NAME [--frame N]
  nukecraft-cli nodes                                 node classes and their knobs (JSON)
  nukecraft-cli commands                              engine commands (JSON)
  nukecraft-cli mcp [SCRIPT.ncs | --demo]             MCP server on stdin/stdout

OPTIONS:
  -v, --verbose    log progress to stderr
  -h, --help       this text
  -V, --version    the version
";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let verbose = take_flag(&mut args, &["-v", "--verbose"]);
    init_logging(if verbose { log::LevelFilter::Info } else { log::LevelFilter::Warn });
    if args.is_empty() || take_flag(&mut args, &["-h", "--help"]) {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if take_flag(&mut args, &["-V", "--version"]) {
        println!("nukecraft-cli {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    match run(args) {
        Ok(Some(v)) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string()));
            ExitCode::SUCCESS
        }
        Ok(None) => ExitCode::SUCCESS,
        Err(Failure::Usage(msg)) => {
            eprintln!("error: {msg}\n\n{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Run(msg)) => {
            eprintln!("error: {msg}");
            ExitCode::FAILURE
        }
    }
}

enum Failure {
    Usage(String),
    Run(String),
}

impl From<nukecraft_engine::EngineError> for Failure {
    fn from(e: nukecraft_engine::EngineError) -> Failure {
        Failure::Run(e.to_string())
    }
}

fn usage(msg: impl Into<String>) -> Failure {
    Failure::Usage(msg.into())
}

fn run(mut args: Vec<String>) -> Result<Option<Value>, Failure> {
    let sub = args.remove(0);
    match sub.as_str() {
        "demo" => {
            let out = positional(&mut args, "OUT.ncs")?;
            let mut s = demo_session()?;
            Ok(Some(s.execute("file.save", &json!({ "path": out }))?))
        }
        "new" => {
            let format = take_opt(&mut args, "--format")?;
            let out = positional(&mut args, "OUT.ncs")?;
            let mut s = Session::new();
            s.execute("file.new", &json!({ "format": format }))?;
            Ok(Some(s.execute("file.save", &json!({ "path": out }))?))
        }
        "exec" => {
            let save = take_flag(&mut args, &["--save"]);
            let command = positional(&mut args, "COMMAND")?;
            let params: Value = match args.first() {
                Some(a) if a.trim_start().starts_with('{') => {
                    let text = args.remove(0);
                    serde_json::from_str(&text).map_err(|e| usage(format!("parameters are not JSON: {e}")))?
                }
                _ => json!({}),
            };
            let mut s = Session::new();
            let script = args.first().cloned();
            if let Some(path) = &script {
                s.execute("file.open", &json!({ "path": path }))?;
            }
            let result = s.execute(&command, &params)?;
            if save {
                if script.is_none() {
                    return Err(usage("--save needs a SCRIPT.ncs"));
                }
                s.execute("file.save", &json!({}))?;
            }
            Ok(Some(result))
        }
        "render" => {
            let writes = take_opt(&mut args, "--write")?;
            let frames = take_opt(&mut args, "--frames")?;
            let s = open(&mut args)?;
            let mut s = s;
            let mut p = json!({});
            if let Some(w) = writes {
                p["nodes"] = json!(w.split(',').map(str::trim).collect::<Vec<_>>());
            }
            if let Some(f) = frames {
                p["frames"] = json!(f);
            }
            Ok(Some(s.execute("render.execute", &p)?))
        }
        "frame" => {
            let node = take_opt(&mut args, "--node")?.ok_or_else(|| usage("--node is required"))?;
            let out = take_opt(&mut args, "--out")?.ok_or_else(|| usage("--out is required"))?;
            let frame = frame_opt(&mut args)?;
            let mut s = open(&mut args)?;
            Ok(Some(s.execute("render.frame", &json!({ "node": node, "path": out, "frame": frame }))?))
        }
        "info" => {
            let node = take_opt(&mut args, "--node")?.ok_or_else(|| usage("--node is required"))?;
            let frame = frame_opt(&mut args)?;
            let mut s = open(&mut args)?;
            Ok(Some(s.execute("query.info", &json!({ "node": node, "frame": frame }))?))
        }
        "nodes" => Ok(Some(Session::new().execute("query.nodeClasses", &json!({}))?)),
        "commands" => Ok(Some(Session::new().execute("query.commands", &json!({}))?)),
        "mcp" => {
            let demo = take_flag(&mut args, &["--demo"]);
            let session = if demo {
                demo_session()?
            } else if let Some(path) = args.first() {
                let mut s = Session::new();
                s.execute("file.open", &json!({ "path": path }))?;
                s
            } else {
                Session::new()
            };
            let stdin = std::io::stdin();
            Server::new(session).serve(stdin.lock(), std::io::stdout().lock()).map_err(|e| Failure::Run(e.to_string()))?;
            Ok(None)
        }
        other => Err(usage(format!("unknown subcommand `{other}`"))),
    }
}

/// Opens the script named by the first positional argument.
fn open(args: &mut Vec<String>) -> Result<Session, Failure> {
    let path = positional(args, "SCRIPT.ncs")?;
    let mut s = Session::new();
    s.execute("file.open", &json!({ "path": path }))?;
    Ok(s)
}

fn positional(args: &mut Vec<String>, name: &str) -> Result<String, Failure> {
    match args.iter().position(|a| !a.starts_with("--")) {
        Some(i) => Ok(args.remove(i)),
        None => Err(usage(format!("{name} is required"))),
    }
}

fn take_flag(args: &mut Vec<String>, names: &[&str]) -> bool {
    let before = args.len();
    args.retain(|a| !names.contains(&a.as_str()));
    args.len() != before
}

fn take_opt(args: &mut Vec<String>, name: &str) -> Result<Option<String>, Failure> {
    let Some(i) = args.iter().position(|a| a == name) else { return Ok(None) };
    args.remove(i);
    if i < args.len() { Ok(Some(args.remove(i))) } else { Err(usage(format!("{name} needs a value"))) }
}

fn frame_opt(args: &mut Vec<String>) -> Result<Option<f64>, Failure> {
    take_opt(args, "--frame")?.map(|f| f.parse::<f64>().map_err(|_| usage(format!("--frame `{f}` is not a number")))).transpose()
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
