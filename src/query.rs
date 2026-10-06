use anyhow::{Result, bail};
use serde_json::{Value, json};
use tracing::debug;
use x11rb::protocol::xproto::{ConnectionExt, Window};
use x11rb::rust_connection::{ReplyError, RustConnection};

use crate::ewmh;
use crate::geometry::Rect;
use crate::layout::apply_layout;
use crate::state::{State, WORKSPACES};

pub fn run(name: &str, conn: &RustConnection, state: &State) -> Result<Value> {
    match name {
        "tree" => Ok(tree(conn, state)),
        "workspaces" => Ok(workspaces(state)),
        "focused" => Ok(focused(state)),
        other => bail!("no such query {other:?}"),
    }
}

fn tree(conn: &RustConnection, state: &State) -> Value {
    let gap = state.config.general.gap;
    let mut windows: Vec<Value> = apply_layout(state.tree(), state.work(), gap)
        .into_iter()
        .map(|(win, rect)| entry(state, win, rect))
        .collect();
    for win in state.floating() {
        windows.push(entry(state, win, geometry(conn, win)));
    }
    json!({ "workspace": state.current, "windows": windows })
}

fn workspaces(state: &State) -> Value {
    let list: Vec<Value> = (0..WORKSPACES)
        .map(|index| {
            let windows = state.windows(index);
            json!({ "index": index, "windows": windows, "occupied": !windows.is_empty() })
        })
        .collect();
    json!({ "current": state.current, "workspaces": list })
}

fn focused(state: &State) -> Value {
    let Some(win) = state.focused else {
        return Value::Null;
    };
    let client = state.clients.get(&win);
    json!({
        "window": win,
        "workspace": client.map(|c| c.workspace),
        "floating": client.is_some_and(|c| c.floating),
        "fullscreen": client.is_some_and(|c| c.fullscreen),
    })
}

fn entry(state: &State, win: Window, rect: Rect) -> Value {
    let client = state.clients.get(&win);
    json!({
        "window": win,
        "x": rect.x,
        "y": rect.y,
        "width": rect.w,
        "height": rect.h,
        "floating": client.is_some_and(|c| c.floating),
        "fullscreen": client.is_some_and(|c| c.fullscreen),
        "focused": state.focused == Some(win),
    })
}

fn geometry(conn: &RustConnection, win: Window) -> Rect {
    match conn
        .get_geometry(win)
        .map_err(ReplyError::from)
        .and_then(|cookie| cookie.reply())
    {
        Ok(g) => ewmh::rect(&g),
        Err(err) => {
            debug!(window = win, error = %err, "geometry unavailable");
            Rect::new(0, 0, 0, 0)
        }
    }
}
