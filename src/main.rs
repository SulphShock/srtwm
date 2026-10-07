use anyhow::Result;
use tracing_subscriber::EnvFilter;
use x11rb::connection::Connection;

use srtwm::config::Config;
use srtwm::geometry::Rect;
use srtwm::state::State;
use srtwm::wm::Wm;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let (conn, screen) = x11rb::connect(None)?;
    let (root, area) = {
        let screen = &conn.setup().roots[screen];
        (
            screen.root,
            Rect::new(
                0,
                0,
                screen.width_in_pixels as i32,
                screen.height_in_pixels as i32,
            ),
        )
    };

    let config = Config::load()?;
    let mut wm = Wm::new(conn, root, &config)?;

    for cmd in &config.autostart {
        let _ = std::process::Command::new("sh").arg("-c").arg(cmd).spawn();
    }

    wm.run(&mut State::new(area, config))
}
