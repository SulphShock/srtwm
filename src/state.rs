use std::collections::HashMap;

use crate::config::Config;
use crate::geometry::Rect;
use crate::layout::bsp::{Tree, Window};
use crate::mouse::Drag;
use crate::window::Client;

pub const WORKSPACES: usize = 9;

pub enum Mode {
    Normal,
    Named(String),
}

pub struct Dock {
    pub win: Window,
    pub insets: [i32; 4],
}

pub struct State {
    pub screen: Rect,
    pub config: Config,
    pub workspaces: [Tree; WORKSPACES],
    pub current: usize,
    pub clients: HashMap<Window, Client>,
    pub drag: Option<Drag>,
    pub focused: Option<Window>,
    pub docks: Vec<Dock>,
    pub mode: Mode,
}

impl State {
    pub fn new(screen: Rect, config: Config) -> Self {
        State {
            screen,
            config,
            workspaces: std::array::from_fn(|_| Tree::new()),
            current: 0,
            clients: HashMap::new(),
            drag: None,
            focused: None,
            docks: Vec::new(),
            mode: Mode::Normal,
        }
    }

    // What is left of the screen once every dock has taken its strip. Two docks
    // on the same edge take the wider one, not the sum of both.
    pub fn work(&self) -> Rect {
        let mut insets = [0i32; 4];
        for dock in &self.docks {
            for (taken, asked) in insets.iter_mut().zip(dock.insets) {
                *taken = (*taken).max(asked);
            }
        }
        let [left, top, right, bottom] = insets;
        Rect::new(
            self.screen.x + left,
            self.screen.y + top,
            (self.screen.w - left - right).max(1),
            (self.screen.h - top - bottom).max(1),
        )
    }

    pub fn tree(&self) -> &Tree {
        &self.workspaces[self.current]
    }

    pub fn tree_mut(&mut self) -> &mut Tree {
        &mut self.workspaces[self.current]
    }

    pub fn windows(&self, ws: usize) -> Vec<Window> {
        let mut out: Vec<Window> = self
            .clients
            .iter()
            .filter(|(_, c)| c.workspace == ws)
            .map(|(win, _)| *win)
            .collect();
        out.sort_unstable();
        out
    }

    pub fn floating(&self) -> Vec<Window> {
        self.windows(self.current)
            .into_iter()
            .filter(|win| self.clients[win].floating)
            .collect()
    }

    // Where the focus lands when the window that had it is gone: the tile the
    // tree points at, or failing that the first float on this workspace.
    pub fn next_focus(&self) -> Option<Window> {
        self.tree()
            .focused()
            .or_else(|| self.floating().first().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        State::new(Rect::new(0, 0, 800, 600), Config::default())
    }

    fn add(state: &mut State, win: Window, ws: usize, floating: bool) {
        state.clients.insert(
            win,
            Client {
                workspace: ws,
                floating,
                fullscreen: false,
                restore: None,
                title: String::new(),
            },
        );
    }

    #[test]
    fn windows_come_back_sorted_so_stacking_never_shuffles() {
        let mut state = state();
        add(&mut state, 30, 0, false);
        add(&mut state, 10, 0, false);
        add(&mut state, 20, 1, false);
        assert_eq!(state.windows(0), vec![10, 30]);
    }

    #[test]
    fn only_floats_on_this_workspace_are_listed() {
        let mut state = state();
        add(&mut state, 1, 0, true);
        add(&mut state, 2, 0, false);
        add(&mut state, 3, 1, true);
        assert_eq!(state.floating(), vec![1]);

        state.current = 1;
        assert_eq!(state.floating(), vec![3]);
    }

    #[test]
    fn focus_falls_to_the_tree_and_then_to_the_first_float() {
        let mut state = state();
        let area = Rect::new(0, 0, 800, 600);
        assert_eq!(state.next_focus(), None);

        add(&mut state, 10, 0, false);
        state.tree_mut().insert(10, area);
        assert_eq!(state.next_focus(), Some(10));

        add(&mut state, 5, 0, true);
        assert_eq!(state.next_focus(), Some(10));
        state.workspaces[0].remove(10);
        assert_eq!(state.next_focus(), Some(5));

        state.clients.remove(&5);
        add(&mut state, 7, 1, true);
        assert_eq!(state.next_focus(), None);
    }
}
