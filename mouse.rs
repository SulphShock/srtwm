use crate::geometry::Rect;
use crate::layout::bsp::Window;

#[derive(Clone, Copy, Debug)]
pub enum DragKind {
    Move,
    Resize,
}

pub struct Drag {
    pub win: Window,
    kind: DragKind,
    from: (i16, i16),
    at: (i16, i16),
    rect: Rect,
}

impl Drag {
    pub fn start(win: Window, kind: DragKind, at: (i16, i16), rect: Rect) -> Self {
        Drag {
            win,
            kind,
            from: at,
            at,
            rect,
        }
    }

    pub fn track(&mut self, pointer: (i16, i16)) {
        self.at = pointer;
    }

    pub fn rect(&self) -> Rect {
        let (dx, dy) = (self.at.0 - self.from.0, self.at.1 - self.from.1);
        match self.kind {
            DragKind::Move => Rect::new(
                self.rect.x + dx as i32,
                self.rect.y + dy as i32,
                self.rect.w,
                self.rect.h,
            ),
            DragKind::Resize => Rect::new(
                self.rect.x,
                self.rect.y,
                (self.rect.w + dx as i32).max(1),
                (self.rect.h + dy as i32).max(1),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drag(kind: DragKind) -> Drag {
        Drag::start(1, kind, (100, 100), Rect::new(40, 60, 200, 150))
    }

    #[test]
    fn a_move_follows_the_pointer_and_keeps_the_size() {
        let mut drag = drag(DragKind::Move);
        drag.track((160, 90));
        assert_eq!(drag.rect(), Rect::new(100, 50, 200, 150));
    }

    #[test]
    fn moving_back_returns_the_window_where_it_was() {
        let mut drag = drag(DragKind::Move);
        drag.track((400, 400));
        drag.track((100, 100));
        assert_eq!(drag.rect(), Rect::new(40, 60, 200, 150));
    }

    #[test]
    fn a_resize_grows_the_window_from_its_corner() {
        let mut drag = drag(DragKind::Resize);
        drag.track((150, 130));
        assert_eq!(drag.rect(), Rect::new(40, 60, 250, 180));
    }

    #[test]
    fn a_resize_past_its_own_size_never_reaches_zero() {
        let mut drag = drag(DragKind::Resize);
        drag.track((-500, -500));
        assert_eq!(drag.rect(), Rect::new(40, 60, 1, 1));
    }
}
