fn main() {
    for p in std::env::args().skip(1) {
        let img = image::open(&p).unwrap().to_rgba8();
        let (w, h) = img.dimensions();
        let name = p.rsplit('/').next().unwrap().to_string();
        let mut v: Vec<_> = marcuru_rs::stamp::shapes(&img, marcuru_rs::stamp::ZONES[0]).into_iter().filter(|s| s.1 >= 30).collect();
        v.sort_by_key(|s| std::cmp::Reverse(s.1));
        let txt: Vec<String> = v.iter().take(4).map(|((x0, y0, x1, y1), n)| { let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1); format!("[{x0},{y0} {bw}x{bh} n{n} fill{:.2} w{:.3} h{:.3}]", *n as f32 / (bw * bh) as f32, bw as f32 / w as f32, bh as f32 / h as f32) }).collect();
        println!("{name}: {}", txt.join(" "));
    }
}
