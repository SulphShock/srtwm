use std::collections::HashMap;

use anyhow::Result;
use tracing::warn;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    ButtonIndex, ConnectionExt, EventMask, GetModifierMappingReply, GrabMode, Keycode, Keysym,
    ModMask, Window,
};
use x11rb::rust_connection::RustConnection;
use xkeysym::key;

use crate::actions::Action;
use crate::config::Config;

pub type Binds = HashMap<(Keycode, ModMask), Action>;

pub fn grab(
    conn: &RustConnection,
    root: Window,
    spec: &HashMap<String, Action>,
    main: &str,
) -> Result<Binds> {
    let keycodes = keycodes(conn)?;
    let mut binds = HashMap::new();
    let (numlock, capslock) = locks(conn, &keycodes)?;

    for (key, action) in spec {
        let Some((mod_mask, keysym)) = parse(key, main) else {
            warn!(spec = key, "cannot read this binding, skipping it");
            continue;
        };
        let Some(&keycode) = keycodes.get(&keysym) else {
            warn!(spec = key, "key is not on this layout, skipping it");
            continue;
        };
        for lock in [ModMask::from(0u16), numlock, capslock, numlock | capslock] {
            let mask = mod_mask | lock;
            conn.grab_key(false, root, mask, keycode, GrabMode::ASYNC, GrabMode::ASYNC)?;
            binds.insert((keycode, mask), action.clone());
        }
    }

    Ok(binds)
}

pub fn grab_buttons(conn: &RustConnection, root: Window, config: &Config) -> Result<()> {
    let Some(mod_mask) = mask_of(&config.general.mod_) else {
        return Ok(());
    };
    let keycodes = keycodes(conn)?;
    let (numlock, capslock) = locks(conn, &keycodes)?;

    for lock in [ModMask::from(0u16), numlock, capslock, numlock | capslock] {
        for button in [1u8, 3u8] {
            conn.grab_button(
                true,
                root,
                EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                root,
                x11rb::NONE,
                ButtonIndex::from(button),
                mod_mask | lock,
            )?;
        }
    }

    Ok(())
}

fn locks(conn: &RustConnection, keycodes: &HashMap<Keysym, Keycode>) -> Result<(ModMask, ModMask)> {
    let map = conn.get_modifier_mapping()?.reply()?;
    Ok((
        lock_mask(&map, keycodes, key::Num_Lock),
        lock_mask(&map, keycodes, key::Caps_Lock),
    ))
}

fn lock_mask(
    map: &GetModifierMappingReply,
    keycodes: &HashMap<Keysym, Keycode>,
    sym: Keysym,
) -> ModMask {
    let Some(wanted) = keycodes.get(&sym).copied() else {
        return ModMask::from(0u16);
    };
    let per = map.keycodes_per_modifier() as usize;

    let mut mask = ModMask::from(0u16);
    for group in 0..8 {
        if map.keycodes[group * per..(group + 1) * per].contains(&wanted) {
            mask |= ModMask::from(1u16 << group);
        }
    }
    mask
}

fn parse(spec: &str, main: &str) -> Option<(ModMask, Keysym)> {
    let mut parts = spec.split('+');
    let name = parts.next_back()?;

    let mut mask = ModMask::from(0u16);
    for modifier in parts {
        mask |= if modifier == "Mod" {
            mask_of(main)?
        } else {
            mask_of(modifier)?
        };
    }
    Some((mask, keysym(name)?))
}

pub fn mask_of(name: &str) -> Option<ModMask> {
    Some(match name {
        "Super" => ModMask::M4,
        "Alt" => ModMask::M1,
        "Ctrl" => ModMask::CONTROL,
        "Shift" => ModMask::SHIFT,
        _ => return None,
    })
}

fn keysym(name: &str) -> Option<Keysym> {
    if name.chars().count() == 1 {
        return Some(xkeysym::Keysym::from_char(name.chars().next()?).raw());
    }
    Some(match name {
        "Return" => key::Return,
        "Escape" => key::Escape,
        "Tab" => key::Tab,
        "BackSpace" => key::BackSpace,
        "space" => key::space,
        "Left" => key::Left,
        "Right" => key::Right,
        "Up" => key::Up,
        "Down" => key::Down,
        _ => return None,
    })
}

fn keycodes(conn: &RustConnection) -> Result<HashMap<Keysym, Keycode>> {
    let setup = conn.setup();
    let count = setup.max_keycode - setup.min_keycode + 1;
    let map = conn
        .get_keyboard_mapping(setup.min_keycode, count)?
        .reply()?;

    let mut keycodes = HashMap::new();
    for (i, syms) in map
        .keysyms
        .chunks(map.keysyms_per_keycode as usize)
        .enumerate()
    {
        for sym in syms {
            if *sym != key::NoSymbol {
                keycodes
                    .entry(*sym)
                    .or_insert(setup.min_keycode + i as Keycode);
            }
        }
    }
    Ok(keycodes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_is_whatever_the_config_calls_it() {
        let (mask, keysym) = parse("Mod+Shift+h", "Super").unwrap();
        assert_eq!(mask, ModMask::M4 | ModMask::SHIFT);
        assert_eq!(keysym, 0x0068);
    }

    #[test]
    fn named_keys_and_unknown_ones() {
        assert_eq!(parse("Mod+Return", "Super").unwrap().1, key::Return);
        assert!(parse("Mod+Hyper", "Super").is_none());
        assert!(parse("Mod+Nope", "Super").is_none());
    }
}
