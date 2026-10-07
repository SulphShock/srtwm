use anyhow::Result;
use serde_json::json;
use tracing::{debug, warn};
use x11rb::CURRENT_TIME;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ConfigWindow, ConfigureWindowAux, ConnectionExt,
    EventMask, GrabMode, GrabStatus, NotifyMode, PropMode, StackMode,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use crate::ewmh::{self, Atoms, dock_insets, float_rect, is_dock, title};
use crate::ipc::Ipc;
use crate::keys;
use crate::layout::bsp::Window;
use crate::mouse::{Drag, DragKind};
use crate::state::{Dock, State};
use crate::window::Client;
use crate::wm::{ignore_gone, set_fullscreen};

pub fn handle(
    conn: &RustConnection,
    atoms: &Atoms,
    ipc: &mut Ipc,
    state: &mut State,
    event: Event,
) -> Result<()> {
    match event {
        Event::ConfigureRequest(req) => {
            let mut cfg = ConfigureWindowAux::default();
            if req.value_mask.contains(ConfigWindow::X) {
                cfg.x = Some(req.x as i32);
            }
            if req.value_mask.contains(ConfigWindow::Y) {
                cfg.y = Some(req.y as i32);
            }
            if req.value_mask.contains(ConfigWindow::WIDTH) && req.width != 0 {
                cfg.width = Some(req.width as u32);
            }
            if req.value_mask.contains(ConfigWindow::HEIGHT) && req.height != 0 {
                cfg.height = Some(req.height as u32);
            }
            if req.value_mask.contains(ConfigWindow::BORDER_WIDTH) {
                cfg.border_width = Some(req.border_width as u32);
            }
            ignore_gone(conn.configure_window(req.window, &cfg)?.check())?;
        }
        Event::MapRequest(req) => {
            let win: Window = req.window;
            debug!(window = win, "map request");

            if is_dock(conn, atoms, win)? {
                state.docks.push(Dock {
                    win,
                    insets: dock_insets(conn, atoms, win)?,
                });
                let watch = ChangeWindowAttributesAux {
                    event_mask: Some(EventMask::STRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE),
                    ..Default::default()
                };
                conn.change_window_attributes(win, &watch)?;
                let cfg = ConfigureWindowAux {
                    border_width: Some(0),
                    stack_mode: Some(StackMode::ABOVE),
                    ..Default::default()
                };
                ignore_gone(conn.configure_window(win, &cfg)?.check())?;
                return ignore_gone(conn.map_window(win)?.check());
            }
            let mut cfg = ConfigureWindowAux {
                border_width: Some(state.config.general.border),
                ..Default::default()
            };
            if !state.clients.contains_key(&win) {
                let workspace = state.current;
                let screen = state.work();
                let name = title(conn, atoms, win)?;
                let floating = match float_rect(conn, atoms, win, screen)? {
                    Some(rect) => {
                        cfg.x = Some(rect.x);
                        cfg.y = Some(rect.y);
                        cfg.width = Some(rect.w as u32);
                        cfg.height = Some(rect.h as u32);
                        true
                    }
                    None => {
                        state.tree_mut().insert(win, screen);
                        false
                    }
                };
                let client = Client {
                    workspace,
                    floating,
                    fullscreen: false,
                    restore: None,
                    title: name,
                };
                state.clients.insert(win, client);
                conn.change_property32(
                    PropMode::REPLACE,
                    win,
                    atoms.net_wm_desktop,
                    AtomEnum::CARDINAL,
                    &[workspace as u32],
                )?;
                state.focused = Some(win);
                let watch = ChangeWindowAttributesAux {
                    event_mask: Some(EventMask::ENTER_WINDOW | EventMask::PROPERTY_CHANGE),
                    ..Default::default()
                };
                conn.change_window_attributes(win, &watch)?;
            }
            ignore_gone(conn.configure_window(win, &cfg)?.check())?;
            ignore_gone(conn.map_window(win)?.check())?;
        }
        Event::DestroyNotify(e) => {
            if state.docks.iter().any(|dock| dock.win == e.window) {
                state.docks.retain(|dock| dock.win != e.window);
                return Ok(());
            }
            let Some(client) = state.clients.remove(&e.window) else {
                return Ok(());
            };
            state.workspaces[client.workspace].remove(e.window);
            if state.focused == Some(e.window) {
                state.focused = state.next_focus();
            }
        }
        Event::ConfigureNotify(e) if dock_here(state, e.window) => {
            reserve(conn, atoms, state, e.window)?;
        }
        Event::PropertyNotify(e)
            if e.atom == atoms.net_wm_strut_partial && dock_here(state, e.window) =>
        {
            reserve(conn, atoms, state, e.window)?;
        }
        Event::ButtonPress(e) => {
            let Some(mod_mask) = keys::mask_of(&state.config.general.mod_) else {
                return Ok(());
            };
            if u16::from(e.state) & u16::from(mod_mask) == 0 {
                return Ok(());
            }
            let kind = match e.detail {
                1 => DragKind::Move,
                3 => DragKind::Resize,
                _ => return Ok(()),
            };
            let under = conn.query_pointer(e.root)?.reply()?.child;
            let Some(win) = owner(conn, state, under)? else {
                debug!("drag asked for over no window, ignoring");
                return Ok(());
            };
            let client = &state.clients[&win];
            if !client.floating || client.fullscreen {
                debug!(window = win, floating = client.floating, "drag not allowed here");
                return Ok(());
            }
            debug!(window = win, ?kind, "drag started");

            let Ok(rect) = conn.get_geometry(win)?.reply() else {
                return Ok(());
            };
            state.drag = Some(Drag::start(win, kind, (e.root_x, e.root_y), ewmh::rect(&rect)));
            let grab = conn
                .grab_pointer(
                    false,
                    e.root,
                    EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                    e.root,
                    0u32,
                    CURRENT_TIME,
                )?
                .reply()?;
            if grab.status != GrabStatus::SUCCESS {
                warn!(status = ?grab.status, "pointer grab refused");
            }
        }
        Event::ButtonRelease(_) => {
            if state.drag.take().is_some() {
                ignore_gone(conn.ungrab_pointer(CURRENT_TIME)?.check())?;
            }
        }
        Event::MotionNotify(e) => {
            if let Some(drag) = state.drag.as_mut() {
                drag.track((e.root_x, e.root_y));
            }
        }
        Event::EnterNotify(e) => {
            if state.config.general.focus_follows_mouse
                && e.mode == NotifyMode::NORMAL
                && let Some(win) = owner(conn, state, e.event)?
            {
                state.focused = Some(win);
            }
        }
        Event::ClientMessage(e) if e.type_ == atoms.net_wm_state => {
            let [action, first, ..] = e.data.as_data32();
            if first != atoms.net_wm_state_fullscreen {
                debug!(state = first, "state change this wm does not keep, ignoring");
                return Ok(());
            }
            let fullscreen = match action {
                0 => false,
                1 => true,
                2 => !state.clients.get(&e.window).is_some_and(|c| c.fullscreen),
                _ => return Ok(()),
            };
            set_fullscreen(conn, atoms, state, e.window, fullscreen)?;
        }
        Event::PropertyNotify(e) if e.atom == atoms.net_wm_name || e.atom == atoms.wm_name => {
            let Some(client) = state.clients.get_mut(&e.window) else {
                return Ok(());
            };
            let name = title(conn, atoms, e.window)?;
            if name == client.title {
                return Ok(());
            }
            client.title = name.clone();
            ipc.broadcast("title", json!({ "window": e.window, "title": name }));
        }
        _ => debug!(?event, "unhandled event"),
    }
    Ok(())
}

fn here(state: &State, win: Window) -> Option<&Client> {
    state
        .clients
        .get(&win)
        .filter(|client| client.workspace == state.current)
}

fn dock_here(state: &State, win: Window) -> bool {
    state.docks.iter().any(|dock| dock.win == win)
}

fn reserve(conn: &RustConnection, atoms: &Atoms, state: &mut State, win: Window) -> Result<()> {
    let insets = dock_insets(conn, atoms, win)?;
    if let Some(dock) = state.docks.iter_mut().find(|dock| dock.win == win) {
        dock.insets = insets;
    }
    Ok(())
}

fn owner(conn: &RustConnection, state: &State, win: Window) -> Result<Option<Window>> {
    let mut win = win;
    loop {
        if here(state, win).is_some() {
            return Ok(Some(win));
        }
        let Ok(tree) = conn.query_tree(win)?.reply() else {
            return Ok(None);
        };
        if tree.parent == win {
            return Ok(None);
        }
        win = tree.parent;
    }
}
