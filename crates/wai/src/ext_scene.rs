//! A panel's `scene` node (API 1.10): the pure parts. The tile layer's pixels, the view's scroll,
//! the sprites' tween and the labels' stacking, all clock- and window-free.

use std::cell::Cell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use image::{Frame, Rgba, RgbaImage};
use wai_ext::panel::{Pattern, Scene};

use crate::appearance::fonts;
use crate::ext_panel::{ExtPanel, PanelKey, tone_color};
use gpui_kit::prelude::FluentBuilder as _;

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

/// A map taller than this scrolls instead of growing the panel.
const MAX_HEIGHT: f32 = 560.;
/// One line of a sprite's label or badge.
const LINE: f32 = 14.;

/// The frame gpui draws: BGRA, premultiplied.
fn render_image(image: RgbaImage) -> Arc<RenderImage> {
    let mut image = image;
    for px in image.pixels_mut() {
        let [r, g, b, a] = px.0;
        let m = |c: u8| (u16::from(c) * u16::from(a) / 255) as u8;
        px.0 = [m(b), m(g), m(r), a];
    }
    Arc::new(RenderImage::new(vec![Frame::new(image)]))
}

/// A sprite on its way to a tile.
#[derive(Clone, Copy)]
struct Motion {
    from: Tile,
    to: Tile,
    since: Instant,
}

impl Motion {
    fn near(&self) -> bool {
        (self.to.x - self.from.x).abs() <= 1 && (self.to.y - self.from.y).abs() <= 1
    }

    fn animating(&self, now: Instant) -> bool {
        self.near() && now.saturating_duration_since(self.since) < TWEEN
    }
}

/// What a panel keeps of a scene between renders.
#[derive(Clone, Default)]
pub(crate) struct SceneState {
    /// The hash of what the tile layer and the sprite images are made of.
    hash: u64,
    pub(crate) layer: Option<Arc<RenderImage>>,
    /// Each sprite pattern's image, by name, with its size in px.
    sprites: HashMap<String, (Arc<RenderImage>, f32, f32)>,
    motions: HashMap<String, Motion>,
    /// The scene's drawn size in px, as last painted.
    view: Rc<Cell<(f32, f32)>>,
    /// The top-left tile shown, once the view is measured.
    offset: Rc<Cell<Option<Tile>>>,
}

fn layer_hash(scene: &Scene) -> u64 {
    let mut h = DefaultHasher::new();
    (scene.cell, scene.scale, &scene.palette, &scene.patterns).hash(&mut h);
    (&scene.legend, &scene.map).hash(&mut h);
    h.finish()
}

impl SceneState {
    /// Takes in a new render of the scene: re-rasterises only when the tiles' makings changed,
    /// and starts a tween for each sprite that changed tile.
    pub(crate) fn update(&mut self, scene: &Scene, now: Instant) {
        let hash = layer_hash(scene);
        if self.layer.is_none() || hash != self.hash {
            self.hash = hash;
            self.layer = Some(render_image(rasterise(scene)));
            self.sprites.clear();
        }
        for sprite in &scene.sprites {
            let cached = self.sprites.contains_key(&sprite.pattern);
            if let (false, Some(image)) = (cached, rasterise_pattern(scene, &sprite.pattern)) {
                let (w, h) = image.dimensions();
                let image = (render_image(image), w as f32, h as f32);
                self.sprites.insert(sprite.pattern.clone(), image);
            }
        }
        let motions = &mut self.motions;
        motions.retain(|id, _| scene.sprites.iter().any(|s| s.id == *id));
        for sprite in &scene.sprites {
            let to = Tile {
                x: sprite.x as i32,
                y: sprite.y as i32,
            };
            match motions.get(&sprite.id) {
                Some(m) if m.to == to => {}
                Some(m) => {
                    let from = m.to;
                    motions.insert(
                        sprite.id.clone(),
                        Motion {
                            from,
                            to,
                            since: now,
                        },
                    );
                }
                None => {
                    let since = now.checked_sub(TWEEN).unwrap_or(now);
                    motions.insert(
                        sprite.id.clone(),
                        Motion {
                            from: to,
                            to,
                            since,
                        },
                    );
                }
            }
        }
    }

    pub(crate) fn animating(&self, now: Instant) -> bool {
        self.motions.values().any(|m| m.animating(now))
    }

    fn spot(&self, id: &str, now: Instant) -> Option<Spot> {
        let m = self.motions.get(id)?;
        Some(tween(m.from, m.to, now.saturating_duration_since(m.since)))
    }
}

fn element_id(kind: &str, id: &str) -> ElementId {
    ElementId::Name(format!("{kind}:{id}").into())
}

/// The scene: the cached tile layer, then the sprites with their labels and badges, scrolled to
/// keep the followed sprite in view.
pub(crate) fn draw(
    scene: &Scene,
    state: &SceneState,
    focus: &FocusHandle,
    now: Instant,
    window: &Window,
    cx: &mut Context<ExtPanel>,
) -> AnyElement {
    let tile = (scene.cell * usize::from(scene.scale)) as f32;
    let (cols, rows) = (scene.width as i32, scene.map.len() as i32);
    let (map_w, map_h) = (cols as f32 * tile, rows as f32 * tile);
    let (view_w, view_h) = state.view.get();
    let measured = view_w > 0. && view_h > 0.;
    let view = match measured {
        true => (
            ((view_w / tile) as i32).max(1),
            ((view_h / tile) as i32).max(1),
        ),
        false => (cols, rows),
    };
    let follow = scene
        .follow
        .as_ref()
        .and_then(|f| state.motions.get(f))
        .map(|m| m.to);
    let offset = match follow {
        Some(follow) => {
            let prev = state.offset.get();
            viewport(view, (cols, rows), follow, i32::from(scene.margin), prev)
        }
        None => Tile { x: 0, y: 0 },
    };
    if measured {
        state.offset.set(Some(offset));
    }
    let (ox, oy) = (offset.x as f32 * tile, offset.y as f32 * tile);

    // The panel learns its drawn size here, and draws again when it changed.
    let (measure, entity) = (state.view.clone(), cx.entity_id());
    let probe = canvas(
        |bounds, _, _| bounds.size,
        move |_, size, _, cx| {
            let size = (f32::from(size.width), f32::from(size.height));
            if measure.get() != size {
                measure.set(size);
                cx.notify(entity);
            }
        },
    )
    .absolute()
    .size_full();

    let tiles = state.layer.clone().map(|layer| {
        img(layer)
            .id(element_id("scene-layer", &scene.id))
            .test_support()
            .absolute()
            .left(px(-ox))
            .top(px(-oy))
            .w(px(map_w))
            .h(px(map_h))
    });

    let tiles_of = |s: &wai_ext::panel::Sprite| {
        let home = Tile { x: 0, y: 0 };
        state.motions.get(&s.id).map_or(home, |m| m.to)
    };
    let stack = label_rows(&scene.sprites.iter().map(tiles_of).collect::<Vec<_>>());
    let sprites = scene.sprites.iter().zip(stack).map(|(sprite, row)| {
        let spot = state.spot(&sprite.id, now).unwrap_or(Spot {
            x: sprite.x as f32,
            y: sprite.y as f32,
        });
        let (left, top) = (spot.x * tile - ox, spot.y * tile - oy);
        let body = state.sprites.get(&sprite.pattern).map(|(image, w, h)| {
            img(image.clone())
                .id(element_id("scene-sprite", &sprite.id))
                .test_support()
                .absolute()
                .left(px(left + (tile - w) / 2.))
                .top(px(top + tile - h))
                .w(px(*w))
                .h(px(*h))
        });
        let theme = cx.theme();
        let (background, foreground) = (theme.background.opacity(0.8), theme.foreground);
        let label_size = fonts::ui(cx, 10.);
        let tag = |text: &str, id: &str, color: Hsla| {
            div()
                .id(element_id(id, &sprite.id))
                .test_support()
                .h(px(LINE))
                .px_1()
                .rounded(px(3.))
                .bg(background)
                .text_size(label_size)
                .line_height(px(LINE))
                .text_color(color)
                .whitespace_nowrap()
                .child(text.to_string())
        };
        let lines = usize::from(sprite.label.is_some()) + usize::from(sprite.badge.is_some());
        let tags = (lines > 0).then(|| {
            let slot = 2. * LINE;
            v_flex()
                .absolute()
                .left(px(left))
                .top(px(top - slot * (row + 1) as f32))
                .w(px(tile))
                .h(px(slot))
                .items_center()
                .justify_end()
                .children(
                    sprite
                        .badge
                        .as_ref()
                        .map(|b| tag(b, "scene-badge", tone_color(sprite.tone, cx))),
                )
                .children(
                    sprite
                        .label
                        .as_ref()
                        .map(|l| tag(l, "scene-label", foreground)),
                )
        });
        (body, tags)
    });
    let (bodies, tags): (Vec<_>, Vec<_>) = sprites.unzip();

    let hint = cx.theme().muted_foreground;
    v_flex()
        .gap_1()
        .child(
            div()
                .id(element_id("panel-scene", &scene.id))
                .test_support()
                .key_context("ExtScene")
                .track_focus(focus)
                .on_mouse_down(MouseButton::Left, {
                    let focus = focus.clone();
                    move |_, window, cx| focus.focus(window, cx)
                })
                .on_key_down({
                    let id = scene.id.clone();
                    cx.listener(move |_, event: &KeyDownEvent, _, cx| {
                        // Shortcuts stay with the keybindings.
                        let mods = event.keystroke.modifiers;
                        if mods.platform || mods.control {
                            return;
                        }
                        cx.emit(PanelKey {
                            scene: id.clone(),
                            key: event.keystroke.key.clone(),
                            repeat: event.is_held,
                        });
                        cx.stop_propagation();
                    })
                })
                .relative()
                .w_full()
                .max_w(px(map_w))
                .h(px(map_h.min(MAX_HEIGHT)))
                .flex_none()
                .overflow_hidden()
                .child(probe)
                .children(tiles)
                .children(bodies.into_iter().flatten())
                .children(tags.into_iter().flatten()),
        )
        .when(!focus.is_focused(window), |el| {
            el.child(
                h_flex()
                    .debug_selector(|| "scene-hint".into())
                    .text_xs()
                    .text_color(hint)
                    .child("Click the map to walk"),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{Spot, Tile, label_rows, rasterise, rasterise_pattern, tween, viewport};
    use image::RgbaImage;
    use serde_json::json;
    use std::time::Duration;
    use wai_ext::panel::Scene;
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
