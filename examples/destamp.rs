//! Tries the 1st Edition seal eraser on card pictures: destamp IN... OUTDIR
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (out, ins) = args.split_last().expect("destamp IN... OUTDIR");
    for p in ins {
        let mut img = image::open(p).expect("open").to_rgba8();
        let found = marcuru_rs::stamp::find(&img);
        let ok = marcuru_rs::stamp::erase(&mut img);
        let name = std::path::Path::new(p).file_stem().unwrap().to_string_lossy().to_string();
        img.save(format!("{out}/{name}-unl.png")).expect("save");
        println!("{name}: found {found:?} erased {ok}");
    }
}
