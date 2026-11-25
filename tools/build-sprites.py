"""Rebuild sprites/poses from the generated pose sheets in docs/art.

Each docs/art/pose-<name>.jpg is one row of four frames on a flat #00FF00 background.
Dev-only: needs Python 3 with Pillow and numpy.

    python tools/build-sprites.py            # rebuild frames, GIFs and the fallback sheet
    python tools/build-sprites.py --contact  # also write sprites-contact.png for review
"""
import os
import sys
from collections import deque

import numpy as np
from PIL import Image

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ART = os.path.join(REPO, "docs", "art")
OUT = os.path.join(REPO, "sprites", "poses")
POSES = ["idle", "think", "read", "write", "terminal", "search", "done", "error", "delegate", "web", "stop", "sleep"]
SHEET_POSES = POSES[:8]


def source_for(pose):
    return os.path.join(ART, "pose-%s.jpg" % pose)


TARGET_H = 440
PAD_TOP = 12
PAD_BOTTOM = 8
PAD_SIDE = 10


def key_sheet(path, pose):
    img = np.asarray(Image.open(path).convert("RGB")).astype(np.float32)
    h, w, _ = img.shape
    r, g, b = img[..., 0], img[..., 1], img[..., 2]
    gdiff = g - np.maximum(r, b)
    border = np.concatenate([img[0], img[-1], img[:, 0], img[:, -1]])
    bg = np.median(border, axis=0)

    cand = gdiff > 70
    seen = np.zeros((h, w), bool)
    q = deque()
    for x in range(w):
        for y in (0, h - 1):
            if cand[y, x] and not seen[y, x]:
                seen[y, x] = True; q.append((y, x))
    for y in range(h):
        for x in (0, w - 1):
            if cand[y, x] and not seen[y, x]:
                seen[y, x] = True; q.append((y, x))
    while q:
        y, x = q.popleft()
        for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            ny, nx = y + dy, x + dx
            if 0 <= ny < h and 0 <= nx < w and cand[ny, nx] and not seen[ny, nx]:
                seen[ny, nx] = True; q.append((ny, nx))
    outside = seen

    # enclosed green blobs (magnifier lens) become tinted glass; small ones (terminal text) stay
    inside = cand & ~outside
    glass = np.zeros((h, w), bool)
    lab = np.zeros((h, w), np.int32)
    n = 0
    comps = []
    for y0, x0 in zip(*np.nonzero(inside)):
        if lab[y0, x0]:
            continue
        n += 1
        comp = [(y0, x0)]
        lab[y0, x0] = n
        i = 0
        while i < len(comp):
            y, x = comp[i]; i += 1
            for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                ny, nx = y + dy, x + dx
                if 0 <= ny < h and 0 <= nx < w and inside[ny, nx] and not lab[ny, nx]:
                    lab[ny, nx] = n; comp.append((ny, nx))
        comps.append(comp)
    lenses = []
    if pose == "search":
        for comp in comps:
            if len(comp) > 600:
                ys, xs = zip(*comp)
                lenses.append((min(ys) - 6, max(ys) + 6, min(xs) - 6, max(xs) + 6))
    for comp in comps:
        ys, xs = zip(*comp)
        cy, cx = sum(ys) / len(ys), sum(xs) / len(xs)
        if any(t <= cy <= b and l <= cx <= r for t, b, l, r in lenses):
            glass[list(ys), list(xs)] = True
        elif pose != "terminal":
            outside[list(ys), list(xs)] = True

    # soft alpha near the outside region, unmixing the green screen
    near = outside.copy()
    for _ in range(3):
        grown = near.copy()
        grown[1:] |= near[:-1]; grown[:-1] |= near[1:]
        grown[:, 1:] |= near[:, :-1]; grown[:, :-1] |= near[:, 1:]
        near = grown
    edge = near & ~outside
    t0, t1 = 15.0, 150.0
    alpha = np.ones((h, w), np.float32)
    alpha[outside] = 0.0
    a_edge = np.clip((t1 - gdiff) / (t1 - t0), 0.0, 1.0)
    alpha[edge] = a_edge[edge]

    rgb = img.copy()
    safe = np.maximum(alpha, 1e-3)[..., None]
    unmixed = (img - (1.0 - alpha[..., None]) * bg[None, None, :]) / safe
    unmixed = np.clip(unmixed, 0, 255)
    rgb[edge] = unmixed[edge]
    # despill: no channel-green halo on edges
    spill = edge & (rgb[..., 1] > np.maximum(rgb[..., 0], rgb[..., 2]))
    rgb[..., 1][spill] = np.maximum(rgb[..., 0], rgb[..., 2])[spill]

    if glass.any():
        rim = glass.copy()
        for _ in range(3):
            grown = rim.copy()
            grown[1:] |= rim[:-1]; grown[:-1] |= rim[1:]
            grown[:, 1:] |= rim[:, :-1]; grown[:, :-1] |= rim[:, 1:]
            rim = grown
        rim &= ~glass
        tint = rim & (rgb[..., 1] > np.maximum(rgb[..., 0], rgb[..., 2]))
        rgb[..., 1][tint] = np.maximum(rgb[..., 0], rgb[..., 2])[tint]
        rgb[glass] = (205, 232, 250)
        alpha[glass] = 0.55
    a8 = np.clip(alpha * 255.0 + 0.5, 0, 255).astype(np.uint8)
    out = np.dstack([np.clip(rgb, 0, 255).astype(np.uint8), a8])
    return Image.fromarray(out, "RGBA")


def split_frames(sheet, expected=4):
    a = np.asarray(sheet)[..., 3]
    cols = (a > 40).sum(axis=0)
    on = cols > 0
    segs = []
    x = 0
    w = len(on)
    while x < w:
        if on[x]:
            s = x
            while x < w and on[x]:
                x += 1
            segs.append([s, x])
        else:
            x += 1
    # merge tiny islands (sweat drops etc.) into the closest large segment
    while len(segs) > expected:
        widths = [e - s for s, e in segs]
        i = int(np.argmin(widths))
        if i == 0:
            j = 1
        elif i == len(segs) - 1:
            j = i - 1
        else:
            j = i - 1 if segs[i][0] - segs[i - 1][1] < segs[i + 1][0] - segs[i][1] else i + 1
        lo, hi = min(i, j), max(i, j)
        segs[lo] = [min(segs[lo][0], segs[hi][0]), max(segs[lo][1], segs[hi][1])]
        del segs[hi]
    if len(segs) != expected:
        raise SystemExit("expected %d frames, got %d" % (expected, len(segs)))
    frames = []
    for s, e in segs:
        crop = sheet.crop((s, 0, e, sheet.height))
        bbox = crop.getchannel("A").point(lambda v: 255 if v > 40 else 0).getbbox()
        frames.append(crop.crop(bbox))
    return frames


def leg_center(frame):
    a = np.asarray(frame)[..., 3] > 40
    h = a.shape[0]
    rows = a[int(h * 0.72):]
    ys, xs = np.nonzero(rows)
    return float(xs.mean()) if len(xs) else frame.width / 2.0


def main():
    prepared = {}
    for pose in POSES:
        sheet = key_sheet(source_for(pose), pose)
        frames = split_frames(sheet)
        heights = sorted(f.height for f in frames)
        scale = TARGET_H / heights[len(heights) // 2]
        scaled = []
        for f in frames:
            nf = f.resize((max(1, round(f.width * scale)), max(1, round(f.height * scale))), Image.LANCZOS)
            scaled.append((nf, leg_center(nf)))
        prepared[pose] = scaled
    half = 0
    tallest = 0
    for pose, frames in prepared.items():
        for f, cx in frames:
            half = max(half, cx, f.width - cx)
            tallest = max(tallest, f.height)
    cw = int(np.ceil(half * 2)) + PAD_SIDE * 2
    ch = tallest + PAD_TOP + PAD_BOTTOM
    print("canvas", cw, ch)
    for name in os.listdir(OUT):
        if name.endswith(".png") or name.endswith(".gif"):
            os.remove(os.path.join(OUT, name))
    sheet = Image.new("RGBA", (cw * len(SHEET_POSES), ch), (0, 0, 0, 0))
    for pi, pose in enumerate(POSES):
        canvases = []
        for i, (f, cx) in enumerate(prepared[pose]):
            c = Image.new("RGBA", (cw, ch), (0, 0, 0, 0))
            x = int(round(cw / 2 - cx))
            y = ch - PAD_BOTTOM - f.height
            c.alpha_composite(f, (x, y))
            c.save(os.path.join(OUT, "%s-%d.png" % (pose, i)), optimize=True)
            canvases.append(c)
        if pose in SHEET_POSES:
            sheet.alpha_composite(canvases[0], (pi * cw, 0))
        delays = [160] * len(canvases)
        if pose == "sleep":
            delays = [520, 620, 720, 520]
        if pose == "idle":
            delays = [420, 90, 360, 420]
        if pose == "terminal":
            delays = [240 if i % 2 == 0 else 120 for i in range(len(canvases))]
        gif_frames = []
        for c in canvases:
            bgc = Image.new("RGBA", c.size, (255, 250, 244, 255))
            bgc.alpha_composite(c)
            gif_frames.append(bgc.convert("RGB").resize((cw // 2, ch // 2), Image.LANCZOS))
        gif_frames[0].save(os.path.join(OUT, pose + ".gif"), save_all=True, append_images=gif_frames[1:],
                           duration=delays, loop=0, optimize=True)
    sheet.save(os.path.join(REPO, "sprites", "diandian.png"), optimize=True)



def write_contact(path):
    w, h = Image.open(os.path.join(OUT, "idle-0.png")).size
    fw, fh = w // 2, h // 2
    rows = (len(POSES) + 1) // 2
    sheet = Image.new("RGBA", (fw * 8, fh * rows), (0, 0, 0, 0))
    for r, pose in enumerate(POSES):
        for i in range(4):
            frame = Image.open(os.path.join(OUT, "%s-%d.png" % (pose, i))).resize((fw, fh), Image.LANCZOS)
            bg = Image.new("RGBA", frame.size, (40, 40, 48, 255) if i % 2 == 0 else (255, 250, 244, 255))
            bg.alpha_composite(frame)
            sheet.paste(bg, (i * fw + (r % 2) * 4 * fw, (r // 2) * fh))
    sheet.save(path)
    print(path)


if __name__ == "__main__":
    main()
    if "--contact" in sys.argv[1:]:
        write_contact(os.path.join(REPO, "sprites-contact.png"))
