use crate::quota::QuotaRow;
use crate::state::View;
use crate::text::{ellipsize, wrap, Fonts};
use tiny_skia::{Color, FillRule, LineCap, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform};

type Rgba = [u8; 4];

pub struct Palette {
    bubble_fill: Rgba,
    bubble_border: Rgba,
    title: Rgba,
    body: Rgba,
    muted: Rgba,
    faint: Rgba,
    divider: Rgba,
    close_fill: Rgba,
    close_mark: Rgba,
    warn: Rgba,
    track: Rgba,
    shadow: Rgba,
}

pub const LIGHT: Palette = Palette {
    bubble_fill: [255, 250, 244, 248],
    bubble_border: [232, 214, 190, 255],
    title: [44, 36, 22, 255],
    body: [63, 52, 40, 255],
    muted: [122, 110, 96, 255],
    faint: [156, 146, 132, 255],
    divider: [236, 226, 210, 255],
    close_fill: [244, 232, 216, 255],
    close_mark: [120, 96, 72, 255],
    warn: [196, 112, 20, 255],
    track: [238, 228, 212, 255],
    shadow: [120, 78, 36, 40],
};

pub const DARK: Palette = Palette {
    bubble_fill: [36, 37, 43, 244],
    bubble_border: [74, 76, 86, 255],
    title: [242, 238, 230, 255],
    body: [220, 214, 204, 255],
    muted: [168, 162, 152, 255],
    faint: [128, 124, 118, 255],
    divider: [60, 62, 70, 255],
    close_fill: [56, 58, 66, 255],
    close_mark: [200, 196, 188, 255],
    warn: [240, 166, 70, 255],
    track: [62, 64, 74, 255],
    shadow: [0, 0, 0, 70],
};

fn accent_color(accent: &str, phase: &str) -> Rgba {
    if phase == "idle" {
        return [176, 164, 146, 255];
    }
    match accent {
        "green" => [34, 176, 98, 255],
        "orange" => [224, 138, 30, 255],
        "red" => [220, 64, 64, 255],
        _ => [47, 111, 235, 255],
    }
}

fn quota_color(pct: f64) -> Rgba {
    if pct >= 90.0 {
        [220, 64, 64, 255]
    } else if pct >= 70.0 {
        [224, 138, 30, 255]
    } else {
        [47, 111, 235, 255]
    }
}

fn mark_color(mark: &str) -> Rgba {
    match mark {
        "done" | "wait" => [34, 176, 98, 255],
        "bad" => [220, 64, 64, 255],
        "warn" => [224, 138, 30, 255],
        _ => [47, 111, 235, 255],
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Area {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

/// Board lines look like `mark|key|text`.
pub fn is_board(history: &str) -> bool {
    ["run|", "wait|", "done|", "bad|", "warn|"].iter().any(|m| history.contains(m))
}

struct BoardRow<'a> {
    mark: &'a str,
    key: &'a str,
    body: &'a str,
}

fn board_rows(history: &str) -> Vec<BoardRow<'_>> {
    history
        .split('\n')
        .filter(|l| !l.is_empty())
        .take(8)
        .map(|line| match line.split_once('|') {
            Some((mark, rest)) if !mark.is_empty() => match rest.split_once('|') {
                Some((key, body)) => BoardRow { mark, key, body },
                None => BoardRow { mark, key: "", body: rest },
            },
            _ => BoardRow { mark: "run", key: "", body: line },
        })
        .collect()
}

/// Where everything goes for one view, in window pixels.
pub struct Spot {
    pub width: u32,
    pub height: u32,
    pub bubble: Area,
    pub close: Area,
    pub headline: Area,
    pub character: Area,
    pub rows: Vec<(String, Area)>,
    pub quota: Option<Area>,
    quota_rows: Vec<QuotaRow>,
    scale: f32,
    text_x: f32,
    text_w: f32,
    meta_y: Option<f32>,
    activity_y: Option<f32>,
    detail: Vec<String>,
    detail_y: f32,
    history_y: Option<f32>,
    history_lines: Vec<String>,
    hint_y: Option<f32>,
}

pub fn layout(view: &View, expanded: bool, quota_rows: &[QuotaRow], fonts: &Fonts, scale: f32, sprite_aspect: f32) -> Spot {
    let px = |v: f32| v * scale;
    let margin = px(12.0);
    let bubble_w = px(292.0);
    let text_x = margin + px(14.0);
    let text_w = bubble_w - px(14.0) - px(30.0);

    let detail = if view.detail.is_empty() { Vec::new() } else { wrap(&fonts.regular, &view.detail, px(12.0), text_w, 2) };
    let detail_line = fonts.regular.line_height(px(12.0)).min(px(18.0));
    let detail_h = detail.len() as f32 * detail_line;

    let board = is_board(&view.history);
    let show_history = board || (expanded && !view.history.is_empty() && view.phase == "running");
    let history_lines: Vec<String> = if show_history && !board {
        view.history.split('\n').filter(|l| !l.is_empty()).map(|l| ellipsize(&fonts.regular, l, px(11.0), text_w)).collect()
    } else {
        Vec::new()
    };
    let history_h = if !show_history {
        0.0
    } else if board {
        board_rows(&view.history).len() as f32 * px(18.0) + px(8.0)
    } else {
        (history_lines.len() as f32 * px(16.0) + px(8.0)).min(px(92.0))
    };

    let mut text_h = px(22.0);
    if !view.meta.is_empty() {
        text_h += px(2.0) + px(16.0);
    }
    if !view.activity.is_empty() {
        text_h += px(3.0) + px(20.0);
    }
    if detail_h > 0.0 {
        text_h += px(2.0) + detail_h;
    }
    if history_h > 0.0 {
        text_h += px(8.0) + history_h;
    }
    if !view.hint.is_empty() {
        text_h += px(4.0) + px(16.0);
    }
    let quota_h = if quota_rows.is_empty() { 0.0 } else { px(18.0) + (quota_rows.len() - 1) as f32 * px(17.0) };
    if quota_h > 0.0 {
        text_h += px(11.0) + quota_h;
    }

    let bubble_h = px(108.0).max(text_h + px(24.0));
    let sprite_h = px(188.0);
    let sprite_w = sprite_h * sprite_aspect;
    let content_h = bubble_h.max(sprite_h);
    let width = (margin + bubble_w + px(4.0) + sprite_w + margin).ceil() as u32;
    let height = (margin + content_h + margin + px(6.0)).ceil() as u32;
    let bubble_y = margin + (content_h - bubble_h) / 2.0;
    let bubble = Area { x: margin, y: bubble_y, w: bubble_w, h: bubble_h };

    let mut cursor = bubble_y + px(12.0);
    let headline = Area { x: text_x, y: cursor, w: text_w, h: px(22.0) };
    cursor += px(22.0) + px(2.0);
    let meta_y = (!view.meta.is_empty()).then(|| {
        let y = cursor;
        cursor += px(16.0) + px(3.0);
        y
    });
    let activity_y = (!view.activity.is_empty()).then(|| {
        let y = cursor;
        cursor += px(20.0) + px(2.0);
        y
    });
    let detail_y = cursor;
    cursor += detail_h;
    let mut rows = Vec::new();
    let history_y = (history_h > 0.0).then(|| {
        let y = cursor + px(8.0);
        if board {
            for (i, row) in board_rows(&view.history).iter().enumerate() {
                if !row.key.is_empty() {
                    rows.push((row.key.to_string(), Area { x: text_x, y: y + i as f32 * px(18.0), w: text_w, h: px(18.0) }));
                }
            }
        }
        cursor += history_h;
        y
    });
    let hint_y = (!view.hint.is_empty()).then(|| cursor + px(4.0));

    let cx = margin + bubble_w + px(4.0) + sprite_w / 2.0;
    let cy = margin + content_h / 2.0;
    Spot {
        width,
        height,
        bubble,
        close: Area { x: bubble.x + bubble.w - px(24.0), y: bubble.y + px(8.0), w: px(16.0), h: px(16.0) },
        headline,
        character: Area { x: cx - sprite_w / 2.0, y: cy - sprite_h / 2.0, w: sprite_w, h: sprite_h },
        rows,
        quota: (quota_h > 0.0).then(|| Area { x: text_x, y: bubble.y + bubble.h - px(10.0) - quota_h, w: text_w, h: quota_h }),
        quota_rows: quota_rows.to_vec(),
        scale,
        text_x,
        text_w,
        meta_y,
        activity_y,
        detail,
        detail_y,
        history_y,
        history_lines,
        hint_y,
    }
}

pub fn draw(pm: &mut Pixmap, spot: &Spot, view: &View, frame: Option<&Pixmap>, fonts: &Fonts, p: &Palette, bob: f32) {
    pm.fill(Color::TRANSPARENT);
    let px = |v: f32| v * spot.scale;
    let b = spot.bubble;

    if let Some(path) = rounded(b.x, b.y, b.w, b.h, px(18.0)) {
        pm.fill_path(&path, &paint(p.bubble_fill), FillRule::Winding, Transform::identity(), None);
        let stroke = Stroke { width: px(1.4).max(1.0), ..Stroke::default() };
        pm.stroke_path(&path, &paint(p.bubble_border), &stroke, Transform::identity(), None);
    }

    let accent = accent_color(&view.accent, &view.phase);
    let dot = px(8.0);
    let h = spot.headline;
    if let Some(circle) = PathBuilder::from_circle(h.x + dot / 2.0, h.y + h.h / 2.0, dot / 2.0) {
        pm.fill_path(&circle, &paint(accent), FillRule::Winding, Transform::identity(), None);
    }
    let head = ellipsize(&fonts.bold, &view.headline, px(16.0), h.w - dot - px(6.0));
    fonts.bold.draw(pm, &head, h.x + dot + px(6.0), h.y, px(16.0), p.title);

    if let Some(y) = spot.meta_y {
        let meta = ellipsize(&fonts.regular, &view.meta, px(11.0), spot.text_w);
        fonts.regular.draw(pm, &meta, spot.text_x, y, px(11.0), p.faint);
    }
    if let Some(y) = spot.activity_y {
        let activity = ellipsize(&fonts.bold, &view.activity, px(13.0), spot.text_w);
        fonts.bold.draw(pm, &activity, spot.text_x, y, px(13.0), p.body);
    }
    let line_h = fonts.regular.line_height(px(12.0)).min(px(18.0));
    for (i, line) in spot.detail.iter().enumerate() {
        fonts.regular.draw(pm, line, spot.text_x, spot.detail_y + i as f32 * line_h, px(12.0), p.muted);
    }

    if let Some(y) = spot.history_y {
        let divider_y = y - px(5.0);
        let mut pb = PathBuilder::new();
        pb.move_to(spot.text_x, divider_y);
        pb.line_to(spot.text_x + spot.text_w, divider_y);
        if let Some(line) = pb.finish() {
            let stroke = Stroke { width: px(1.0).max(1.0), ..Stroke::default() };
            pm.stroke_path(&line, &paint(p.divider), &stroke, Transform::identity(), None);
        }
        if is_board(&view.history) {
            let row_h = px(18.0);
            let dot = px(7.0);
            for (i, row) in board_rows(&view.history).iter().enumerate() {
                let ry = y + i as f32 * row_h;
                if let Some(c) = PathBuilder::from_circle(spot.text_x + dot / 2.0, ry + row_h / 2.0, dot / 2.0) {
                    pm.fill_path(&c, &paint(mark_color(row.mark)), FillRule::Winding, Transform::identity(), None);
                }
                let w = (spot.text_w - dot - px(6.0)).max(1.0);
                let body = ellipsize(&fonts.regular, row.body, px(11.0), w);
                fonts.regular.draw(pm, &body, spot.text_x + dot + px(6.0), ry + px(1.0), px(11.0), p.body);
            }
        } else {
            for (i, line) in spot.history_lines.iter().enumerate() {
                fonts.regular.draw(pm, line, spot.text_x, y + i as f32 * px(16.0), px(11.0), p.muted);
            }
        }
    }

    if let Some(y) = spot.hint_y {
        let hint = ellipsize(&fonts.regular, &view.hint, px(11.0), spot.text_w);
        let color = if view.hint_alert { p.warn } else { p.faint };
        fonts.regular.draw(pm, &hint, spot.text_x, y, px(11.0), color);
    }
    if let Some(area) = spot.quota {
        draw_quota(pm, spot, area, fonts, p);
    }

    let c = spot.close;
    if let Some(circle) = PathBuilder::from_circle(c.x + c.w / 2.0, c.y + c.h / 2.0, c.w / 2.0) {
        pm.fill_path(&circle, &paint(p.close_fill), FillRule::Winding, Transform::identity(), None);
    }
    let pad = c.w * 0.32;
    let mut pb = PathBuilder::new();
    pb.move_to(c.x + pad, c.y + pad);
    pb.line_to(c.x + c.w - pad, c.y + c.h - pad);
    pb.move_to(c.x + c.w - pad, c.y + pad);
    pb.line_to(c.x + pad, c.y + c.h - pad);
    if let Some(cross) = pb.finish() {
        let stroke = Stroke { width: px(1.4).max(1.2), line_cap: LineCap::Round, ..Stroke::default() };
        pm.stroke_path(&cross, &paint(p.close_mark), &stroke, Transform::identity(), None);
    }

    if let Some(frame) = frame {
        let ch = spot.character;
        let (sw, sh) = (frame.width() as f32, frame.height() as f32);
        let cx = ch.x + ch.w / 2.0;
        let cy = ch.y + ch.h / 2.0 + bob;
        if let Some(shadow) = oval(cx - sw * 0.22, cy + sh * 0.36, sw * 0.44, sh * 0.07) {
            pm.fill_path(&shadow, &paint(p.shadow), FillRule::Winding, Transform::identity(), None);
        }
        let x = (cx - sw / 2.0).round() as i32;
        let y = (cy - sh / 2.0).round() as i32;
        pm.draw_pixmap(x, y, frame.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    }
}

fn draw_quota(pm: &mut Pixmap, spot: &Spot, area: Area, fonts: &Fonts, p: &Palette) {
    let px = |v: f32| v * spot.scale;
    let em = px(11.0);
    let face = &fonts.regular;
    let mut pb = PathBuilder::new();
    pb.move_to(area.x, area.y - px(5.0));
    pb.line_to(area.x + area.w, area.y - px(5.0));
    if let Some(line) = pb.finish() {
        let stroke = Stroke { width: px(1.0).max(1.0), ..Stroke::default() };
        pm.stroke_path(&line, &paint(p.divider), &stroke, Transform::identity(), None);
    }
    let right_edge = area.x + area.w;
    let mut y = area.y;
    for (i, row) in spot.quota_rows.iter().enumerate() {
        let row_h = if i == 0 { px(18.0) } else { px(17.0) };
        let text_top = y + (row_h - face.line_height(em)) / 2.0;
        let mut x = area.x;
        if !row.label.is_empty() {
            let lw = px(if crate::lang::english() { 46.0 } else { 36.0 });
            let label = ellipsize(face, &row.label, em, lw);
            face.draw(pm, &label, x, text_top, em, p.muted);
            x += lw;
        }
        if let Some(pct) = row.bar {
            let bar_w = if i == 0 { px(60.0) } else { px(118.0) };
            let bar_h = px(6.0);
            let by = y + (row_h - bar_h) / 2.0;
            if let Some(track) = rounded(x, by, bar_w, bar_h, bar_h / 2.0) {
                pm.fill_path(&track, &paint(p.track), FillRule::Winding, Transform::identity(), None);
            }
            let clamped = pct.clamp(0.0, 100.0);
            if clamped > 0.0 {
                let w = (bar_w * clamped as f32 / 100.0).max(bar_h);
                if let Some(fill) = rounded(x, by, w, bar_h, bar_h / 2.0) {
                    pm.fill_path(&fill, &paint(quota_color(pct)), FillRule::Winding, Transform::identity(), None);
                }
            }
            x += bar_w + px(6.0);
        }
        if !row.text.is_empty() {
            let color = if row.warn { p.warn } else if i == 0 || row.bar.is_some() { p.body } else { p.muted };
            let room = (right_edge - x).max(1.0);
            let width = if row.right.is_empty() { room } else { room.min(face.measure(&row.text, em)) };
            let text = ellipsize(face, &row.text, em, width.max(1.0));
            face.draw(pm, &text, x, text_top, em, color);
            x += width + px(8.0);
        }
        if !row.right.is_empty() && right_edge - x > px(12.0) {
            let right = ellipsize(face, &row.right, em, right_edge - x);
            let w = face.measure(&right, em);
            face.draw(pm, &right, right_edge - w, text_top, em, p.faint);
        }
        y += row_h;
    }
}

fn paint(rgba: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    p.anti_alias = true;
    p
}

fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

fn oval(x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
    PathBuilder::from_oval(Rect::from_xywh(x, y, w, h)?)
}
