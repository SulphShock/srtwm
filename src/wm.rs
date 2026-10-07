use std::collections::HashMap;
use std::os::fd::AsFd;

use anyhow::{Result, bail};
use serde_json::json;
use tracing::{debug, info, trace, warn};
use x11rb::CURRENT_TIME;
use x11rb::connection::Connection;
use x11rb::errors::ReplyError;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ClientMessageData, ClientMessageEvent, ConfigureWindowAux,
    ConnectionExt, EventMask, InputFocus, MAP_REQUEST_EVENT, MapRequestEvent, ModMask, PropMode,
    StackMode, Window,
};
use x11rb::protocol::{ErrorKind, Event};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use crate::actions::Action;
use crate::config::Config;
use crate::event;
use crate::ewmh::{self, Atoms, desktop};
use crate::geometry::Rect;
use crate::ipc::{self, Ipc, Request};
use crate::keys::{self, Binds};
use crate::layout::apply_layout;
use crate::query;
use crate::state::{Mode, State, WORKSPACES};

pub fn ignore_gone(result: Result<(), ReplyError>) -> Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(ReplyError::X11Error(e)) if e.error_kind == ErrorKind::Window => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub struct Wm {
    conn: RustConnection,
    root: Window,
    atoms: Atoms,
    binds: Binds,
    ipc: Ipc,
}

impl Wm {
    pub fn new(conn: RustConnection, root: Window, config: &Config) -> Result<Self> {
        let attrs = ChangeWindowAttributesAux {
            event_mask: Some(
                EventMask::SUBSTRUCTURE_REDIRECT
                    | EventMask::SUBSTRUCTURE_NOTIFY
                    | EventMask::ENTER_WINDOW,
            ),
            ..Default::default()
        };
        match conn.change_window_attributes(root, &attrs)?.check() {
            Ok(()) => {}
            Err(ReplyError::X11Error(e)) if e.error_kind == ErrorKind::Access => {
                bail!("another window manager is running")
            }
            Err(e) => return Err(e.into()),
        }

        let atoms = Atoms::new(&conn)?;
        let binds = keys::grab(&conn, root, &config.binds, &config.general.mod_)?;
        keys::grab_buttons(&conn, root, config)?;
        info!(binds = binds.len(), "keys grabbed");

        for (atom, value) in [
            (atoms.net_number_of_desktops, WORKSPACES as u32),
            (atoms.net_current_desktop, 0),
        ] {
            conn.change_property32(PropMode::REPLACE, root, atom, AtomEnum::CARDINAL, &[value])?;
        }

        let ipc = Ipc::listen()?;
        info!(path = %ipc::socket_path().display(), "ipc listening");

        info!(root, "managing root");
        Ok(Wm { conn, root, atoms, binds, ipc })
    }

    pub fn run(&mut self, state: &mut State) -> Result<()> {
        self.adopt(state)?;
        loop {
            let before = (state.current, state.focused);
            let ready = self.ipc.poll(self.conn.stream().as_fd())?;
            if ready.listener {
                self.ipc.accept();
            }
            for idx in ready.clients {
                for request in self.ipc.read(idx) {
                    self.answer(idx, state, request)?;
                }
                self.ipc.flush(idx);
            }

            if ready.x {
                while let Some(event) = self.conn.poll_for_event()? {
                    if let Event::KeyPress(e) = event {
                        let mods = ModMask::from(u16::from(e.state));
                        if let Some(action) = self.binds.get(&(e.detail, mods)) {
                            self.run_action(state, action.clone())?;
                        } else {
                            trace!(keycode = e.detail, ?mods, "unbound key");
                        }
                        continue;
                    }
                    event::handle(&self.conn, &self.atoms, &mut self.ipc, state, event)?;
                }
            }
            self.announce(state, before);
            self.apply_layout(state)?;
            self.set_focus(state)?;
            self.conn.flush()?;
        }
    }

    fn adopt(&mut self, state: &mut State) -> Result<()> {
        let mapped = self.conn.query_tree(self.root)?.reply()?.children;
        if mapped.is_empty() {
            return Ok(());
        }
        info!(count = mapped.len(), "adopting windows mapped before us");

        let wanted: Vec<(Window, Option<usize>)> = mapped
            .iter()
            .map(|win| Ok((*win, desktop(&self.conn, &self.atoms, *win)?)))
            .collect::<Result<_>>()?;

        for win in mapped {
            let event = Event::MapRequest(MapRequestEvent {
                response_type: MAP_REQUEST_EVENT,
                sequence: 0,
                parent: self.root,
                window: win,
            });
            event::handle(&self.conn, &self.atoms, &mut self.ipc, state, event)?;
        }
        for (win, to) in wanted {
            let Some(to) = to else { continue };
            if to != state.current && state.clients.contains_key(&win) {
                state.focused = Some(win);
                self.send(state, to)?;
            }
        }
        state.focused = state.next_focus();
        self.set_desktop(state.current)?;
        Ok(())
    }

    fn announce(&mut self, state: &State, before: (usize, Option<Window>)) {
        if before.0 != state.current {
            let occupied: Vec<usize> = (0..WORKSPACES)
                .filter(|index| !state.windows(*index).is_empty())
                .collect();
            self.ipc
                .broadcast("workspace", json!({ "current": state.current, "occupied": occupied }));
        }
        if before.1 != state.focused {
            self.ipc.broadcast("focus", json!({ "window": state.focused }));
        }
    }

    fn answer(&mut self, idx: usize, state: &mut State, request: Request) -> Result<()> {
        if let Some(name) = request.query {
            let value = match query::run(&name, &self.conn, state) {
                Ok(data) => ipc::data(data),
                Err(err) => ipc::error(&format!("{err:#}")),
            };
            self.ipc.reply(idx, value);
            return Ok(());
        }

        if let Some(topics) = request.subscribe {
            match ipc::check_topics(&topics) {
                Ok(()) => {
                    self.ipc.set_topics(idx, topics);
                    self.ipc.reply(idx, ipc::ok());
                }
                Err(err) => self.ipc.reply(idx, ipc::error(&format!("{err:#}"))),
            }
            return Ok(());
        }

        let Some(cmd) = request.cmd else {
            self.ipc.reply(idx, ipc::error("a line needs a cmd or a query"));
            return Ok(());
        };
        let spec = match request.arg {
            Some(arg) => format!("{cmd}:{arg}"),
            None => cmd.clone(),
        };
        match Action::parse(&spec) {
            Some(action) => {
                self.run_action(state, action)?;
                self.ipc.reply(idx, ipc::ok());
            }
            None => self.ipc.reply(idx, ipc::error(&format!("cannot read {spec:?} as an action"))),
        }
        Ok(())
    }

    fn run_action(&mut self, state: &mut State, action: Action) -> Result<()> {
        debug!(?action, "action");
        match action {
            Action::Spawn(cmd) => {
                if let Err(err) = std::process::Command::new(cmd).spawn() {
                    warn!(error = %err, "spawn failed");
                }
            }
            Action::Focus(dir) => {
                if let Some(win) = state.tree().neighbor(dir) {
                    state.tree_mut().focus(win);
                    state.focused = Some(win);
                }
            }
            Action::Swap(dir) => {
                if let Some(win) = state.focused
                    && let Some(other) = state.tree().neighbor(dir)
                {
                    state.tree_mut().swap(win, other);
                }
            }
            Action::Workspace(index) => self.switch(state, index)?,
            Action::Send(index) => self.send(state, index)?,
            Action::ToggleFloat => self.toggle_float(state)?,
            Action::ToggleFullscreen => self.toggle_fullscreen(state)?,
            Action::Reload => self.reload(state)?,
            Action::Resize(dir, amount) => {
                state.tree_mut().resize(dir, amount as f32);
            }
            Action::Mode(name) => self.set_mode(state, &name)?,
            Action::Close => {
                if let Some(win) = state.focused {
                    self.close(win)?;
                }
            }
        }
        Ok(())
    }

    fn switch(&self, state: &mut State, to: usize) -> Result<()> {
        if to == state.current {
            return Ok(());
        }
        for win in state.windows(state.current) {
            ignore_gone(self.conn.unmap_window(win)?.check())?;
        }
        state.current = to;

        let focused_is_here = state
            .focused
            .is_some_and(|win| state.clients.get(&win).is_some_and(|c| c.workspace == to));
        if !focused_is_here {
            state.focused = state.next_focus();
        }

        self.apply_layout(state)?;
        for win in state.windows(to) {
            ignore_gone(self.conn.map_window(win)?.check())?;
        }
        self.set_focus(state)?;
        self.set_desktop(to)?;
        Ok(())
    }

    fn send(&self, state: &mut State, to: usize) -> Result<()> {
        let Some(win) = state.focused else {
            return Ok(());
        };
        if to == state.current {
            return Ok(());
        }

        ignore_gone(self.conn.unmap_window(win)?.check())?;

        let screen = state.work();
        let was_tiled = state.tree_mut().remove(win);
        if was_tiled {
            state.workspaces[to].insert(win, screen);
        }
        if let Some(client) = state.clients.get_mut(&win) {
            client.workspace = to;
        }
        self.conn.change_property32(
            PropMode::REPLACE,
            win,
            self.atoms.net_wm_desktop,
            AtomEnum::CARDINAL,
            &[to as u32],
        )?;
        state.focused = state.next_focus();
        Ok(())
    }

    fn toggle_float(&self, state: &mut State) -> Result<()> {
        let Some(win) = state.focused else {
            return Ok(());
        };
        let floating = match state.clients.get_mut(&win) {
            Some(client) => {
                client.floating = !client.floating;
                client.floating
            }
            None => return Ok(()),
        };

        if floating {
            state.tree_mut().remove(win);
        } else {
            let screen = state.work();
            state.tree_mut().insert(win, screen);
        }
        Ok(())
    }

    fn toggle_fullscreen(&self, state: &mut State) -> Result<()> {
        let Some(win) = state.focused else {
            return Ok(());
        };
        let on = !state.clients.get(&win).is_some_and(|c| c.fullscreen);
        set_fullscreen(&self.conn, &self.atoms, state, win, on)
    }

    fn reload(&mut self, state: &mut State) -> Result<()> {
        match Config::load() {
            Ok(config) => {
                let binds = active_binds(&config, &state.mode);
                self.rebind(&config, binds)?;
                state.config = config;
                info!("config reloaded");
            }
            Err(err) => warn!(error = %format!("{err:#}"), "reload failed, keeping the old config"),
        }
        Ok(())
    }

    fn set_mode(&mut self, state: &mut State, name: &str) -> Result<()> {
        let mode = if name == "normal" {
            Mode::Normal
        } else if state.config.modes.contains_key(name) {
            Mode::Named(name.to_string())
        } else {
            warn!(mode = name, "no such mode");
            return Ok(());
        };
        self.rebind(&state.config, active_binds(&state.config, &mode))?;
        state.mode = mode;
        let name = match &state.mode {
            Mode::Normal => "normal",
            Mode::Named(name) => name.as_str(),
        };
        self.ipc.broadcast("mode", json!({ "mode": name }));
        info!(mode = %name, "mode");
        Ok(())
    }

    fn rebind(&mut self, config: &Config, spec: &HashMap<String, Action>) -> Result<()> {
        let binds = keys::grab(&self.conn, self.root, spec, &config.general.mod_)?;
        for (keycode, mods) in self.binds.keys() {
            if !binds.contains_key(&(*keycode, *mods)) {
                self.conn.ungrab_key(*keycode, self.root, *mods)?;
            }
        }
        self.binds = binds;
        Ok(())
    }

    fn set_desktop(&self, index: usize) -> Result<()> {
        self.conn.change_property32(
            PropMode::REPLACE,
            self.root,
            self.atoms.net_current_desktop,
            AtomEnum::CARDINAL,
            &[index as u32],
        )?;
        Ok(())
    }

    fn close(&self, win: Window) -> Result<()> {
        let Ok(protocols) = self
            .conn
            .get_property(false, win, self.atoms.wm_protocols, AtomEnum::ATOM, 0, 32)?
            .reply()
        else {
            return Ok(());
        };

        if protocols
            .value32()
            .is_some_and(|mut atoms| atoms.any(|a| a == self.atoms.wm_delete_window))
        {
            let data =
                ClientMessageData::from([self.atoms.wm_delete_window, CURRENT_TIME, 0, 0, 0]);
            self.conn.send_event(
                false,
                win,
                EventMask::NO_EVENT,
                ClientMessageEvent::new(32, win, self.atoms.wm_protocols, data),
            )?;
        } else {
            self.conn.kill_client(win)?;
        }
        Ok(())
    }

    fn apply_layout(&self, state: &State) -> Result<()> {
        let gap = state.config.general.gap;
        for (win, rect) in apply_layout(state.tree(), state.work(), gap) {
            if state.clients[&win].fullscreen {
                continue;
            }
            place(&self.conn, win, rect)?;
            self.set_border_color(state, win)?;
        }

        for win in state.floating() {
            let cfg = ConfigureWindowAux {
                stack_mode: Some(StackMode::ABOVE),
                ..Default::default()
            };
            ignore_gone(self.conn.configure_window(win, &cfg)?.check())?;
            self.set_border_color(state, win)?;
        }

        for win in state.windows(state.current) {
            if !state.clients[&win].fullscreen {
                continue;
            }
            let border = state.config.general.border;
            let work = state.work();
            let cfg = ConfigureWindowAux {
                x: Some(work.x + border as i32),
                y: Some(work.y + border as i32),
                width: Some((work.w - border as i32 * 2) as u32),
                height: Some((work.h - border as i32 * 2) as u32),
                stack_mode: Some(StackMode::ABOVE),
                ..Default::default()
            };
            ignore_gone(self.conn.configure_window(win, &cfg)?.check())?;
            self.set_border_color(state, win)?;
        }

        if let Some(drag) = &state.drag {
            place(&self.conn, drag.win, drag.rect())?;
        }
        Ok(())
    }

    fn set_border_color(&self, state: &State, win: Window) -> Result<()> {
        let color = if state.focused == Some(win) {
            state.config.colors.focused
        } else {
            state.config.colors.unfocused
        };
        self.conn.change_property32(
            PropMode::REPLACE,
            win,
            self.atoms.net_wm_border_color,
            AtomEnum::CARDINAL,
            &[color],
        )?;
        Ok(())
    }

    fn set_focus(&self, state: &State) -> Result<()> {
        let Some(win) = state.focused else {
            return Ok(());
        };
        match self
            .conn
            .set_input_focus(InputFocus::POINTER_ROOT, win, CURRENT_TIME)?
            .check()
        {
            Ok(()) => Ok(()),
            Err(ReplyError::X11Error(e))
                if matches!(e.error_kind, ErrorKind::Window | ErrorKind::Match) =>
            {
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }
}

fn place(conn: &RustConnection, win: Window, rect: Rect) -> Result<()> {
    let cfg = ConfigureWindowAux {
        x: Some(rect.x),
        y: Some(rect.y),
        width: Some(rect.w as u32),
        height: Some(rect.h as u32),
        ..Default::default()
    };
    ignore_gone(conn.configure_window(win, &cfg)?.check())
}

fn active_binds<'a>(config: &'a Config, mode: &'a Mode) -> &'a HashMap<String, Action> {
    match mode {
        Mode::Named(name) => config.modes.get(name).unwrap_or(&config.binds),
        Mode::Normal => &config.binds,
    }
}

pub fn set_fullscreen(
    conn: &RustConnection,
    atoms: &Atoms,
    state: &mut State,
    win: Window,
    on: bool,
) -> Result<()> {
    let restore = {
        let Some(client) = state.clients.get_mut(&win) else {
            return Ok(());
        };
        if client.fullscreen == on {
            return Ok(());
        }
        if on {
            let Ok(rect) = conn.get_geometry(win)?.reply() else {
                return Ok(());
            };
            client.restore = Some(ewmh::rect(&rect));
        }
        client.fullscreen = on;
        client.restore
    };

    let fullscreen = if on { atoms.net_wm_state_fullscreen } else { 0 };
    conn.change_property32(
        PropMode::REPLACE,
        win,
        atoms.net_wm_state,
        AtomEnum::ATOM,
        &[fullscreen],
    )?;

    if !on && state.clients[&win].floating && let Some(rect) = restore {
        place(conn, win, rect)?;
    }
    Ok(())
}
