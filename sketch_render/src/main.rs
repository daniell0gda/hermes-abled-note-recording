use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use sketch_render::{page, render, Params, View};

const USAGE: &str = "usage: sketch-render <view.json|-> [--out <file.html|file.svg>] [--params <json>] [--report]";

struct Options {
    input: String,
    out: Option<PathBuf>,
    params: Params,
    report: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let options = parse_args(std::env::args().skip(1).collect())?;
    let text = read_input(&options.input)?;
    let view: View = serde_json::from_str(&text).map_err(|error| format!("invalid view JSON: {error}"))?;
    let rendered = render(&view, &options.params);
    if options.report {
        let report = serde_json::to_string(&rendered.report).map_err(|error| error.to_string())?;
        eprintln!("{report}");
    }
    match &options.out {
        Some(path) if path.extension().is_some_and(|extension| extension == "html") => {
            std::fs::write(path, page(&view, &rendered, options.params.theme)).map_err(|error| error.to_string())
        }
        Some(path) => std::fs::write(path, &rendered.svg).map_err(|error| error.to_string()),
        None => {
            println!("{}", rendered.svg);
            Ok(())
        }
    }
}

fn parse_args(args: Vec<String>) -> Result<Options, String> {
    let mut input = None;
    let mut out = None;
    let mut params = Params::default();
    let mut report = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => out = Some(PathBuf::from(args.next().ok_or(USAGE)?)),
            "--params" => {
                let json = args.next().ok_or(USAGE)?;
                params = serde_json::from_str(&json).map_err(|error| format!("invalid params JSON: {error}"))?;
            }
            "--report" => report = true,
            _ if input.is_none() => input = Some(arg),
            _ => return Err(USAGE.to_string()),
        }
    }
    Ok(Options { input: input.ok_or(USAGE)?, out, params, report })
}

fn read_input(input: &str) -> Result<String, String> {
    if input == "-" {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text).map_err(|error| error.to_string())?;
        return Ok(text);
    }
    std::fs::read_to_string(input).map_err(|error| format!("{input}: {error}"))
}
