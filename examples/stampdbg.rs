//! Prints what the seal finder sees: the darkest pixels in the strip, by row band.
fn main() {
    for p in std::env::args().skip(1) {
        let img = image::open(&p).unwrap().to_rgba8();
        let (w, h) = img.dimensions();
        let mut best = (255u8, 0, 0);
        let mut dark = 0;
        let (mut minx, mut maxx, mut miny, mut maxy) = (w, 0, h, 0);
        for y in (h as f32 * 0.5) as u32..(h as f32 * 0.64) as u32 {
            for x in (w as f32 * 0.015) as u32..(w as f32 * 0.1) as u32 {
                let q = img.get_pixel(x, y).0;
                let hi = q[0].max(q[1]).max(q[2]);
                if hi < best.0 { best = (hi, x, y); }
                if hi < 110 { dark += 1; minx = minx.min(x); maxx = maxx.max(x); miny = miny.min(y); maxy = maxy.max(y); }
            }
        }
        println!("{} {}x{} darkest {:?} dark<110: {} box x{}-{} y{}-{}", p.rsplit('/').next().unwrap(), w, h, best, dark, minx, maxx, miny, maxy);
    }
}
