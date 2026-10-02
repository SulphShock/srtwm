use crate::geometry::Dir;

#[derive(Clone, Debug)]
pub enum Action {
    Spawn(String),
    Focus(Dir),
    Swap(Dir),
    Workspace(usize),
    Send(usize),
    Close,
    ToggleFloat,
    ToggleFullscreen,
    Reload,
    Resize(Dir, i32),
    Mode(String),
}

impl Action {
    pub fn parse(text: &str) -> Option<Action> {
        let (name, arg) = match text.split_once(':') {
            Some((name, arg)) => (name, Some(arg)),
            None => (text, None),
        };
        match name {
            "spawn" => Some(Action::Spawn(arg?.to_string())),
            "focus" => Some(Action::Focus(Dir::parse(arg?)?)),
            "swap" => Some(Action::Swap(Dir::parse(arg?)?)),
            "workspace" => Some(Action::Workspace(arg?.parse().ok()?)),
            "send" => Some(Action::Send(arg?.parse().ok()?)),
            "close" => Some(Action::Close),
            "toggle-float" => Some(Action::ToggleFloat),
            "toggle-fullscreen" => Some(Action::ToggleFullscreen),
            "reload" => Some(Action::Reload),
            // resize takes two colons, so it splits its own arg again
            "resize" => {
                let (dir, amount) = arg?.split_once(':')?;
                Some(Action::Resize(Dir::parse(dir)?, amount.parse().ok()?))
            }
            "mode" => Some(Action::Mode(arg?.to_string())),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_bindings_the_config_uses() {
        assert!(matches!(Action::parse("close"), Some(Action::Close)));
        assert!(matches!(
            Action::parse("spawn:kitty"),
            Some(Action::Spawn(cmd)) if cmd == "kitty"
        ));
        assert!(matches!(
            Action::parse("focus:left"),
            Some(Action::Focus(Dir::Left))
        ));
        assert!(matches!(
            Action::parse("swap:up"),
            Some(Action::Swap(Dir::Up))
        ));
        assert!(matches!(
            Action::parse("workspace:2"),
            Some(Action::Workspace(2))
        ));
        assert!(matches!(Action::parse("send:2"), Some(Action::Send(2))));
        assert!(matches!(
            Action::parse("toggle-float"),
            Some(Action::ToggleFloat)
        ));
        assert!(matches!(
            Action::parse("toggle-fullscreen"),
            Some(Action::ToggleFullscreen)
        ));
        assert!(matches!(Action::parse("reload"), Some(Action::Reload)));
        assert!(matches!(
            Action::parse("resize:left:20"),
            Some(Action::Resize(Dir::Left, 20))
        ));
        assert!(matches!(
            Action::parse("mode:normal"),
            Some(Action::Mode(name)) if name == "normal"
        ));
    }

    #[test]
    fn rejects_nonsense() {
        assert!(Action::parse("focus:sideways").is_none());
        assert!(Action::parse("focus").is_none());
        assert!(Action::parse("teleport:left").is_none());
        assert!(Action::parse("workspace:many").is_none());
        assert!(Action::parse("resize:left").is_none());
    }
}
