use crate::geometry::{Axis, Dir, Rect};

pub type Window = u32;

const MIN: f32 = 0.1;

enum Node {
    Leaf {
        win: Window,
        parent: Option<usize>,
    },
    Split {
        axis: Axis,
        ratio: f32,
        a: usize,
        b: usize,
        parent: Option<usize>,
    },
}

pub struct Tree {
    // TODO: nodes are never reused, so a long session leaks an entry per closed window
    nodes: Vec<Node>,
    root: Option<usize>,
    focus: Option<usize>,
}

impl Tree {
    pub fn new() -> Self {
        Tree {
            nodes: Vec::new(),
            root: None,
            focus: None,
        }
    }

    pub fn focused(&self) -> Option<Window> {
        match self.focus.map(|i| &self.nodes[i]) {
            Some(Node::Leaf { win, .. }) => Some(*win),
            _ => None,
        }
    }

    pub fn focus(&mut self, win: Window) -> bool {
        match self.find(win) {
            Some(i) => {
                self.focus = Some(i);
                true
            }
            None => false,
        }
    }

    pub fn insert(&mut self, win: Window, area: Rect) {
        let leaf = self.nodes.len();
        self.nodes.push(Node::Leaf {
            win,
            parent: None,
        });

        let Some(root) = self.root else {
            self.root = Some(leaf);
            self.focus = Some(leaf);
            return;
        };

        let at = self.focus.expect("root implies focus");
        let r = self.rect_of(root, at, area).expect("focus is in the tree");
        let axis = if r.w >= r.h {
            Axis::Horizontal
        } else {
            Axis::Vertical
        };

        let split = self.nodes.len();
        self.nodes.push(Node::Split {
            axis,
            ratio: 0.5,
            a: at,
            b: leaf,
            parent: None,
        });

        let up = self.parent(at);
        if let Some(p) = up {
            self.replace(p, at, split);
        } else {
            self.root = Some(split);
        }
        self.set_parent(split, up);
        self.set_parent(at, Some(split));
        self.set_parent(leaf, Some(split));
        self.focus = Some(leaf);
    }

    pub fn remove(&mut self, win: Window) -> bool {
        let Some(root) = self.root else { return false };
        let Some(leaf) = self.find(win) else {
            return false;
        };

        if leaf == root {
            self.root = None;
            self.focus = None;
            return true;
        }

        let p = self.parent(leaf).unwrap();
        let sib = match &self.nodes[p] {
            Node::Split { a, b, .. } if *a == leaf => *b,
            Node::Split { a, .. } => *a,
            _ => unreachable!("leaf's parent is a split"),
        };

        let gp = self.parent(p);
        if let Some(g) = gp {
            self.replace(g, p, sib);
        } else {
            self.root = Some(sib);
        }
        self.set_parent(sib, gp);
        if self.focus == Some(leaf) {
            self.focus = Some(self.first_leaf(sib));
        }
        true
    }

    pub fn neighbor(&self, dir: Dir) -> Option<Window> {
        let mut idx = self.focus?;
        loop {
            let p = self.parent(idx)?;
            let (axis, a, b) = match &self.nodes[p] {
                Node::Split { axis, a, b, .. } => (*axis, *a, *b),
                _ => return None,
            };
            let first = a == idx;
            let crosses = match (axis, dir) {
                (Axis::Horizontal, Dir::Right) => first,
                (Axis::Horizontal, Dir::Left) => !first,
                (Axis::Vertical, Dir::Down) => first,
                (Axis::Vertical, Dir::Up) => !first,
                _ => false,
            };
            if crosses {
                return self.edge(if first { b } else { a }, dir);
            }
            idx = p;
        }
    }

    // The nearest leaf inside `idx` in `dir`: first for Right/Down, last for Left/Up.
    fn edge(&self, idx: usize, dir: Dir) -> Option<Window> {
        match &self.nodes[idx] {
            Node::Leaf { win, .. } => Some(*win),
            Node::Split { a, b, .. } => match dir {
                Dir::Right | Dir::Down => self.edge(*a, dir),
                Dir::Left | Dir::Up => self.edge(*b, dir),
            },
        }
    }

    pub fn swap(&mut self, x: Window, y: Window) -> bool {
        let (Some(i), Some(j)) = (self.find(x), self.find(y)) else {
            return false;
        };
        if let Node::Leaf { win, .. } = &mut self.nodes[i] {
            *win = y;
        }
        if let Node::Leaf { win, .. } = &mut self.nodes[j] {
            *win = x;
        }
        true
    }

    pub fn resize(&mut self, dir: Dir, amount: f32) -> bool {
        let Some(f) = self.focus else { return false };
        let Some(p) = self.parent(f) else { return false };
        let (axis, first) = match &self.nodes[p] {
            Node::Split { axis, a, .. } => (*axis, *a == f),
            _ => return false,
        };
        if matches!(axis, Axis::Vertical) != matches!(dir, Dir::Up | Dir::Down) {
            return false;
        }
        let delta = if matches!(dir, Dir::Left | Dir::Up) == first {
            -amount
        } else {
            amount
        };
        if let Node::Split { ratio, .. } = &mut self.nodes[p] {
            *ratio = (*ratio + delta).clamp(MIN, 1.0 - MIN);
        }
        true
    }

    pub fn leaves(&self, area: Rect) -> Vec<(Window, Rect)> {
        let mut out = Vec::new();
        if let Some(root) = self.root {
            self.walk(root, area, &mut out);
        }
        out
    }

    fn walk(&self, idx: usize, area: Rect, out: &mut Vec<(Window, Rect)>) {
        match &self.nodes[idx] {
            Node::Leaf { win, .. } => out.push((*win, area)),
            Node::Split {
                axis, ratio, a, b, ..
            } => {
                let (x, y) = area.split(*axis, *ratio);
                self.walk(*a, x, out);
                self.walk(*b, y, out);
            }
        }
    }

    fn find(&self, win: Window) -> Option<usize> {
        self.find_in(self.root?, win)
    }

    fn find_in(&self, idx: usize, win: Window) -> Option<usize> {
        match &self.nodes[idx] {
            Node::Leaf { win: w, .. } if *w == win => Some(idx),
            Node::Leaf { .. } => None,
            Node::Split { a, b, .. } => self.find_in(*a, win).or_else(|| self.find_in(*b, win)),
        }
    }

    fn parent(&self, idx: usize) -> Option<usize> {
        match &self.nodes[idx] {
            Node::Leaf { parent, .. } | Node::Split { parent, .. } => *parent,
        }
    }

    fn set_parent(&mut self, idx: usize, p: Option<usize>) {
        match &mut self.nodes[idx] {
            Node::Leaf { parent, .. } | Node::Split { parent, .. } => *parent = p,
        }
    }

    fn replace(&mut self, p: usize, old: usize, new: usize) {
        if let Node::Split { a, b, .. } = &mut self.nodes[p] {
            if *a == old {
                *a = new;
            } else {
                *b = new;
            }
        }
    }

    fn first_leaf(&self, idx: usize) -> usize {
        match &self.nodes[idx] {
            Node::Leaf { .. } => idx,
            Node::Split { a, .. } => self.first_leaf(*a),
        }
    }

    fn rect_of(&self, idx: usize, target: usize, area: Rect) -> Option<Rect> {
        if idx == target {
            return Some(area);
        }
        let Node::Split {
            axis, ratio, a, b, ..
        } = &self.nodes[idx]
        else {
            return None;
        };
        let (x, y) = area.split(*axis, *ratio);
        self.rect_of(*a, target, x)
            .or_else(|| self.rect_of(*b, target, y))
    }
}

impl Default for Tree {
    fn default() -> Self {
        Tree::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::new(0, 0, 1000, 800)
    }

    fn rect_of(tree: &Tree, win: Window, area: Rect) -> Rect {
        tree.leaves(area)
            .into_iter()
            .find(|(w, _)| *w == win)
            .map(|(_, r)| r)
            .expect("window is in the tree")
    }

    fn cascade() -> Tree {
        let area = area();
        let mut tree = Tree::new();
        for win in 1..=4 {
            tree.insert(win, area);
        }
        tree
    }

    #[test]
    fn one_window_fills_the_area() {
        let area = area();
        let mut tree = Tree::new();
        tree.insert(1, area);
        assert_eq!(tree.leaves(area), vec![(1, area)]);
    }

    #[test]
    fn two_windows_split_the_longer_side() {
        let area = area();
        let mut tree = Tree::new();
        tree.insert(1, area);
        tree.insert(2, area);
        assert_eq!(rect_of(&tree, 1, area), Rect::new(0, 0, 500, 800));
        assert_eq!(rect_of(&tree, 2, area), Rect::new(500, 0, 500, 800));
    }

    #[test]
    fn a_tall_area_splits_top_to_bottom() {
        let area = Rect::new(0, 0, 400, 900);
        let mut tree = Tree::new();
        tree.insert(1, area);
        tree.insert(2, area);
        assert_eq!(rect_of(&tree, 1, area), Rect::new(0, 0, 400, 450));
        assert_eq!(rect_of(&tree, 2, area), Rect::new(0, 450, 400, 450));
    }

    #[test]
    fn four_windows_cascade_from_the_focus() {
        let area = area();
        let tree = cascade();
        assert_eq!(rect_of(&tree, 1, area), Rect::new(0, 0, 500, 800));
        assert_eq!(rect_of(&tree, 2, area), Rect::new(500, 0, 500, 400));
        assert_eq!(rect_of(&tree, 3, area), Rect::new(500, 400, 250, 400));
        assert_eq!(rect_of(&tree, 4, area), Rect::new(750, 400, 250, 400));
    }

    #[test]
    fn six_windows_cover_the_area_exactly() {
        let area = area();
        let mut tree = Tree::new();
        for win in 1..=6 {
            tree.insert(win, area);
        }

        let leaves = tree.leaves(area);
        assert_eq!(leaves.len(), 6);
        assert_eq!(
            leaves.iter().map(|(_, r)| r.w * r.h).sum::<i32>(),
            area.w * area.h
        );
        for i in 0..leaves.len() {
            for j in i + 1..leaves.len() {
                let (a, b) = (leaves[i].1, leaves[j].1);
                let apart =
                    a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
                assert!(apart, "{} and {} overlap", leaves[i].0, leaves[j].0);
            }
        }
    }

    #[test]
    fn insert_focuses_the_new_window() {
        let area = area();
        let mut tree = Tree::new();
        tree.insert(1, area);
        tree.insert(2, area);
        assert_eq!(tree.focused(), Some(2));
    }

    #[test]
    fn remove_middle_promotes_the_sibling() {
        let area = area();
        let mut tree = Tree::new();
        for win in 1..=3 {
            tree.insert(win, area);
        }
        assert!(tree.remove(2));
        assert_eq!(tree.leaves(area).len(), 2);
        assert_eq!(rect_of(&tree, 1, area), Rect::new(0, 0, 500, 800));
        assert_eq!(rect_of(&tree, 3, area), Rect::new(500, 0, 500, 800));
    }

    #[test]
    fn remove_focuses_the_promoted_sibling() {
        let area = area();
        let mut tree = Tree::new();
        for win in 1..=3 {
            tree.insert(win, area);
        }
        tree.remove(3);
        assert_eq!(tree.focused(), Some(2));
    }

    #[test]
    fn remove_the_only_window_empties_the_tree() {
        let area = area();
        let mut tree = Tree::new();
        tree.insert(1, area);
        assert!(tree.remove(1));
        assert!(tree.leaves(area).is_empty());
        assert_eq!(tree.focused(), None);
    }

    #[test]
    fn remove_an_unknown_window_reports_false() {
        let area = area();
        let mut tree = Tree::new();
        tree.insert(1, area);
        assert!(!tree.remove(9));
        assert_eq!(tree.leaves(area), vec![(1, area)]);
    }

    #[test]
    fn neighbor_follows_the_direction() {
        let mut tree = cascade();
        tree.focus(1);
        assert_eq!(tree.neighbor(Dir::Right), Some(2));
        tree.focus(2);
        assert_eq!(tree.neighbor(Dir::Down), Some(3));
    }

    #[test]
    fn neighbor_uses_the_split_closest_to_the_focus() {
        let mut tree = cascade();
        tree.focus(4);
        assert_eq!(tree.neighbor(Dir::Left), Some(3));
        tree.focus(3);
        assert_eq!(tree.neighbor(Dir::Up), Some(2));
    }

    #[test]
    fn neighbor_at_the_edge_is_none() {
        let mut tree = cascade();
        tree.focus(1);
        assert_eq!(tree.neighbor(Dir::Left), None);
        assert_eq!(tree.neighbor(Dir::Up), None);
        tree.focus(2);
        assert_eq!(tree.neighbor(Dir::Up), None);
    }

    #[test]
    fn swap_exchanges_the_windows() {
        let area = area();
        let mut tree = cascade();
        assert!(tree.swap(1, 2));
        assert_eq!(rect_of(&tree, 2, area), Rect::new(0, 0, 500, 800));
        assert_eq!(rect_of(&tree, 1, area), Rect::new(500, 0, 500, 400));
    }

    #[test]
    fn swap_of_an_unknown_window_reports_false() {
        let mut tree = cascade();
        assert!(!tree.swap(1, 9));
    }

    #[test]
    fn resize_moves_the_split() {
        let area = area();
        let mut tree = cascade();
        tree.focus(1);
        assert!(tree.resize(Dir::Right, 0.25));
        assert_eq!(rect_of(&tree, 1, area), Rect::new(0, 0, 750, 800));
        assert_eq!(rect_of(&tree, 2, area), Rect::new(750, 0, 250, 400));
    }

    #[test]
    fn resize_across_the_split_does_nothing() {
        let area = area();
        let mut tree = cascade();
        tree.focus(1);
        assert!(!tree.resize(Dir::Down, 0.25));
        assert_eq!(rect_of(&tree, 1, area), Rect::new(0, 0, 500, 800));
    }

    #[test]
    fn resize_stops_at_the_limit() {
        let area = area();
        let mut tree = cascade();
        tree.focus(1);
        tree.resize(Dir::Right, 5.0);
        assert_eq!(rect_of(&tree, 1, area), Rect::new(0, 0, 900, 800));
    }

    #[test]
    fn focus_moves_to_a_window_in_the_tree() {
        let mut tree = cascade();
        assert!(tree.focus(3));
        assert_eq!(tree.focused(), Some(3));
        assert!(!tree.focus(9));
        assert_eq!(tree.focused(), Some(3));
    }
}
