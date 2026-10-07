use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use srtwm::actions::Action;
use srtwm::ipc::{TOPICS, socket_path};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("srtwmctl: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (request, follow) = match args.split_first() {
        Some((first, rest)) if first == "subscribe" => {
            let topics: Vec<&str> = if rest.is_empty() {
                TOPICS.to_vec()
            } else {
                rest.iter().map(|topic| topic.as_str()).collect()
            };
            (json!({ "subscribe": topics }), true)
        }
        Some((first, rest)) if first == "query" => (json!({ "query": rest.join(" ") }), false),
        Some((first, rest)) => (json!({ "cmd": first, "arg": rest.join(" ") }), false),
        None => bail!("usage: srtwmctl <action> [arg] | query <tree|workspaces|focused> | subscribe [topics...]"),
    };
    if let Some(cmd) = request["cmd"].as_str() {
        let arg = request["arg"].as_str().unwrap_or_default();
        let spec = if arg.is_empty() { cmd.to_string() } else { format!("{cmd}:{arg}") };
        if Action::parse(&spec).is_none() {
            bail!("cannot read {spec:?} as an action");
        }
    }

    let path = socket_path();
    let mut stream =
        UnixStream::connect(&path).with_context(|| format!("connecting to {}", path.display()))?;
    writeln!(stream, "{request}")?;

    let mut reader = BufReader::new(stream.try_clone()?);
    if follow {
        for line in reader.lines() {
            let line = line?;
            let value: Value = serde_json::from_str(&line).context("reading the stream")?;
            if value["ok"] == Value::Bool(false) {
                bail!("{}", value["error"].as_str().unwrap_or("the wm said no"));
            }
            println!("{line}");
        }
        return Ok(());
    }

    let mut line = String::new();
    reader.read_line(&mut line)?;
    let reply: Value = serde_json::from_str(line.trim()).context("reading the answer")?;

    if reply["ok"].as_bool() != Some(true) {
        bail!("{}", reply["error"].as_str().unwrap_or("the wm said no"));
    }
    match reply.get("data") {
        Some(data) => println!("{}", serde_json::to_string_pretty(data)?),
        None => println!("ok"),
    }
    Ok(())
}
