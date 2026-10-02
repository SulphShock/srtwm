#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }

    pub fn split(&self, axis: Axis, ratio: f32) -> (Rect, Rect) {
        match axis {
            Axis::Horizontal => {
                let w = (self.w as f32 * ratio).round() as i32;
                (
                    Rect::new(self.x, self.y, w, self.h),
                    Rect::new(self.x + w, self.y, self.w - w, self.h),
                )
            }
            Axis::Vertical => {
                let h = (self.h as f32 * ratio).round() as i32;
                (
                    Rect::new(self.x, self.y, self.w, h),
                    Rect::new(self.x, self.y + h, self.w, self.h - h),
                )
            }
        }
    }

    // Clamped, because a crowded tree can inset a pane down to nothing.
    pub fn inset(&self, by: i32) -> Rect {
        Rect::new(
            self.x + by,
            self.y + by,
            (self.w - 2 * by).max(0),
            (self.h - 2 * by).max(0),
        )
    }

    pub fn centered(&self, w: i32, h: i32) -> Rect {
        Rect::new(self.x + (self.w - w) / 2, self.y + (self.h - h) / 2, w, h)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl Dir {
    pub fn parse(name: &str) -> Option<Dir> {
        Some(match name {
            "left" => Dir::Left,
            "right" => Dir::Right,
            "up" => Dir::Up,
            "down" => Dir::Down,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inset_takes_the_gap_off_every_side() {
        assert_eq!(Rect::new(0, 0, 100, 100).inset(8), Rect::new(8, 8, 84, 84));
    }

    #[test]
    fn inset_never_goes_negative() {
        assert_eq!(Rect::new(0, 0, 10, 10).inset(8), Rect::new(8, 8, 0, 0));
    }

    #[test]
    fn centered_splits_the_difference_evenly() {
        assert_eq!(
            Rect::new(0, 0, 1000, 800).centered(400, 200),
            Rect::new(300, 300, 400, 200)
        );
    }

    #[test]
    fn centered_keeps_the_offset_of_the_area_it_sits_in() {
        assert_eq!(
            Rect::new(10, 20, 100, 100).centered(40, 40),
            Rect::new(40, 50, 40, 40)
        );
    }

    #[test]
    fn split_halves_are_adjacent() {
        let (a, b) = Rect::new(0, 0, 100, 40).split(Axis::Horizontal, 0.5);
        assert_eq!(a, Rect::new(0, 0, 50, 40));
        assert_eq!(b, Rect::new(50, 0, 50, 40));
    }

    #[test]
    fn directions_parse_from_their_names() {
        assert_eq!(Dir::parse("left"), Some(Dir::Left));
        assert_eq!(Dir::parse("down"), Some(Dir::Down));
        assert_eq!(Dir::parse("north"), None);
    }
}
