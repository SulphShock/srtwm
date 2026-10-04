use crate::geometry::Rect;

pub struct Client {
    pub workspace: usize,
    pub floating: bool,
    pub fullscreen: bool,
    pub restore: Option<Rect>,
    pub title: String,
}
