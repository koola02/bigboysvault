//! Asset prep: trims transparent padding off artwork so layout can align the *visible* art
//! (feet on a shared baseline, equal optical size) instead of the padded file box.
//!   cargo run --release --bin trim -- <src.png> <dst.png>
use image::{GenericImageView, RgbaImage};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let img = image::open(&a[1]).expect("open").to_rgba8();
    let (w, h) = img.dimensions();
    let corner = img.get_pixel(0, 0);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    let mut opaque = 0u64;
    for (x, y, p) in img.enumerate_pixels() {
        if p[3] > 16 {
            opaque += 1;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    println!(
        "{}: {w}x{h}, corner rgba={:?}, opaque pixels {:.0}% -> art bbox x {x0}..{x1} y {y0}..{y1} = {}x{}",
        a[1], corner.0, 100.0 * opaque as f64 / (w as f64 * h as f64), x1 - x0 + 1, y1 - y0 + 1
    );
    if a.len() > 2 {
        let out: RgbaImage = img.view(x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image();
        out.save(&a[2]).expect("save");
        println!("  wrote {}", a[2]);
    }
}
