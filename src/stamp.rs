//! Unlimited pictures of vintage cards.
//!
//! Every free scan of the Wizards-era sets (Base Set to Neo Destiny) is of a 1st Edition card, so the Unlimited printing
//! has no picture of its own. The two printings differ only by the black "1st Edition" seal on the left of the card,
//! just under the illustration. This finds that seal and paints it out with the card's own background, copied from the
//! strip of card right above it, so the texture and colour carry on. If the seal can't be found with confidence the
//! picture is left alone and `false` comes back.
use image::{Rgba, RgbaImage};

/// A pixel dark and grey enough to be the seal's black ink.
fn ink(p: &Rgba<u8>) -> bool {
    let [r, g, b, _] = p.0;
    let (hi, lo) = (r.max(g).max(b), r.min(g).min(b));
    hi < 110 && hi - lo < 50
}

/// Where the seal can be, as fractions of the card: on a Pokémon, the strip left of the illustration just above the
/// flavour text; on a Trainer, the bottom left of the text box.
pub const ZONES: [(f32, f32, f32, f32); 2] = [(0.015, 0.125, 0.5, 0.64), (0.12, 0.30, 0.84, 0.95)];
/// Where the disc's centre falls in each zone, top to bottom, as fractions of the card's height.
const BANDS: [(f32, f32); 2] = [(0.525, 0.59), (0.87, 0.935)];

pub type Box = (u32, u32, u32, u32);

/// The dark shapes in one zone (4-connected, so the seal's letters stay apart from its disc): each box and pixel count.
pub fn shapes(img: &RgbaImage, (fx0, fx1, fy0, fy1): (f32, f32, f32, f32)) -> Vec<(Box, u32)> {
    let (w, h) = img.dimensions();
    shapes_in(img, ((w as f32 * fx0) as u32, (h as f32 * fy0) as u32, (w as f32 * fx1) as u32, (h as f32 * fy1) as u32))
}

/// The dark shapes inside one rectangle of the picture (x0, y0, x1, y1 with exclusive ends).
fn shapes_in(img: &RgbaImage, (zx0, zy0, zx1, zy1): Box) -> Vec<(Box, u32)> {
    let (zw, zh) = ((zx1 - zx0) as usize, (zy1 - zy0) as usize);
    let mut seen = vec![false; zw * zh];
    let mut out = vec![];
    for sy in 0..zh {
        for sx in 0..zw {
            if seen[sy * zw + sx] || !ink(img.get_pixel(zx0 + sx as u32, zy0 + sy as u32)) {
                continue;
            }
            let (mut b, mut n, mut stack) = ((sx, sy, sx, sy), 0u32, vec![(sx, sy)]);
            seen[sy * zw + sx] = true;
            while let Some((x, y)) = stack.pop() {
                n += 1;
                b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= zw as i32 || ny >= zh as i32 {
                        continue;
                    }
                    let (nx, ny) = (nx as usize, ny as usize);
                    if !seen[ny * zw + nx] && ink(img.get_pixel(zx0 + nx as u32, zy0 + ny as u32)) {
                        seen[ny * zw + nx] = true;
                        stack.push((nx, ny));
                    }
                }
            }
            out.push(((zx0 + b.0 as u32, zy0 + b.1 as u32, zx0 + b.2 as u32, zy0 + b.3 as u32), n));
        }
    }
    out
}

/// The seal's box, inclusive, if there is one.
pub fn find(img: &RgbaImage) -> Option<Box> {
    find_parts(img).map(|p| p.0)
}

/// The seal's box and its disc's box. The seal is found by its most telling part, the solid black disc with the "1"
/// in it; the arched EDITION letters always sit in the same place around that disc.
fn find_parts(img: &RgbaImage) -> Option<(Box, Box)> {
    let (w, h) = img.dimensions();
    if w < 120 || h < 160 {
        return None;
    }
    let (wf, hf) = (w as f32, h as f32);
    for (zone, (lo, hi)) in ZONES.into_iter().zip(BANDS) {
        let disc = shapes(img, zone).into_iter().filter(|&((x0, y0, x1, y1), n)| {
            let (bw, bh) = ((x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32);
            let fill = n as f32 / (bw * bh);
            let cy = (y0 + y1) as f32 / 2.0 / hf;
            (0.026..=0.058).contains(&(bw / wf)) && (0.018..=0.044).contains(&(bh / hf)) && (0.6..=1.6).contains(&(bw / bh)) && (0.38..=0.75).contains(&fill) && (lo..=hi).contains(&cy)
        }).max_by_key(|s| s.1);
        let Some((dbox, _)) = disc else { continue };
        let (x0, _, x1, y1) = dbox;
        // The disc's bottom edge is clean (the letters arch over its top), so the seal is measured from there,
        // at the seal's standard size: about 3.5% of the card across for the disc.
        let d = wf * 0.035;
        let cx = (x0 + x1) as f32 / 2.0;
        let bx0 = (cx - d * 1.2).max(0.0) as u32;
        let bx1 = ((cx + d * 1.2) as u32).min(w - 1);
        let by0 = (y1 as f32 - d * 1.75).max(0.0) as u32;
        let by1 = ((y1 as f32 + d * 0.15) as u32).min(h - 1);
        // The disc itself, round and solid, at its true size under the letters.
        let disc = ((cx - d * 0.55).max(0.0) as u32, (y1 as f32 - d * 1.05).max(0.0) as u32, ((cx + d * 0.55) as u32).min(w - 1), (y1 + 1).min(h - 1));
        return Some(((bx0, by0, bx1, by1), disc));
    }
    None
}

/// Paints out the seal. True when it was found and removed.
///
/// Only the seal's own ink is painted, plus the "1" inside its disc and a hair around the strokes. Each of those pixels
/// takes the colour of the plain card around it, filled in from the edges inward, with the card's grain added back,
/// so frames, borders and symbols next to the seal are never touched or copied.
pub fn erase(img: &mut RgbaImage) -> bool {
    let Some(((sx0, sy0, sx1, sy1), (dx0, dy0, dx1, dy1))) = find_parts(img) else { return false };
    let (w, h) = img.dimensions();
    // A little room round the seal's outline, so a letter at its edge still counts as the seal's.
    let m = (w as f32 * 0.009) as u32 + 1;
    let (ex0, ey0, ex1, ey1) = (sx0.saturating_sub(m), sy0.saturating_sub(m), (sx1 + m).min(w - 2), (sy1 + m).min(h - 2));
    let ww = w as usize;
    let mut mask = vec![false; ww * h as usize];
    // The seal's strokes: shapes wholly inside its outline (a frame or border line running in from outside isn't one).
    let mut strokes = 0usize;
    for ((a, b, c, d), _) in shapes_in(img, (ex0, ey0, ex1 + 1, ey1 + 1)) {
        if a > ex0 && b > ey0 && c < ex1 && d < ey1 {
            for y in b..=d {
                for x in a..=c {
                    if ink(img.get_pixel(x, y)) {
                        mask[y as usize * ww + x as usize] = true;
                        strokes += 1;
                    }
                }
            }
        }
    }
    if strokes == 0 {
        return false;
    }
    // The whole disc, "1" and all.
    let (cx, cy) = ((dx0 + dx1) as f32 / 2.0, (dy0 + dy1) as f32 / 2.0);
    let (rx, ry) = ((dx1 - dx0) as f32 / 2.0 + 1.5, (dy1 - dy0) as f32 / 2.0 + 1.5);
    for y in dy0.saturating_sub(2)..=(dy1 + 2).min(h - 1) {
        for x in dx0.saturating_sub(2)..=(dx1 + 2).min(w - 1) {
            let (nx, ny) = ((x as f32 - cx) / rx, (y as f32 - cy) / ry);
            if nx * nx + ny * ny <= 1.0 {
                mask[y as usize * ww + x as usize] = true;
            }
        }
    }
    // A hair around every stroke, so no dark fringe is left from the print's soft edges.
    let grow = (w / 300).max(2) as i64;
    let core = mask.clone();
    for y in ey0..=ey1 {
        for x in ex0..=ex1 {
            if !core[y as usize * ww + x as usize] {
                continue;
            }
            for oy in -grow..=grow {
                for ox in -grow..=grow {
                    let (nx, ny) = (x as i64 + ox, y as i64 + oy);
                    if nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64 {
                        mask[ny as usize * ww + nx as usize] = true;
                    }
                }
            }
        }
    }
    // The card's grain, from the plain pixels just around the seal.
    let (mut sum, mut sq, mut n) = ([0f64; 3], [0f64; 3], 0f64);
    for y in ey0.saturating_sub(3)..=(ey1 + 3).min(h - 1) {
        for x in ex0.saturating_sub(3)..=(ex1 + 3).min(w - 1) {
            let p = img.get_pixel(x, y);
            if !mask[y as usize * ww + x as usize] && !ink(p) {
                for c in 0..3 {
                    sum[c] += p[c] as f64;
                    sq[c] += (p[c] as f64).powi(2);
                }
                n += 1.0;
            }
        }
    }
    let grain = if n > 0.0 { ((0..3).map(|c| sq[c] / n - (sum[c] / n).powi(2)).sum::<f64>() / 3.0).max(0.0).sqrt().min(12.0) } else { 0.0 };
    // Fill from the edges inward: each pass, every masked pixel with enough plain neighbours takes their average.
    let g = grow as u32 + 3;
    let mut left: Vec<(u32, u32)> = (ey0.saturating_sub(g)..=(ey1 + g).min(h - 1))
        .flat_map(|y| (ex0.saturating_sub(g)..=(ex1 + g).min(w - 1)).map(move |x| (x, y)))
        .filter(|&(x, y)| mask[y as usize * ww + x as usize])
        .collect();
    let mut rounds = 0;
    while !left.is_empty() && rounds < 200 {
        rounds += 1;
        let mut done: Vec<(usize, [f32; 3])> = vec![];
        for (i, &(x, y)) in left.iter().enumerate() {
            let (mut acc, mut k) = ([0f32; 3], 0f32);
            for oy in -1i64..=1 {
                for ox in -1i64..=1 {
                    let (nx, ny) = (x as i64 + ox, y as i64 + oy);
                    if (ox == 0 && oy == 0) || nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 || mask[ny as usize * ww + nx as usize] {
                        continue;
                    }
                    let p = img.get_pixel(nx as u32, ny as u32);
                    // Ink outside the seal (a frame line, a symbol) never bleeds in.
                    if ink(p) {
                        continue;
                    }
                    let wgt = if ox == 0 || oy == 0 { 1.0 } else { 0.7 };
                    for c in 0..3 {
                        acc[c] += p[c] as f32 * wgt;
                    }
                    k += wgt;
                }
            }
            if k >= 1.5 || (rounds > 20 && k > 0.0) {
                done.push((i, [acc[0] / k, acc[1] / k, acc[2] / k]));
            }
        }
        if done.is_empty() {
            return false;
        }
        for &(i, c) in &done {
            let (x, y) = left[i];
            // The grain: a little steady noise, so the patch isn't glassy smooth next to the printed card.
            let hsh = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) % 1000;
            let jitter = (hsh as f32 / 1000.0 - 0.5) * grain as f32 * 0.9;
            let px = |v: f32| (v + jitter).round().clamp(0.0, 255.0) as u8;
            img.put_pixel(x, y, Rgba([px(c[0]), px(c[1]), px(c[2]), 255]));
        }
        let gone: std::collections::HashSet<usize> = done.iter().map(|d| d.0).collect();
        for &(i, _) in &done {
            let (x, y) = left[i];
            mask[y as usize * ww + x as usize] = false;
        }
        left = left.into_iter().enumerate().filter(|(i, _)| !gone.contains(i)).map(|(_, p)| p).collect();
    }
    if !left.is_empty() {
        return false;
    }
    // Nothing of the seal may be left: no stroke of its own inside its outline.
    !shapes_in(img, (ex0, ey0, ex1 + 1, ey1 + 1)).iter().any(|&((a, b, c, d), n)| a > ex0 && b > ey0 && c < ex1 && d < ey1 && n >= 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> RgbaImage {
        // A plain grey card face with a yellow border.
        let mut img = RgbaImage::from_pixel(600, 825, Rgba([200, 200, 205, 255]));
        for y in 0..825 { for x in 0..600 { if x < 15 || x > 585 || y < 15 || y > 810 { img.put_pixel(x, y, Rgba([240, 210, 40, 255])); } } }
        img
    }

    #[test]
    fn finds_and_removes_the_seal() {
        let mut img = card();
        for y in 455..480i32 { for x in 28..54i32 {
            let d = (((x - 41).pow(2) + (y - 468).pow(2)) as f32).sqrt();
            let one = (x - 41).abs() <= 1 && (y - 468).abs() <= 6;
            if d <= 10.5 && !one { img.put_pixel(x as u32, y as u32, Rgba([20, 20, 20, 255])); }
        } }
        // The arched letters: a few small blocks over the disc.
        for (lx, ly) in [(27, 452), (33, 447), (40, 445), (47, 447), (53, 452)] { for y in ly..ly + 4 { for x in lx..lx + 3 { img.put_pixel(x, y, Rgba([20, 20, 20, 255])); } } }
        assert!(find(&img).is_some(), "the seal is found");
        assert!(erase(&mut img), "and removed");
        let p = img.get_pixel(41, 470);
        assert!(p[0] > 150, "card colour where the seal was: {p:?}");
    }

    #[test]
    fn a_card_without_a_seal_is_left_alone() {
        let mut img = card();
        let before = img.clone();
        assert!(!erase(&mut img));
        assert_eq!(img, before);
    }
}
