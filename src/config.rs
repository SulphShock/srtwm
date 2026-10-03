use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use crate::actions::Action;
use crate::geometry::Dir;

pub struct Config {
    pub general: General,
    pub colors: Colors,
    pub binds: HashMap<String, Action>,
    pub modes: HashMap<String, HashMap<String, Action>>,
    pub autostart: Vec<String>,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct General {
    #[serde(rename = "mod")]
    pub mod_: String,
    pub terminal: String,
    pub gap: i32,
    pub border: u32,
    pub focus_follows_mouse: bool,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct Colors {
    #[serde(deserialize_with = "hex")]
    pub focused: u32,
    #[serde(deserialize_with = "hex")]
    pub unfocused: u32,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    general: General,
    #[serde(default)]
    colors: Colors,
    #[serde(default)]
    binds: HashMap<String, String>,
    #[serde(default, rename = "mode")]
    modes: HashMap<String, HashMap<String, String>>,
    #[serde(default)]
    autostart: Vec<toml::Value>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
        };

        let file: File =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let binds = if file.binds.is_empty() {
            default_binds(&file.general.terminal)
        } else {
            parse_binds(&file.binds, "bind")?
        };

        Ok(Config {
            general: file.general,
            colors: file.colors,
            binds,
            modes: file
                .modes
                .iter()
                .map(|(name, binds)| {
                    Ok((name.clone(), parse_binds(binds, &format!("mode {name}"))?))
                })
                .collect::<Result<_>>()?,
            autostart: file
                .autostart
                .iter()
                .filter_map(|entry| entry.get("cmd").and_then(|c| c.as_str()).map(String::from))
                .collect(),
        })
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            general: General::default(),
            colors: Colors::default(),
            binds: default_binds(&General::default().terminal),
            modes: default_modes(),
            autostart: Vec::new(),
        }
    }
}

impl Default for General {
    fn default() -> Self {
        General {
            mod_: "Super".to_string(),
            terminal: "xterm".to_string(),
            gap: 8,
            border: 2,
            focus_follows_mouse: true,
        }
    }
}

impl Default for Colors {
    fn default() -> Self {
        Colors {
            focused: 0x88c0d0,
            unfocused: 0x3b4252,
        }
    }
}

const HJKL: [(&str, Dir); 4] = [
    ("h", Dir::Left),
    ("j", Dir::Down),
    ("k", Dir::Up),
    ("l", Dir::Right),
];

fn default_binds(terminal: &str) -> HashMap<String, Action> {
    let mut binds = HashMap::from([
        (
            "Mod+Return".to_string(),
            Action::Spawn(terminal.to_string()),
        ),
        ("Mod+r".to_string(), Action::Mode("resize".to_string())),
        ("Mod+Shift+r".to_string(), Action::Reload),
        ("Mod+f".to_string(), Action::ToggleFullscreen),
        ("Mod+q".to_string(), Action::Close),
    ]);
    for (key, dir) in HJKL {
        binds.insert(format!("Mod+{key}"), Action::Focus(dir));
        binds.insert(format!("Mod+Shift+{key}"), Action::Swap(dir));
    }
    for n in 1..=9 {
        binds.insert(format!("Mod+{n}"), Action::Workspace(n - 1));
        binds.insert(format!("Mod+Shift+{n}"), Action::Send(n - 1));
    }
    binds
}

fn default_modes() -> HashMap<String, HashMap<String, Action>> {
    let mut resize = HashMap::new();
    for (key, dir) in HJKL {
        resize.insert(key.to_string(), Action::Resize(dir, 20));
    }
    resize.insert("Escape".to_string(), Action::Mode("normal".to_string()));
    HashMap::from([("resize".to_string(), resize)])
}

fn parse_binds(binds: &HashMap<String, String>, what: &str) -> Result<HashMap<String, Action>> {
    binds
        .iter()
        .map(|(key, text)| {
            let action = Action::parse(text)
                .ok_or_else(|| anyhow!("{what} {key}: cannot read {text:?} as an action"))?;
            Ok((key.clone(), action))
        })
        .collect()
}

fn hex<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let text = String::deserialize(deserializer)?;
    u32::from_str_radix(text.trim_start_matches('#'), 16)
        .map_err(|_| serde::de::Error::custom(format!("{text:?} is not a colour like #88c0d0")))
}

fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    base.join("srtwm").join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn the_ordinary_binds_and_a_modes_binds_take_the_same_path() {
        let ordinary = parse_binds(
            &map(&[("Mod+q", "close"), ("Mod+Return", "spawn:kitty")]),
            "bind",
        )
        .expect("both spellings are actions");
        assert!(matches!(ordinary.get("Mod+q"), Some(Action::Close)));
        assert!(matches!(ordinary.get("Mod+Return"), Some(Action::Spawn(c)) if c == "kitty"));

        let mode = parse_binds(&map(&[("h", "resize:left:20")]), "mode resize").expect("reads");
        assert!(matches!(mode.get("h"), Some(Action::Resize(Dir::Left, 20))));
    }

    #[test]
    fn a_binding_the_parser_cannot_read_names_the_key_it_failed_on() {
        let err = parse_binds(&map(&[("Mod+nope", "teleport:left")]), "bind")
            .expect_err("teleport is not an action")
            .to_string();
        assert!(err.contains("Mod+nope"), "{err}");
        assert!(err.contains("teleport:left"), "{err}");
    }

    #[test]
    fn one_bad_binding_fails_the_whole_file() {
        assert!(parse_binds(&map(&[("Mod+q", "close"), ("Mod+x", "fly")]), "bind").is_err());
    }

    #[test]
    fn a_partial_table_keeps_the_built_in_defaults_for_the_rest() {
        let file: File = toml::from_str(
            r##"
            [general]
            gap = 0
            [colors]
            focused = "#ff0000"
            "##,
        )
        .expect("parses");

        assert_eq!(file.general.gap, 0);
        assert_eq!(file.general.mod_, "Super");
        assert_eq!(file.general.terminal, "xterm");
        assert_eq!(file.general.border, 2);
        assert!(file.general.focus_follows_mouse);
        assert_eq!(file.colors.focused, 0xff0000);
        assert_eq!(file.colors.unfocused, 0x3b4252);
    }

    #[test]
    fn an_empty_table_is_all_defaults() {
        let file: File = toml::from_str("").expect("parses");
        assert_eq!(file.general, General::default());
        assert_eq!(file.colors, Colors::default());
    }

    #[test]
    fn the_default_keys_are_the_ones_the_readme_promises() {
        let binds = default_binds("xterm");
        assert!(matches!(binds.get("Mod+h"), Some(Action::Focus(Dir::Left))));
        assert!(matches!(
            binds.get("Mod+Shift+j"),
            Some(Action::Swap(Dir::Down))
        ));
        assert!(matches!(binds.get("Mod+5"), Some(Action::Workspace(4))));
        let modes = default_modes();
        let resize = modes.get("resize").expect("the resize mode is there");
        assert!(matches!(
            resize.get("l"),
            Some(Action::Resize(Dir::Right, 20))
        ));
        assert!(matches!(resize.get("Escape"), Some(Action::Mode(m)) if m == "normal"));
    }
}
