//! A panel's `scene` node (API 1.10): the pure parts. The tile layer's pixels, the view's scroll,
//! the sprites' tween and the labels' stacking, all clock- and window-free.
#![allow(dead_code)]

use std::time::Duration;

use image::{Rgba, RgbaImage};
use wai_ext::panel::{Pattern, Scene};

/// A sprite that changed tile takes this long to arrive.
pub(crate) const TWEEN: Duration = Duration::from_millis(150);

/// A tile position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tile {
    pub x: i32,
    pub y: i32,
}

/// A position between tiles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Spot {
    pub x: f32,
    pub y: f32,
}

/// A pattern as plain RGBA, each of its pixels `scale` by `scale` on the image; a space (or a
/// key the palette lacks) is transparent.
fn paint(scene: &Scene, pattern: &Pattern) -> RgbaImage {
    let scale = u32::from(scene.scale);
    let (w, h) = (pattern.width as u32, pattern.rows.len() as u32);
    RgbaImage::from_fn(w * scale, h * scale, |x, y| {
        let key = pattern.rows[(y / scale) as usize][(x / scale) as usize];
        match scene.palette.get(&key) {
            Some([r, g, b]) => Rgba([*r, *g, *b, 255]),
            None => Rgba([0; 4]),
        }
    })
}

/// A sprite's pattern, or nothing for a name the scene lacks.
pub(crate) fn rasterise_pattern(scene: &Scene, name: &str) -> Option<RgbaImage> {
    scene.patterns.get(name).map(|p| paint(scene, p))
}

/// The whole map's tile layer: every legend tile at its place, a character the legend lacks left
/// transparent.
pub(crate) fn rasterise(scene: &Scene) -> RgbaImage {
    let tile = (scene.cell * usize::from(scene.scale)) as u32;
    let mut layer = RgbaImage::new(scene.width as u32 * tile, scene.map.len() as u32 * tile);
    for (row, line) in scene.map.iter().enumerate() {
        for (col, ch) in line.iter().enumerate() {
            let pattern = scene.legend.get(ch).and_then(|n| scene.patterns.get(n));
            if let Some(pattern) = pattern {
                let (x, y) = (col as i64 * i64::from(tile), row as i64 * i64::from(tile));
                image::imageops::replace(&mut layer, &paint(scene, pattern), x, y);
            }
        }
    }
    layer
}

/// One axis of `viewport`.
fn scroll(view: i32, map: i32, follow: i32, margin: i32, prev: Option<i32>) -> i32 {
    if map <= view {
        return 0;
    }
    // A margin wider than half the view would pull both ways.
    let margin = margin.min((view - 1) / 2);
    let from = match prev {
        None => follow - view / 2,
        Some(prev) if follow < prev + margin => follow - margin,
        Some(prev) if follow > prev + view - 1 - margin => follow - (view - 1 - margin),
        Some(prev) => prev,
    };
    from.clamp(0, map - view)
}

/// The top-left tile shown: the least scroll from `prev` that keeps `follow` `margin` tiles from
/// every edge, or the map's centre on `follow` when there is no `prev`. All in tiles; a map that
/// fits the view does not scroll.
pub(crate) fn viewport(
    view: (i32, i32),
    map: (i32, i32),
    follow: Tile,
    margin: i32,
    prev: Option<Tile>,
) -> Tile {
    Tile {
        x: scroll(view.0, map.0, follow.x, margin, prev.map(|p| p.x)),
        y: scroll(view.1, map.1, follow.y, margin, prev.map(|p| p.y)),
    }
}

/// Where a sprite is `elapsed` after it left `from` for `to`: linear over `TWEEN`. A move of more
/// than one tile (a teleport or a snap back) is not tweened.
pub(crate) fn tween(from: Tile, to: Tile, elapsed: Duration) -> Spot {
    let near = (to.x - from.x).abs() <= 1 && (to.y - from.y).abs() <= 1;
    let t = match near {
        true => (elapsed.as_secs_f32() / TWEEN.as_secs_f32()).min(1.),
        false => 1.,
    };
    let at = |a: i32, b: i32| a as f32 + (b - a) as f32 * t;
    Spot {
        x: at(from.x, to.x),
        y: at(from.y, to.y),
    }
}

/// For each sprite, how many sprites before it share its tile: the row its label stacks on.
pub(crate) fn label_rows(tiles: &[Tile]) -> Vec<usize> {
    (0..tiles.len())
        .map(|i| tiles[..i].iter().filter(|t| **t == tiles[i]).count())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wai_ext::panel::{Node, parse};

    fn scene(extra: serde_json::Value) -> Box<Scene> {
        let mut v = json!({
            "type": "scene", "id": "s", "cell": 4, "scale": 2,
            "palette": {"a": "#f00", "b": "#0000ff"},
            "patterns": {
                "floor": ["aaaa", "a  a", "a  a", "bbbb"],
                "dot": ["a"],
            },
            "legend": {".": "floor"},
            "map": ["..", ".?"],
            "sprites": [],
        });
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().cloned().unwrap_or_default());
        let Ok(Node::Scene(s)) = parse(&v) else {
            panic!("{v}")
        };
        s
    }

    fn at(layer: &RgbaImage, x: u32, y: u32) -> [u8; 4] {
        layer.get_pixel(x, y).0
    }

    #[test]
    fn rasterise_paints_palette_and_transparency() {
        let layer = rasterise(&scene(json!({})));
        // 2 tiles of 4 px, each pixel doubled.
        assert_eq!(layer.dimensions(), (16, 16));
        assert_eq!(at(&layer, 0, 0), [255, 0, 0, 255]);
        assert_eq!(at(&layer, 1, 1), [255, 0, 0, 255]);
        assert_eq!(at(&layer, 0, 6), [0, 0, 255, 255]);
        // A space is transparent.
        assert_eq!(at(&layer, 3, 3)[3], 0);
        assert_eq!(at(&layer, 4, 3)[3], 0);
        // The second tile starts at x = 8.
        assert_eq!(at(&layer, 8, 0), [255, 0, 0, 255]);
        // `?` is not in the legend: drawn empty.
        assert_eq!(at(&layer, 8, 8)[3], 0);
        assert_eq!(at(&layer, 12, 12)[3], 0);
        // A sprite pattern rasterises on its own size.
        let dot = rasterise_pattern(&scene(json!({})), "dot").unwrap();
        assert_eq!(dot.dimensions(), (2, 2));
        assert_eq!(at(&dot, 1, 1), [255, 0, 0, 255]);
        assert!(rasterise_pattern(&scene(json!({})), "nope").is_none());
    }

    fn t(x: i32, y: i32) -> Tile {
        Tile { x, y }
    }

    #[test]
    fn viewport_keeps_margin() {
        let view = (10, 10);
        let map = (40, 30);
        // Inside the margin: no scroll.
        assert_eq!(viewport(view, map, t(5, 5), 3, Some(t(2, 2))), t(2, 2));
        // 3 tiles from the right edge is allowed (x = 2 + 9 - 3 = 8); past it scrolls by the least.
        assert_eq!(viewport(view, map, t(8, 5), 3, Some(t(2, 2))), t(2, 2));
        assert_eq!(viewport(view, map, t(9, 5), 3, Some(t(2, 2))), t(3, 2));
        // And the left and top edges.
        assert_eq!(viewport(view, map, t(4, 5), 3, Some(t(2, 2))), t(1, 2));
        assert_eq!(viewport(view, map, t(5, 1), 3, Some(t(2, 2))), t(2, 0));
        // Never past the map.
        assert_eq!(
            viewport(view, map, t(39, 29), 3, Some(t(30, 20))),
            t(30, 20)
        );
        assert_eq!(viewport(view, map, t(0, 0), 3, Some(t(5, 5))), t(0, 0));
        // A map that fits does not scroll.
        assert_eq!(
            viewport((50, 50), map, t(39, 29), 3, Some(t(4, 4))),
            t(0, 0)
        );
    }

    #[test]
    fn viewport_centres_first_time() {
        let (view, map) = ((10, 10), (40, 30));
        assert_eq!(viewport(view, map, t(20, 15), 3, None), t(15, 10));
        // Clamped at the corners.
        assert_eq!(viewport(view, map, t(1, 1), 3, None), t(0, 0));
        assert_eq!(viewport(view, map, t(39, 29), 3, None), t(30, 20));
    }

    #[test]
    fn tween_takes_150ms() {
        let ms = Duration::from_millis;
        let (a, b) = (t(2, 3), t(3, 3));
        assert_eq!(tween(a, b, ms(0)), Spot { x: 2., y: 3. });
        assert_eq!(tween(a, b, ms(75)), Spot { x: 2.5, y: 3. });
        assert_eq!(tween(a, b, ms(150)), Spot { x: 3., y: 3. });
        assert_eq!(tween(a, b, ms(400)), Spot { x: 3., y: 3. });
    }

    #[test]
    fn tween_skips_jumps() {
        let spot = tween(t(2, 3), t(9, 3), Duration::ZERO);
        assert_eq!(spot, Spot { x: 9., y: 3. });
        let spot = tween(t(2, 3), t(2, 5), Duration::ZERO);
        assert_eq!(spot, Spot { x: 2., y: 5. });
    }

    #[test]
    fn shared_tile_labels_stack() {
        let tiles = [t(1, 1), t(2, 2), t(1, 1), t(1, 1)];
        assert_eq!(label_rows(&tiles), [0, 0, 1, 2]);
        assert_eq!(label_rows(&[]), Vec::<usize>::new());
    }
}
