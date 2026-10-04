use anyhow::Result;
use x11rb::protocol::ErrorKind;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt, GetGeometryReply, GetPropertyReply, Window,
};
use x11rb::rust_connection::{ReplyError, RustConnection};

use crate::geometry::Rect;
use crate::state::WORKSPACES;

pub struct Atoms {
    pub net_current_desktop: Atom,
    pub net_number_of_desktops: Atom,
    pub wm_protocols: Atom,
    pub wm_delete_window: Atom,
    pub net_wm_border_color: Atom,
    pub wm_transient_for: Atom,
    pub net_wm_window_type: Atom,
    pub net_wm_window_type_dialog: Atom,
    pub net_wm_window_type_utility: Atom,
    pub net_wm_window_type_splash: Atom,
    pub wm_normal_hints: Atom,
    pub net_wm_state: Atom,
    pub net_wm_state_fullscreen: Atom,
    pub net_wm_name: Atom,
    pub wm_name: Atom,
    pub net_wm_window_type_dock: Atom,
    pub net_wm_strut_partial: Atom,
    pub net_wm_desktop: Atom,
}

impl Atoms {
    pub fn new(conn: &RustConnection) -> Result<Self> {
        Ok(Atoms {
            wm_protocols: intern(conn, b"WM_PROTOCOLS")?,
            wm_delete_window: intern(conn, b"WM_DELETE_WINDOW")?,
            net_wm_border_color: intern(conn, b"_NET_WM_BORDER_COLOR")?,
            net_current_desktop: intern(conn, b"_NET_CURRENT_DESKTOP")?,
            net_number_of_desktops: intern(conn, b"_NET_NUMBER_OF_DESKTOPS")?,
            wm_transient_for: intern(conn, b"WM_TRANSIENT_FOR")?,
            net_wm_window_type: intern(conn, b"_NET_WM_WINDOW_TYPE")?,
            net_wm_window_type_dialog: intern(conn, b"_NET_WM_WINDOW_TYPE_DIALOG")?,
            net_wm_window_type_utility: intern(conn, b"_NET_WM_WINDOW_TYPE_UTILITY")?,
            net_wm_window_type_splash: intern(conn, b"_NET_WM_WINDOW_TYPE_SPLASH")?,
            net_wm_name: intern(conn, b"_NET_WM_NAME")?,
            wm_name: intern(conn, b"WM_NAME")?,
            wm_normal_hints: intern(conn, b"WM_NORMAL_HINTS")?,
            net_wm_state: intern(conn, b"_NET_WM_STATE")?,
            net_wm_state_fullscreen: intern(conn, b"_NET_WM_STATE_FULLSCREEN")?,
            net_wm_window_type_dock: intern(conn, b"_NET_WM_WINDOW_TYPE_DOCK")?,
            net_wm_strut_partial: intern(conn, b"_NET_WM_STRUT_PARTIAL")?,
            net_wm_desktop: intern(conn, b"_NET_WM_DESKTOP")?,
        })
    }
}

fn intern(conn: &RustConnection, name: &[u8]) -> Result<Atom> {
    Ok(conn.intern_atom(false, name)?.reply()?.atom)
}

pub fn rect(g: &GetGeometryReply) -> Rect {
    Rect::new(g.x as i32, g.y as i32, g.width as i32, g.height as i32)
}

fn property<T>(
    conn: &RustConnection,
    win: Window,
    property: Atom,
    kind: AtomEnum,
    len: u32,
    unpack: fn(&GetPropertyReply) -> Vec<T>,
) -> Result<Vec<T>> {
    match conn
        .get_property(false, win, property, kind, 0, len)?
        .reply()
    {
        Ok(reply) => Ok(unpack(&reply)),
        Err(ReplyError::X11Error(e)) if e.error_kind == ErrorKind::Window => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}

fn as_words(reply: &GetPropertyReply) -> Vec<u32> {
    reply.value32().into_iter().flatten().collect()
}

fn as_bytes(reply: &GetPropertyReply) -> Vec<u8> {
    reply.value8().into_iter().flatten().collect()
}

// A dialog, a transient, or anything with a size it will not let go of, is
// asking for a place of its own. Returns where to put it, or None to tile it.
pub fn float_rect(
    conn: &RustConnection,
    atoms: &Atoms,
    win: Window,
    screen: Rect,
) -> Result<Option<Rect>> {
    let transient = !property(
        conn,
        win,
        atoms.wm_transient_for,
        AtomEnum::WINDOW,
        1,
        as_words,
    )?
    .is_empty();

    let types: Vec<Atom> = property(
        conn,
        win,
        atoms.net_wm_window_type,
        AtomEnum::ATOM,
        8,
        as_words,
    )?;
    let asks_for_its_own_place = [
        atoms.net_wm_window_type_dialog,
        atoms.net_wm_window_type_utility,
        atoms.net_wm_window_type_splash,
    ]
    .iter()
    .any(|kind| types.contains(kind));

    let hints = size_hints(conn, atoms, win)?;

    if !transient && !asks_for_its_own_place && !hints.is_some_and(|h| h.fixed()) {
        return Ok(None);
    }

    let (w, h) = match hints {
        Some(h) if h.fixed() => (h.min_w, h.min_h),
        Some(h) if h.base_w > 0 && h.base_h > 0 => (h.base_w, h.base_h),
        _ => (screen.w * 3 / 5, screen.h * 3 / 5),
    };
    Ok(Some(screen.centered(w.min(screen.w), h.min(screen.h))))
}

pub fn is_dock(conn: &RustConnection, atoms: &Atoms, win: Window) -> Result<bool> {
    let types = property(
        conn,
        win,
        atoms.net_wm_window_type,
        AtomEnum::ATOM,
        8,
        as_words,
    )?;
    Ok(types.contains(&atoms.net_wm_window_type_dock))
}

pub fn desktop(conn: &RustConnection, atoms: &Atoms, win: Window) -> Result<Option<usize>> {
    let words = property(
        conn,
        win,
        atoms.net_wm_desktop,
        AtomEnum::CARDINAL,
        1,
        as_words,
    )?;
    match words.first() {
        Some(&index) if index < WORKSPACES as u32 => Ok(Some(index as usize)),
        _ => Ok(None),
    }
}

pub fn dock_insets(conn: &RustConnection, atoms: &Atoms, win: Window) -> Result<[i32; 4]> {
    let words = property(
        conn,
        win,
        atoms.net_wm_strut_partial,
        AtomEnum::CARDINAL,
        12,
        as_words,
    )?;
    let at = |n: usize| words.get(n).copied().unwrap_or(0) as i32;
    Ok([at(0), at(2), at(1), at(3)])
}

pub fn title(conn: &RustConnection, atoms: &Atoms, win: Window) -> Result<String> {
    let modern = property(conn, win, atoms.net_wm_name, AtomEnum::ANY, 256, as_bytes)?;
    if modern.is_empty() {
        let bytes = property(conn, win, atoms.wm_name, AtomEnum::ANY, 256, as_bytes)?;
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    Ok(String::from_utf8_lossy(&modern).into_owned())
}

#[derive(Clone, Copy)]
struct SizeHints {
    min_w: i32,
    min_h: i32,
    max_w: i32,
    max_h: i32,
    base_w: i32,
    base_h: i32,
}

impl SizeHints {
    fn fixed(&self) -> bool {
        self.min_w > 0 && self.min_w == self.max_w && self.min_h == self.max_h
    }
}

fn size_hints(conn: &RustConnection, atoms: &Atoms, win: Window) -> Result<Option<SizeHints>> {
    let words = property(
        conn,
        win,
        atoms.wm_normal_hints,
        AtomEnum::WM_SIZE_HINTS,
        18,
        as_words,
    )?;
    Ok(parse_size_hints(&words))
}

fn parse_size_hints(words: &[u32]) -> Option<SizeHints> {
    let (base_w, base_h) = match words.len() {
        18 => (words[11] as i32, words[12] as i32),
        7 => (0, 0),
        _ => return None,
    };
    Some(SizeHints {
        min_w: words[1] as i32,
        min_h: words[2] as i32,
        max_w: words[3] as i32,
        max_h: words[4] as i32,
        base_w,
        base_h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normal_hints(min: [u32; 2], max: [u32; 2], base: [u32; 2]) -> Vec<u32> {
        let mut words = vec![0; 18];
        words[1..3].copy_from_slice(&min);
        words[3..5].copy_from_slice(&max);
        words[11..13].copy_from_slice(&base);
        words
    }

    #[test]
    fn a_window_that_cannot_grow_is_fixed() {
        let hints = parse_size_hints(&normal_hints([200, 100], [200, 100], [200, 100])).unwrap();
        assert!(hints.fixed());
    }

    #[test]
    fn a_window_with_room_to_grow_is_not_fixed() {
        let hints = parse_size_hints(&normal_hints([200, 100], [800, 600], [640, 480])).unwrap();
        assert!(!hints.fixed());
    }

    #[test]
    fn no_hints_at_all_leaves_the_choice_open() {
        assert!(parse_size_hints(&[]).is_none());
    }

    #[test]
    fn the_seven_word_wm_size_layout_still_reads() {
        let hints = parse_size_hints(&[0, 200, 100, 200, 100, 1, 1]).unwrap();
        assert!(hints.fixed());
    }
}
