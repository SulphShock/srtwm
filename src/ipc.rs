use std::io::{Read, Write};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::net::{UnixListener, UnixStream};

use anyhow::{Context, Result, bail};
use nix::errno::Errno;
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use serde::Deserialize;
use serde_json::Value;
use tracing::warn;

// One request per line: {"cmd":"focus","arg":"left"}, {"query":"tree"} or
// {"subscribe":["workspace","focus"]}.
#[derive(Deserialize)]
pub struct Request {
    #[serde(default)]
    pub cmd: Option<String>,
    #[serde(default)]
    pub arg: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub subscribe: Option<Vec<String>>,
}

pub const TOPICS: [&str; 4] = ["workspace", "focus", "title", "mode"];

pub fn check_topics(topics: &[String]) -> Result<()> {
    for topic in topics {
        if !TOPICS.contains(&topic.as_str()) {
            bail!(
                "no such event {topic:?}, the wm sends {}",
                TOPICS.join(", ")
            );
        }
    }
    Ok(())
}

pub fn socket_path() -> std::path::PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => std::path::PathBuf::from(dir).join("srtwm.sock"),
        None => std::path::PathBuf::from("/tmp/srtwm.sock"),
    }
}

#[derive(Default)]
pub struct Ready {
    pub x: bool,
    pub listener: bool,
    pub clients: Vec<usize>,
}

pub struct Ipc {
    listener: UnixListener,
    clients: Vec<Peer>,
}

struct Peer {
    stream: UnixStream,
    buf: Vec<u8>,
    out: Vec<u8>,
    topics: Vec<String>,
    gone: bool,
}

impl Ipc {
    pub fn listen() -> Result<Self> {
        let path = socket_path();
        let _ = std::fs::remove_file(&path);
        let listener =
            UnixListener::bind(&path).with_context(|| format!("binding {}", path.display()))?;
        listener.set_nonblocking(true)?;
        Ok(Ipc {
            listener,
            clients: Vec::new(),
        })
    }

    pub fn poll(&mut self, x: BorrowedFd<'_>) -> Result<Ready> {
        // a client that hung up is dropped here, so the vector and the poll
        // list stay in step and an index means the same thing on both sides
        self.clients.retain(|c| !c.gone);

        let mut fds = vec![
            PollFd::new(x, PollFlags::POLLIN),
            PollFd::new(self.listener.as_fd(), PollFlags::POLLIN),
        ];
        for client in &self.clients {
            let mut flags = PollFlags::POLLIN;
            if !client.out.is_empty() {
                flags |= PollFlags::POLLOUT;
            }
            fds.push(PollFd::new(client.stream.as_fd(), flags));
        }

        let mut ready = Ready::default();
        match poll(&mut fds, PollTimeout::NONE) {
            Ok(_) => {}
            Err(Errno::EINTR) => return Ok(ready),
            Err(e) => return Err(e.into()),
        }

        let hit = |idx: usize| fds[idx].revents().is_some_and(|r| !r.is_empty());
        ready.x = hit(0);
        ready.listener = hit(1);
        ready.clients = (2..fds.len())
            .filter(|idx| hit(*idx))
            .map(|idx| idx - 2)
            .collect();
        Ok(ready)
    }

    pub fn accept(&mut self) {
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    let _ = stream.set_nonblocking(true);
                    self.clients.push(Peer {
                        stream,
                        buf: Vec::new(),
                        out: Vec::new(),
                        topics: Vec::new(),
                        gone: false,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return,
                Err(e) => {
                    warn!(error = %e, "accept failed");
                    return;
                }
            }
        }
    }

    pub fn read(&mut self, idx: usize) -> Vec<Request> {
        let mut chunk = [0u8; 4096];
        loop {
            match self.clients[idx].stream.read(&mut chunk) {
                Ok(0) => {
                    self.clients[idx].gone = true;
                    break;
                }
                Ok(n) => self.clients[idx].buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => self.clients[idx].gone = true,
            }
        }

        if self.clients[idx].buf.len() > 64 * 1024 {
            self.clients[idx].gone = true;
            return Vec::new();
        }

        let buf = std::mem::take(&mut self.clients[idx].buf);
        let mut requests = Vec::new();
        for line in buf.split(|b| *b == b'\n') {
            if line.is_empty() {
                continue;
            }
            match serde_json::from_slice::<Request>(line) {
                Ok(request) => requests.push(request),
                Err(err) => self.reply(idx, error(&err.to_string())),
            }
        }
        requests
    }

    pub fn reply(&mut self, idx: usize, value: Value) {
        let mut line = value.to_string();
        line.push('\n');
        self.clients[idx].out.extend_from_slice(line.as_bytes());
    }

    pub fn set_topics(&mut self, idx: usize, topics: Vec<String>) {
        self.clients[idx].topics = topics;
    }

    pub fn broadcast(&mut self, event: &str, mut value: Value) {
        if let Some(fields) = value.as_object_mut() {
            fields.insert("event".to_string(), Value::String(event.to_string()));
        }
        let line = format!("{value}\n");
        for idx in 0..self.clients.len() {
            if self.clients[idx].gone || !self.clients[idx].topics.iter().any(|t| t == event) {
                continue;
            }
            self.clients[idx].out.extend_from_slice(line.as_bytes());
            self.flush(idx);
        }
    }

    pub fn flush(&mut self, idx: usize) {
        let out = std::mem::take(&mut self.clients[idx].out);
        let mut out = out.as_slice();
        while !out.is_empty() {
            match self.clients[idx].stream.write(out) {
                Ok(0) => {
                    self.clients[idx].gone = true;
                    return;
                }
                Ok(n) => out = &out[n..],
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    self.clients[idx].gone = true;
                    return;
                }
            }
        }
        if !out.is_empty() {
            self.clients[idx].out.extend_from_slice(out);
        }
    }
}

pub fn ok() -> Value {
    serde_json::json!({ "ok": true })
}

pub fn data(value: Value) -> Value {
    serde_json::json!({ "ok": true, "data": value })
}

pub fn error(message: &str) -> Value {
    serde_json::json!({ "ok": false, "error": message })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_line_parses() {
        let request: Request =
            serde_json::from_str(r#"{"cmd":"focus","arg":"left"}"#).expect("parses");
        assert_eq!(request.cmd.as_deref(), Some("focus"));
        assert_eq!(request.arg.as_deref(), Some("left"));
        assert!(request.query.is_none());
    }

    #[test]
    fn a_query_line_parses() {
        let request: Request = serde_json::from_str(r#"{"query":"tree"}"#).expect("parses");
        assert_eq!(request.query.as_deref(), Some("tree"));
        assert!(request.cmd.is_none());
    }

    #[test]
    fn a_subscribe_line_parses() {
        let request: Request =
            serde_json::from_str(r#"{"subscribe":["workspace","focus"]}"#).expect("parses");
        let topics = request.subscribe.expect("topics");
        assert_eq!(topics, ["workspace", "focus"]);
        assert!(request.cmd.is_none());
    }

    #[test]
    fn a_topic_the_wm_never_sends_is_refused() {
        assert!(check_topics(&["focus".to_string()]).is_ok());
        assert!(check_topics(&["colour".to_string()]).is_err());
    }

    #[test]
    fn a_line_that_is_not_a_request_is_refused() {
        assert!(serde_json::from_str::<Request>("hello").is_err());
    }

    #[test]
    fn the_answers_say_what_went_wrong() {
        assert_eq!(ok(), serde_json::json!({"ok": true}));
        assert_eq!(
            error("no such query"),
            serde_json::json!({"ok": false, "error": "no such query"})
        );
    }
}
