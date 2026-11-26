use std::path::{Path, PathBuf};
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

// Only the current pose is kept, already scaled to on-screen size: the source frames are
// 277x461 (~0.5 MB each decoded), the scaled ones ~0.08 MB at 100% DPI.
pub struct Clip {
    pub pose: String,
    pub frames: Vec<Pixmap>,
    pub delays_ms: Vec<u32>,
}

pub fn find_pose_dir() -> Option<PathBuf> {
    let mut starts = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        starts.push(exe);
    }
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    for start in starts {
        for dir in start.ancestors() {
            let candidate = dir.join("sprites").join("poses");
            if candidate.join("idle-0.png").exists() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn load(dir: &Path, pose: &str, target_h: u32) -> Option<Clip> {
    let mut frames = Vec::new();
    for i in 0..12 {
        let path = dir.join(format!("{pose}-{i}.png"));
        if !path.exists() {
            break;
        }
        let source = Pixmap::load_png(&path).ok()?;
        frames.push(scale_to_height(&source, target_h)?);
    }
    if frames.is_empty() {
        return None;
    }
    let delays_ms = (0..frames.len())
        .map(|i| match pose {
            "idle" => [420, 90, 360, 420][i % 4],
            "sleep" => [520, 620, 720, 520][i % 4],
            "terminal" => if i % 2 == 0 { 240 } else { 120 },
            _ => 160,
        })
        .collect();
    Some(Clip { pose: pose.to_string(), frames, delays_ms })
}

fn scale_to_height(source: &Pixmap, target_h: u32) -> Option<Pixmap> {
    let s = target_h as f32 / source.height() as f32;
    let target_w = ((source.width() as f32) * s).round().max(1.0) as u32;
    if s >= 1.0 {
        let mut out = Pixmap::new(target_w, target_h)?;
        let paint = PixmapPaint { quality: FilterQuality::Bicubic, ..PixmapPaint::default() };
        out.draw_pixmap(0, 0, source.as_ref(), &paint, Transform::from_scale(s, s), None);
        return Some(out);
    }
    Some(area_downscale(source, target_w, target_h))
}

// Bicubic sampling aliases when shrinking ~2.5x; averaging the covered source area does not.
// Premultiplied input, so a plain average is correct.
pub fn area_downscale(source: &Pixmap, tw: u32, th: u32) -> Pixmap {
    let (sw, sh) = (source.width() as usize, source.height() as usize);
    let src = source.data();
    let mut out = Pixmap::new(tw, th).expect("non-empty target");
    let dst = out.data_mut();
    let fx = sw as f32 / tw as f32;
    let fy = sh as f32 / th as f32;
    for ty in 0..th as usize {
        let y0 = ty as f32 * fy;
        let y1 = y0 + fy;
        for tx in 0..tw as usize {
            let x0 = tx as f32 * fx;
            let x1 = x0 + fx;
            let mut acc = [0f32; 4];
            let mut total = 0f32;
            let mut sy = y0.floor() as usize;
            while (sy as f32) < y1 && sy < sh {
                let wy = (y1.min(sy as f32 + 1.0) - y0.max(sy as f32)).max(0.0);
                let mut sx = x0.floor() as usize;
                while (sx as f32) < x1 && sx < sw {
                    let w = wy * (x1.min(sx as f32 + 1.0) - x0.max(sx as f32)).max(0.0);
                    let at = (sy * sw + sx) * 4;
                    for c in 0..4 {
                        acc[c] += src[at + c] as f32 * w;
                    }
                    total += w;
                    sx += 1;
                }
                sy += 1;
            }
            let at = (ty * tw as usize + tx) * 4;
            for c in 0..4 {
                dst[at + c] = if total > 0.0 { (acc[c] / total).round().min(255.0) as u8 } else { 0 };
            }
        }
    }
    out
}
