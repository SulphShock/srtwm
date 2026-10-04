pub mod bsp;

use crate::geometry::Rect;
use bsp::Tree;

// The tree splits the whole screen; the gap comes off each tile so no two
// windows touch.
pub fn apply_layout(tree: &Tree, screen: Rect, gap: i32) -> Vec<(bsp::Window, Rect)> {
    tree.leaves(screen)
        .into_iter()
        .map(|(win, rect)| {
            let inset = rect.inset(gap);
            (
                win,
                Rect::new(inset.x, inset.y, inset.w.max(1), inset.h.max(1)),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Rect {
        Rect::new(0, 0, 1000, 800)
    }

    #[test]
    fn every_window_is_inset_by_the_gap() {
        let screen = screen();
        let mut tree = Tree::new();
        for win in 1..=4 {
            tree.insert(win, screen);
        }

        let rects: Vec<Rect> = apply_layout(&tree, screen, 8)
            .into_iter()
            .map(|(_, r)| r)
            .collect();
        assert_eq!(rects[0], Rect::new(8, 8, 484, 784));
        for rect in &rects {
            assert!(rect.x >= 8 && rect.y >= 8);
        }
    }

    #[test]
    fn an_empty_tree_lays_out_to_nothing() {
        assert!(apply_layout(&Tree::new(), screen(), 8).is_empty());
    }

    #[test]
    fn a_crowded_screen_never_asks_x_for_a_zero_size() {
        let screen = Rect::new(0, 0, 1280, 720);
        let mut tree = Tree::new();
        for win in 1..=24 {
            tree.insert(win, screen);
        }
        for (_, rect) in apply_layout(&tree, screen, 10) {
            assert!(rect.w >= 1 && rect.h >= 1, "{rect:?}");
        }
    }
}
