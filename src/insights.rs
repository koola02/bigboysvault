//! What's inside the vault, worked out from the real catalogue for the home page: how the cards split by condition,
//! and how their prices spread out, as a smooth curve (a kernel density estimate on a log scale, because card prices span several orders of magnitude).
use vault_search::{cond_class, peso, title_and_set, Card, COND_LABELS};

const W: f64 = 600.0;
const BASE: f64 = 138.0;
const TOP: f64 = 14.0;
const BINS: usize = 18;

#[derive(Clone, Debug)]
pub struct Slice {
    pub key: &'static str,
    pub label: &'static str,
    pub count: u32,
    pub pct: String,
    /// Share of a ring whose length is 100, and where this share starts on it.
    pub len: f64,
    pub at: f64,
}

#[derive(Clone, Debug)]
pub struct Bar {
    pub x: f64,
    pub w: f64,
    pub y: f64,
    pub h: f64,
    pub count: u32,
    pub label: String,
    pub href: String,
}

/// A mark on the price axis. Level 0 is a power of ten or an end of the range, 1 is a five, 2 is a two: smaller screens drop the higher levels.
#[derive(Clone, Debug)]
pub struct Tick {
    pub x: f64,
    pub label: String,
    pub level: u8,
    /// "l" or "r" for the cheapest and the dearest price, which sit at the very ends.
    pub end: &'static str,
}

#[derive(Clone, Debug)]
pub struct Spot {
    pub handle: String,
    pub title: String,
    pub img: String,
    pub price: String,
    pub cond: &'static str,
}

#[derive(Clone, Debug, Default)]
pub struct Insights {
    pub slices: Vec<Slice>,
    pub bars: Vec<Bar>,
    pub curve: String,
    pub area: String,
    pub ticks: Vec<Tick>,
    pub total: u32,
    pub total_text: String,
    pub median: String,
    pub typical: String,
    pub top: Option<Spot>,
    pub newest: Option<Spot>,
}

fn spot(c: &Card) -> Spot {
    Spot { handle: c.handle.clone(), title: title_and_set(&c.title, &c.set).0, img: c.img.clone(), price: peso(c.price), cond: cond_class(&c.variant) }
}

/// A standout tile for one card. `read` can only guess the newest card from the list's order, and the owner can drag
/// cards about, so callers that know the real newest card (by the date it was added) put it in with this.
pub fn spot_of(c: &Card) -> Spot {
    spot(c)
}

/// ₱20, ₱500, ₱2.5k, ₱18k, ₱1.2M: short enough to sit under a chart.
fn short_money(v: f64) -> String {
    let trim = |x: f64| { let s = format!("{:.1}", x); s.trim_end_matches('0').trim_end_matches('.').to_string() };
    if v >= 1_000_000.0 { format!("₱{}M", trim(v / 1_000_000.0)) } else if v >= 1000.0 { format!("₱{}k", trim(v / 1000.0)) } else { peso(v) }
}

/// A price rounded to two significant figures, for the ends of the axis.
fn two_figures(v: f64) -> f64 {
    if v <= 0.0 { return 0.0; }
    let p = 10f64.powf(v.log10().floor() - 1.0);
    (v / p).round() * p
}

fn condition_of(c: &Card) -> &'static str {
    if c.grade.is_empty() { cond_class(&c.variant) } else { "gr" }
}

/// Everything the panel shows. None when the vault has nothing priced to talk about.
pub fn read(cards: &[Card]) -> Option<Insights> {
    let priced: Vec<(f64, f64)> = cards.iter().filter(|c| c.price > 0.0 && c.qty > 0).map(|c| (c.price, f64::from(c.qty))).collect();
    if priced.is_empty() {
        return None;
    }
    let total: u32 = cards.iter().map(|c| c.qty).sum();

    // The condition ring.
    let mut slices = vec![];
    let mut at = 0.0;
    for (key, label) in COND_LABELS {
        let count: u32 = cards.iter().filter(|c| condition_of(c) == key).map(|c| c.qty).sum();
        if count == 0 {
            continue;
        }
        let len = f64::from(count) / f64::from(total) * 100.0;
        slices.push(Slice { key, label, count, pct: if len < 1.0 { "<1".to_string() } else { format!("{:.0}", len) }, len, at });
        at += len;
    }

    // Prices on a log scale.
    let logs: Vec<(f64, f64)> = priced.iter().map(|(p, w)| (p.log10(), *w)).collect();
    let (mut lo, mut hi) = logs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (l, _)| (a.min(*l), b.max(*l)));
    if hi - lo < 0.5 {
        lo -= 0.25;
        hi += 0.25;
    }
    let weight: f64 = logs.iter().map(|x| x.1).sum();
    let mean = logs.iter().map(|(l, w)| l * w).sum::<f64>() / weight;
    let var = logs.iter().map(|(l, w)| (l - mean).powi(2) * w).sum::<f64>() / weight;
    // Silverman's rule for how smooth the curve should be.
    let bandwidth = (1.06 * var.sqrt().max(0.05) * weight.powf(-0.2)).max(0.04);
    let x_of = |l: f64| (l - lo) / (hi - lo) * W;
    let samples: Vec<(f64, f64)> = (0..=72)
        .map(|i| {
            let l = lo + (hi - lo) * f64::from(i) / 72.0;
            let d: f64 = logs.iter().map(|(p, w)| w * (-0.5 * ((l - p) / bandwidth).powi(2)).exp()).sum();
            (l, d)
        })
        .collect();
    let peak = samples.iter().fold(0.0f64, |m, s| m.max(s.1)).max(1e-9);
    let y_of = |d: f64| BASE - d / peak * (BASE - TOP);
    let mut curve = String::new();
    for (i, (l, d)) in samples.iter().enumerate() {
        curve += &format!("{}{:.1} {:.1} ", if i == 0 { "M" } else { "L" }, x_of(*l), y_of(*d));
    }
    let area = format!("{curve}L{W:.1} {BASE:.1} L0 {BASE:.1} Z");
    let typical = samples.iter().fold((lo, 0.0), |best, s| if s.1 > best.1 { *s } else { best }).0;

    // The bars behind the curve, each a way into those cards.
    let step = (hi - lo) / BINS as f64;
    let mut counts = [0u32; BINS];
    for (l, w) in &logs {
        let i = (((l - lo) / step) as usize).min(BINS - 1);
        counts[i] += *w as u32;
    }
    let tallest = f64::from(*counts.iter().max().unwrap_or(&1)).max(1.0);
    let bars = counts
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let (a, b) = (lo + step * i as f64, lo + step * (i + 1) as f64);
            let (from, to) = (10f64.powf(a).floor(), 10f64.powf(b).ceil());
            let h = f64::from(*n) / tallest * (BASE - TOP);
            Bar { x: x_of(a), w: x_of(b) - x_of(a), y: BASE - h, h, count: *n, label: format!("{n} card{} from {} to {}", if *n == 1 { "" } else { "s" }, peso(from), peso(to)), href: format!("/search?min={from}&max={to}") }
        })
        .collect();

    // 10, 20, 50, 100, 200, 500, 1k… across the range, with the real cheapest and dearest prices at the two ends.
    let (cheap, dear) = (10f64.powf(logs.iter().fold(f64::INFINITY, |a, l| a.min(l.0))), 10f64.powf(logs.iter().fold(f64::NEG_INFINITY, |a, l| a.max(l.0))));
    let mut ticks = vec![Tick { x: x_of(cheap.log10()), label: short_money(two_figures(cheap)), level: 0, end: "l" }];
    let room = W * 0.075;
    let mut power = lo.floor() as i32;
    while f64::from(power) <= hi + 1.0 {
        for (m, level) in [(1.0, 0u8), (2.0, 2), (5.0, 1)] {
            let v = m * 10f64.powi(power);
            let x = x_of(v.log10());
            if v > cheap && v < dear && x - ticks[0].x > room && x_of(dear.log10()) - x > room {
                ticks.push(Tick { x, label: short_money(v), level, end: "" });
            }
        }
        power += 1;
    }
    ticks.push(Tick { x: x_of(dear.log10()), label: short_money(two_figures(dear)), level: 0, end: "r" });
    // Marks close to either end give way first on small screens, so they never run into the end labels.
    let (left, right) = (ticks[0].x, x_of(dear.log10()));
    for t in ticks.iter_mut().filter(|t| t.end.is_empty()) {
        if t.x - left < W * 0.17 || right - t.x < W * 0.17 {
            t.level = t.level.max(2);
        }
    }
    ticks.sort_by(|a, b| a.x.total_cmp(&b.x));

    let mut sorted = priced.clone();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut seen = 0.0;
    let median = sorted.iter().find(|(_, w)| { seen += w; seen >= weight / 2.0 }).map_or(0.0, |x| x.0);

    let top = cards.iter().filter(|c| !c.img.is_empty()).max_by(|a, b| a.price.total_cmp(&b.price)).map(spot);
    // A first guess only: the list is in the owner's order. Callers that know the newest card by date set it with spot_of.
    let newest = cards.iter().find(|c| !c.img.is_empty()).map(spot);
    let total_text = peso(f64::from(total)).trim_start_matches('₱').to_string();
    Some(Insights { slices, bars, curve, area, ticks, total, total_text, median: peso(median), typical: peso(10f64.powf(typical)), top, newest })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(title: &str, price: f64, cond: &str, qty: u32) -> Card {
        Card { handle: title.to_lowercase(), title: title.into(), variant: cond.into(), price, qty, img: "/i.jpg".into(), ..Default::default() }
    }

    #[test]
    fn it_adds_up() {
        let list = vec![card("New", 100.0, "Near Mint", 2), card("Mid", 1000.0, "Near Mint", 1), card("Top", 10000.0, "Lightly Played", 1)];
        let i = read(&list).unwrap();
        assert_eq!(i.total, 4);
        assert_eq!(i.slices.iter().map(|s| s.count).sum::<u32>(), 4);
        assert!((i.slices.iter().map(|s| s.len).sum::<f64>() - 100.0).abs() < 1e-6);
        assert_eq!(i.bars.iter().map(|b| b.count).sum::<u32>(), 4);
        assert_eq!(i.top.unwrap().title, "Top");
        assert_eq!(i.newest.unwrap().title, "New");
        assert_eq!(i.median, "₱100");
        assert!(i.curve.starts_with('M') && i.area.ends_with('Z'));
        let labels: Vec<&str> = i.ticks.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels.first(), Some(&"₱100"));
        assert_eq!(labels.last(), Some(&"₱10k"));
        assert!(labels.contains(&"₱1k") && labels.contains(&"₱500") && labels.contains(&"₱200"), "{labels:?}");
        assert!(i.ticks.windows(2).all(|w| w[1].x > w[0].x), "in order, never on top of each other");
        assert_eq!(short_money(8000.0), "₱8k");
        assert_eq!(short_money(2500.0), "₱2.5k");
        assert_eq!(two_figures(18_437.0), 18_000.0);
    }

    #[test]
    fn a_standout_tile_for_any_card() {
        let mut c = card("Pikachu - 057/191 - Surging Sparks", 450.0, "Lightly Played", 1);
        c.set = "Surging Sparks".into();
        let s = spot_of(&c);
        assert_eq!(s.price, "₱450");
        assert_eq!(s.handle, c.handle);
        assert_eq!(s.cond, cond_class("Lightly Played"));
    }

    #[test]
    fn a_vault_with_one_price_still_draws() {
        let i = read(&[card("Only", 500.0, "Near Mint", 1)]).unwrap();
        assert!(!i.curve.contains("NaN") && !i.curve.contains("inf"));
        assert!(read(&[card("Free", 0.0, "Near Mint", 1)]).is_none());
    }
}
