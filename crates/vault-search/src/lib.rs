//! The vault's search brain. One copy of this code runs on the server (the `/api/search` route used by AI assistants)
//! and, compiled to WebAssembly, in the browser (the card finder). Both give the same answers because they are the same code.

use std::borrow::Borrow;

/// One card, or several copies of it shown as one entry.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Card {
    pub handle: String,
    pub title: String,
    pub variant: String,
    pub set: String,
    pub no: String,
    pub price: f64,
    pub img: String,
    pub grade: String,
    pub qty: u32,
    /// Positions in the binder list, once for each copy held there.
    pub binders: Vec<u16>,
    pub tags: String,
    /// The set's symbol picture, when the catalogue has one.
    pub sym: String,
    /// Normal, Holofoil, Reverse Holo and so on. See `finish_group`.
    pub finish: String,
    /// "en", "ja" or "zh".
    pub lang: String,
    /// The owner's notes about this copy, on one line.
    pub notes: String,
    /// Normalised text used for matching. Worked out when a card is read, not sent over the wire.
    pub text: String,
    /// Position in the catalogue: 0 is the newest card.
    pub order: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Binder {
    pub handle: String,
    pub title: String,
    pub img: String,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Newest,
    PriceAsc,
    PriceDesc,
    Name,
}

impl Sort {
    pub fn parse(s: &str) -> Sort {
        match s {
            "price-asc" => Sort::PriceAsc,
            "price-desc" => Sort::PriceDesc,
            "title" => Sort::Name,
            _ => Sort::Newest,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Sort::Newest => "",
            Sort::PriceAsc => "price-asc",
            Sort::PriceDesc => "price-desc",
            Sort::Name => "title",
        }
    }
}

/// What someone is looking for, after reading their words.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    /// Words that must match the card's text.
    pub words: Vec<String>,
    pub min_price: Option<f64>,
    pub max_price: Option<f64>,
    pub graded: bool,
    /// Position in the binder list.
    pub binder: Option<u16>,
    /// Set by words like "cheapest". The sort menu is used when this is None.
    pub sort: Option<Sort>,
    /// Pick any number in each list. A card has to fit one choice from every list that has any: Near Mint or Lightly Played, and Holofoil, and Pikachu or Mewtwo.
    pub conds: Vec<String>,
    pub finishes: Vec<String>,
    pub langs: Vec<String>,
    pub names: Vec<String>,
}

/// The conditions someone can filter by, best first.
pub const COND_LABELS: [(&str, &str); 7] = [("nm", "Near Mint"), ("lp", "Lightly Played"), ("mp", "Moderately Played"), ("hp", "Heavily Played"), ("dm", "Damaged"), ("sl", "Sealed"), ("gr", "Graded")];
pub const LANG_LABELS: [(&str, &str); 3] = [("en", "English"), ("ja", "日本語"), ("zh", "中文")];
pub const FINISH_LABELS: [&str; 7] = ["Normal", "Holofoil", "Reverse Holo", "1st Edition", "Unlimited", "Full Art", "Illustration"];

/// One thing to search for in the advanced search.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Token {
    Cond(String),
    Finish(String),
    Lang(String),
    Name(String),
}

impl Token {
    pub fn label(&self) -> String {
        let find = |list: &[(&str, &'static str)], k: &str| list.iter().find(|x| x.0 == k).map(|x| x.1.to_string()).unwrap_or_else(|| k.to_string());
        match self {
            Token::Cond(c) => find(&COND_LABELS, c),
            Token::Lang(l) => find(&LANG_LABELS, l),
            Token::Finish(f) | Token::Name(f) => f.clone(),
        }
    }
}

impl Filter {
    pub fn apply_tokens(&mut self, tokens: &[Token]) {
        for t in tokens {
            match t {
                Token::Cond(c) => self.conds.push(c.clone()),
                Token::Finish(f) => self.finishes.push(f.clone()),
                Token::Lang(l) => self.langs.push(l.clone()),
                Token::Name(n) => self.names.push(n.clone()),
            }
        }
    }
}

/// Every way of saying a condition, finish or language. Longer phrases are tried first.
const PHRASES: [(&str, u8, &str); 30] = [
    ("sealed", 0, "sl"),
    ("near mint", 0, "nm"), ("nm", 0, "nm"), ("mint", 0, "nm"), ("lightly played", 0, "lp"), ("lightly", 0, "lp"), ("lp", 0, "lp"),
    ("moderately played", 0, "mp"), ("moderately", 0, "mp"), ("mp", 0, "mp"), ("heavily played", 0, "hp"), ("heavily", 0, "hp"), ("damaged", 0, "dm"),
    ("graded", 0, "gr"), ("psa", 0, "gr"), ("cgc", 0, "gr"), ("bgs", 0, "gr"),
    ("reverse holofoil", 1, "Reverse Holo"), ("reverse holo", 1, "Reverse Holo"), ("reverse", 1, "Reverse Holo"), ("holofoil", 1, "Holofoil"), ("holo", 1, "Holofoil"),
    ("1st edition", 1, "1st Edition"), ("first edition", 1, "1st Edition"), ("1st", 1, "1st Edition"), ("unlimited", 1, "Unlimited"), ("normal", 1, "Normal"),
    ("english", 2, "en"), ("japanese", 2, "ja"), ("chinese", 2, "zh"),
];

fn word_case(t: &str) -> String {
    t.split(' ').map(|w| { let mut c = w.chars(); c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default() }).collect::<Vec<_>>().join(" ")
}

/// Reads "near mint and holofoil, pikachu, mewtwo" as separate things to look for. Whatever isn't a condition, finish or language is a Pokémon's name.
/// `is_name` says whether several words together are one known name, like "flying pikachu". Otherwise each word is its own name.
pub fn read_tokens(text: &str, is_name: &dyn Fn(&str) -> bool) -> Vec<Token> {
    let mut out: Vec<Token> = vec![];
    let mut add = |t: Token| { if !out.contains(&t) { out.push(t); } };
    let tidy = text.to_lowercase().replace(" and ", ",").replace(['+', '&', '、', '，', ';'], ",");
    for seg in tidy.split(',') {
        let seg = seg.replace("日本語", " japanese ").replace("中文", " chinese ");
        let words: Vec<String> = norm(&seg).split(' ').filter(|w| !w.is_empty()).map(str::to_string).collect();
        let mut rest: Vec<String> = vec![];
        let flush = |rest: &mut Vec<String>, add: &mut dyn FnMut(Token)| {
            if rest.is_empty() { return; }
            let whole = rest.join(" ");
            if rest.len() == 1 || is_name(&whole) { add(Token::Name(word_case(&whole))); } else { for w in rest.iter() { add(Token::Name(word_case(w))); } }
            rest.clear();
        };
        let mut i = 0;
        'words: while i < words.len() {
            let mut best: Option<(usize, u8, &str)> = None;
            for (phrase, kind, value) in PHRASES {
                let want: Vec<&str> = phrase.split(' ').collect();
                if words.len() - i >= want.len() && want.iter().enumerate().all(|(k, w)| words[i + k] == *w) && best.is_none_or(|b| want.len() > b.0) {
                    best = Some((want.len(), kind, value));
                }
            }
            if let Some((n, kind, value)) = best {
                flush(&mut rest, &mut add);
                add(match kind { 0 => Token::Cond(value.into()), 1 => Token::Finish(value.into()), _ => Token::Lang(value.into()) });
                i += n;
                continue 'words;
            }
            rest.push(words[i].clone());
            i += 1;
        }
        flush(&mut rest, &mut add);
    }
    out
}

/// Which group a card's finish belongs to. A card with none recorded counts as Normal, unless its rarity says it's a holo.
pub fn finish_group(finish: &str, rarity: &str) -> &'static str {
    let t = finish.to_lowercase();
    match () {
        _ if t.contains("1st") || t.contains("first") => "1st Edition",
        _ if t.contains("reverse") => "Reverse Holo",
        _ if t.contains("holo") => "Holofoil",
        _ if t.contains("unlimited") => "Unlimited",
        _ if t.contains("full") => "Full Art",
        _ if t.contains("illustration") => "Illustration",
        _ if t.is_empty() && rarity.to_lowercase().contains("holo") => "Holofoil",
        _ => "Normal",
    }
}

/// The price at a spot on the slider (0 to 100). The scale curves, so the many cheap cards get as much room as the few dear ones.
pub fn price_at(pos: u32, lo: f64, hi: f64) -> f64 {
    let (lo, hi) = (lo.max(1.0), hi.max(lo.max(1.0) + 1.0));
    if pos == 0 { return lo.floor(); }
    if pos >= 100 { return hi.ceil(); }
    let v = lo * (hi / lo).powf(f64::from(pos) / 100.0);
    let step = if v < 100.0 { 5.0 } else if v < 1000.0 { 10.0 } else if v < 10_000.0 { 100.0 } else { 500.0 };
    (v / step).round() * step
}

/// The slider spot closest to a price.
pub fn pos_for(price: f64, lo: f64, hi: f64) -> u32 {
    let (lo, hi) = (lo.max(1.0), hi.max(lo.max(1.0) + 1.0));
    ((price.max(lo) / lo).ln() / (hi / lo).ln() * 100.0).round().clamp(0.0, 100.0) as u32
}

/// Lowercase, letters and digits only, single spaces. Works for any alphabet, so Japanese and Chinese names match too.
pub fn norm(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = true;
    for ch in s.chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_alphanumeric() {
            out.push(ch);
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_string()
}

/// Optimal-string-alignment edit distance (insert, delete, substitute, transpose).
pub fn osa(a: &[char], b: &[char]) -> usize {
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=m {
        d[0][j] = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = (a[i - 1] != b[j - 1]) as usize;
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[n][m]
}

/// "₱1,234"
pub fn peso(v: f64) -> String {
    let whole = v.round() as i64;
    let s = whole.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    format!("₱{out}")
}

/// A number someone typed: 500, 1,000, ₱500, 1k, 2.5k.
pub fn amount(word: &str) -> Option<f64> {
    let w = word.trim_start_matches(['₱', '$']).replace(',', "").to_lowercase();
    let w = w.strip_prefix("php").unwrap_or(&w);
    let (digits, times) = match w.strip_suffix('k') {
        Some(d) => (d, 1000.0),
        None => (w, 1.0),
    };
    let v: f64 = digits.parse().ok()?;
    (v.is_finite() && v >= 0.0).then_some(v * times)
}

/// Reads plain words: "pikachu under 500", "graded charizard", "cheapest umbreon", "psa 10 over 1k".
/// Anything that isn't a price or sort instruction is kept as a word to match against the cards.
pub fn read_words(query: &str) -> Filter {
    let mut f = Filter::default();
    let raw: Vec<&str> = query.split_whitespace().take(16).collect();
    let mut i = 0;
    while i < raw.len() {
        let w = raw[i].to_lowercase();
        let next = raw.get(i + 1).and_then(|n| amount(n));
        match w.as_str() {
            "under" | "below" | "less" | "max" | "maximum" | "cheaper" => {
                // "less than 500" and "cheaper than 500"
                let skip_than = raw.get(i + 1).is_some_and(|n| n.eq_ignore_ascii_case("than"));
                let v = if skip_than { raw.get(i + 2).and_then(|n| amount(n)) } else { next };
                if let Some(v) = v {
                    f.max_price = Some(v);
                    i += if skip_than { 3 } else { 2 };
                    continue;
                }
            }
            "over" | "above" | "more" | "min" | "minimum" => {
                let skip_than = raw.get(i + 1).is_some_and(|n| n.eq_ignore_ascii_case("than"));
                let v = if skip_than { raw.get(i + 2).and_then(|n| amount(n)) } else { next };
                if let Some(v) = v {
                    f.min_price = Some(v);
                    i += if skip_than { 3 } else { 2 };
                    continue;
                }
            }
            "cheap" | "cheapest" => {
                f.sort = Some(Sort::PriceAsc);
                i += 1;
                continue;
            }
            "expensive" | "priciest" | "rarest" => {
                f.sort = Some(Sort::PriceDesc);
                i += 1;
                continue;
            }
            "newest" | "latest" => {
                f.sort = Some(Sort::Newest);
                i += 1;
                continue;
            }
            "graded" | "slab" | "slabs" | "slabbed" => {
                f.graded = true;
                i += 1;
                continue;
            }
            "most" if raw.get(i + 1).is_some_and(|n| n.eq_ignore_ascii_case("expensive")) => {
                f.sort = Some(Sort::PriceDesc);
                i += 2;
                continue;
            }
            _ => {}
        }
        // Filler that would otherwise stop every card from matching.
        if !matches!(w.as_str(), "a" | "an" | "the" | "for" | "of" | "with" | "card" | "cards" | "show" | "me" | "find" | "i" | "want" | "any" | "only") {
            f.words.extend(norm(&w).split(' ').filter(|p| !p.is_empty()).map(str::to_string));
        }
        i += 1;
    }
    f
}

fn word_hit(text: &str, word: &str) -> bool {
    text.split(' ').any(|t| t.starts_with(word)) || (word.len() >= 3 && text.contains(word))
}

fn typo_hit(text: &str, word: &str) -> bool {
    let w: Vec<char> = word.chars().collect();
    let allowed = if w.len() >= 7 { 2 } else if w.len() >= 4 { 1 } else { 0 };
    allowed > 0
        && text.split(' ').any(|t| {
            let head: Vec<char> = t.chars().take(w.len()).collect();
            head.len() + allowed >= w.len() && osa(&w, &head) <= allowed
        })
}

/// The result of a search: positions in `cards`, best first.
pub struct Found {
    pub hits: Vec<usize>,
    /// True when nothing matched exactly and the closest spellings are shown instead.
    pub typo: bool,
}

/// Find the cards. `sort` is the sort menu's value; words like "cheapest" take priority over it.
pub fn search<C: Borrow<Card>>(cards: &[C], f: &Filter, sort: Sort) -> Found {
    let card = |i: usize| -> &Card { cards[i].borrow() };
    // Each name finds whole words first ("mew" isn't Mewtwo), then the start of a word ("pika"), then the closest spelling.
    let titles: Vec<String> = if f.names.is_empty() { vec![] } else { (0..cards.len()).map(|i| norm(&title_and_set(&card(i).title, &card(i).set).0)).collect() };
    let name_is = |title: &str, words: &[String], level: u8| match level {
        0 => words.iter().all(|w| title.split(' ').any(|t| t == w)),
        1 => words.iter().all(|w| title.split(' ').any(|t| t.starts_with(w.as_str()))),
        _ => words.iter().all(|w| typo_hit(title, w)),
    };
    let names: Vec<(Vec<String>, u8)> = f.names.iter().map(|n| {
        let words: Vec<String> = norm(n).split(' ').filter(|w| !w.is_empty()).map(str::to_string).collect();
        let level = (0..3u8).find(|l| titles.iter().any(|t| name_is(t, &words, *l))).unwrap_or(0);
        (words, level)
    }).collect();
    let passes = |i: usize| {
        let c = card(i);
        f.min_price.is_none_or(|m| c.price >= m)
            && f.max_price.is_none_or(|m| c.price <= m)
            && (!f.graded || !c.grade.is_empty())
            && f.binder.is_none_or(|b| c.binders.contains(&b))
            && (f.conds.is_empty() || f.conds.iter().any(|x| x == cond_class(&c.variant) || (x == "gr" && !c.grade.is_empty())))
            && (f.finishes.is_empty() || f.finishes.iter().any(|x| *x == c.finish))
            && (f.langs.is_empty() || f.langs.iter().any(|x| *x == c.lang))
            && (names.is_empty() || names.iter().any(|(w, l)| name_is(&titles[i], w, *l)))
    };
    let picked = |test: &dyn Fn(&Card) -> bool| -> Vec<usize> { (0..cards.len()).filter(|&i| passes(i) && test(card(i))).collect() };
    let mut hits = picked(&|c| f.words.iter().all(|w| word_hit(&c.text, w)));
    let mut typo = !hits.is_empty() && names.iter().any(|(_, l)| *l == 2);
    if hits.is_empty() && !f.words.is_empty() {
        hits = picked(&|c| f.words.iter().all(|w| word_hit(&c.text, w) || typo_hit(&c.text, w)));
        typo = !hits.is_empty();
    }
    // Typed words and no sort asked for: the best match comes first, and the owner's order settles ties.
    let query: Vec<String> = f.words.iter().cloned().chain(names.iter().flat_map(|(w, _)| w.iter().cloned())).collect();
    if f.sort.is_none() && sort == Sort::Newest && !query.is_empty() && hits.len() > 1 {
        let all: Vec<&str> = (0..cards.len()).map(|i| card(i).text.as_str()).collect();
        let heads: Vec<String> = hits.iter().map(|&i| norm(&title_and_set(&card(i).title, &card(i).set).0)).collect();
        let scores = relevance(&query, &all, &hits, &heads);
        let mut ranked: Vec<(usize, f64)> = hits.iter().copied().zip(scores).collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(card(a.0).order.cmp(&card(b.0).order)));
        hits = ranked.into_iter().map(|(i, _)| i).collect();
        return Found { hits, typo };
    }
    match f.sort.unwrap_or(sort) {
        Sort::Newest => hits.sort_by_key(|&i| card(i).order),
        Sort::PriceAsc => hits.sort_by(|&a, &b| card(a).price.total_cmp(&card(b).price).then(card(a).order.cmp(&card(b).order))),
        Sort::PriceDesc => hits.sort_by(|&a, &b| card(b).price.total_cmp(&card(a).price).then(card(a).order.cmp(&card(b).order))),
        Sort::Name => hits.sort_by_cached_key(|&i| (card(i).title.to_lowercase(), card(i).order)),
    }
    Found { hits, typo }
}

/// How well each hit matches the typed words: a BM25-style score.
///
///   score(card) = Σ_w  idf(w) · q(card, w) · (k + 1) / (1 + k · (1 − b + b · len / avg_len))
///   idf(w)      = ln(1 + (N − df(w) + ½) / (df(w) + ½))     N cards, df(w) of them contain w
///   q(card, w)  = 3 when w is a whole word of the name, 2 when a word of the name starts with it,
///                 1.2 a whole word anywhere else (set, tags, notes), 1 the start of one, 0.6 inside one, 0.35 a near spelling
///   k = 1.2, b = 0.5, len = words in the name, so "Pikachu" outranks "Pikachu & Zekrom GX" for "pikachu".
/// Rare words count for more than common ones, the name counts for more than the rest, and a shorter name that is all match wins.
pub fn relevance(words: &[String], all_texts: &[&str], hits: &[usize], heads: &[String]) -> Vec<f64> {
    let n = all_texts.len().max(1) as f64;
    let idf: Vec<f64> = words
        .iter()
        .map(|w| {
            let df = all_texts.iter().filter(|t| word_hit(t, w)).count() as f64;
            (1.0 + (n - df + 0.5) / (df + 0.5)).ln()
        })
        .collect();
    let lens: Vec<f64> = heads.iter().map(|h| h.split(' ').filter(|t| !t.is_empty()).count().max(1) as f64).collect();
    let avg = (lens.iter().sum::<f64>() / lens.len().max(1) as f64).max(1.0);
    let (k, b) = (1.2, 0.5);
    hits.iter()
        .enumerate()
        .map(|(j, &i)| {
            let (head, text) = (heads[j].as_str(), all_texts[i]);
            let norm_len = (k + 1.0) / (1.0 + k * (1.0 - b + b * lens[j] / avg));
            words
                .iter()
                .zip(&idf)
                .map(|(w, idf)| {
                    let q = if head.split(' ').any(|t| t == w) {
                        3.0
                    } else if head.split(' ').any(|t| t.starts_with(w.as_str())) {
                        2.0
                    } else if text.split(' ').any(|t| t == w) {
                        1.2
                    } else if text.split(' ').any(|t| t.starts_with(w.as_str())) {
                        1.0
                    } else if w.len() >= 3 && text.contains(w.as_str()) {
                        0.6
                    } else if typo_hit(text, w) {
                        0.35
                    } else {
                        0.0
                    };
                    idf * q * norm_len
                })
                .sum()
        })
        .collect()
}

/// A title without the set on the end, plus the set: "Pikachu - 017/71 - Clay Burst" becomes ("Pikachu - 017/71", "Clay Burst").
/// The set field is used when it's filled in. Older cards only have it inside the title.
pub fn title_and_set(title: &str, set: &str) -> (String, String) {
    let set = set.trim();
    if !set.is_empty() {
        let short = title.strip_suffix(set).map(|t| t.trim_end().trim_end_matches('-').trim_end()).filter(|t| !t.is_empty());
        return (short.unwrap_or(title).to_string(), set.to_string());
    }
    match title.rsplit_once(" - ") {
        Some((head, last)) if head.contains(" - ") => (head.to_string(), last.trim().to_string()),
        _ => (title.to_string(), String::new()),
    }
}

/// Tabs and line breaks separate fields and rows, so they can't appear inside one.
fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

pub fn binder_line(b: &Binder) -> String {
    format!("B\t{}\t{}\t{}\t{}\n", clean(&b.handle), clean(&b.title), clean(&b.img), b.count)
}

/// The set's symbol from TCGdex, the catalogue card ids come from ("tcgdex:sv10-217"). Nothing for cards outside it. A set it has no symbol for just shows no symbol.
pub fn set_symbol(card_id: &str) -> String {
    let Some((set, _)) = card_id.trim().strip_prefix("tcgdex:").and_then(|id| id.rsplit_once('-')) else { return String::new() };
    if !set.chars().all(|c| c.is_ascii_alphanumeric() || c == '.') {
        return String::new();
    }
    // The few sets TCGdex has no symbol for are in the Pokémon TCG image library, under their own codes.
    let elsewhere = match set {
        "sv07" => Some("sv7"),
        "sv08" => Some("sv8"),
        "sv08.5" => Some("sv8pt5"),
        "svp" => Some("svp"),
        _ => None,
    };
    if let Some(code) = elsewhere {
        return format!("https://images.pokemontcg.io/{code}/symbol.png");
    }
    let series = match set {
        "cel25" => Some("swsh"),
        "30th" => Some("me"),
        "det1" => Some("sm"),
        "g1" => Some("xy"),
        _ => ["base", "gym", "neo", "ecard", "ex", "dp", "pl", "hgss", "bw", "xy", "sm", "swsh", "sv", "me"].into_iter().find(|p| set.starts_with(*p)),
    };
    series.map(|series| format!("https://assets.tcgdex.net/en/{series}/{set}/symbol.png")).unwrap_or_default()
}

/// Which colour a condition's pill gets: "nm" emerald, "lp" amber, "mp" coral, "hp" and "dm" reds, "gr" brass for graded.
pub fn cond_class(text: &str) -> &'static str {
    // A range like "LP-NM" is written worst first, so it counts as its lower grade: LP-NM is Lightly Played.
    let t = text.trim().to_lowercase().replace(['-', '/'], " ");
    let first = t.split_whitespace().next().unwrap_or("");
    match () {
        _ if t.starts_with("near mint") || t.starts_with("mint") || first == "nm" => "nm",
        _ if t.starts_with("lightly") || first == "lp" => "lp",
        _ if t.starts_with("moderately") || first == "mp" => "mp",
        _ if t.starts_with("heavily") || first == "hp" => "hp",
        _ if t.starts_with("damaged") || first == "dmg" => "dm",
        _ if t.starts_with("sealed") || t.split_whitespace().any(|w| w.trim_matches(|c: char| !c.is_alphanumeric()) == "sealed") => "sl",
        // A slab: the grading company comes first, with or without a space before the grade ("PSA 10", "PSA10", "ACE 10").
        _ if ["psa", "cgc", "bgs", "sgc", "ace", "tag"].iter().any(|g| first.strip_prefix(g).is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit() || c == '.'))) || t.contains("slab") => "gr",
        _ => "",
    }
}

/// The short words on a tile's pill: "Near Mint · Reverse Holo". Older imports hold everything on one line, rarity included, and a plain finish isn't worth repeating.
pub fn tile_condition(text: &str) -> String {
    let words = |t: &str| -> String {
        if t.chars().any(char::is_uppercase) { return t.to_string(); }
        t.split(['-', ' ']).filter(|w| !w.is_empty()).map(|w| { let mut c = w.chars(); c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default() }).collect::<Vec<_>>().join(" ")
    };
    let parts: Vec<&str> = text.split(" / ").flat_map(|p| p.split(" · ")).map(str::trim).filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        [] => String::new(),
        [one] => words(one),
        [c, f, ..] if f.eq_ignore_ascii_case("normal") => words(c),
        [c, f, ..] => format!("{} · {}", words(c), words(f)),
    }
}

/// Which foil a tile's shine imitates, from its name and finish.
pub fn foil_class(text: &str) -> &'static str {
    let t = text.to_lowercase();
    if t.contains("1st") || t.contains("first edition") { "first" }
    else if t.contains("reverse") { "reverse" }
    else if t.contains(" ex ") || t.contains(" ex-") || t.ends_with(" ex") || t.contains(" gx") || t.contains(" vmax") || t.contains(" vstar") || t.contains("full art") || t.contains("full-art") || t.contains("illustration") { "ex" }
    else if t.contains("holo") { "holo" }
    else { "plain" }
}

impl Card {
    /// How many copies are in this binder, or in the whole vault.
    pub fn copies_in(&self, binder: Option<u16>) -> u32 {
        match binder {
            Some(b) => self.binders.iter().filter(|x| **x == b).count() as u32,
            None => self.qty,
        }
    }
}

pub fn card_line(c: &Card) -> String {
    let binders = c.binders.iter().map(u16::to_string).collect::<Vec<_>>().join(",");
    format!(
        "C\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        clean(&c.handle), clean(&c.title), clean(&c.variant), clean(&c.set), clean(&c.no), c.price, clean(&c.img), clean(&c.grade), c.qty, binders, clean(&c.tags), clean(&c.sym), clean(&c.finish), clean(&c.lang), clean(&c.notes)
    )
}

/// Read the catalogue file. It's plain tab-separated text, which keeps the browser download small and quick to read.
pub fn parse(data: &str) -> (Vec<Binder>, Vec<Card>) {
    let (mut binders, mut cards) = (vec![], vec![]);
    for line in data.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.as_slice() {
            ["B", handle, title, img, count] => binders.push(Binder { handle: handle.to_string(), title: title.to_string(), img: img.to_string(), count: count.parse().unwrap_or(0) }),
            // Older files have no notes at the end.
            ["C", handle, title, variant, set, no, price, img, grade, qty, bs, tags, sym, finish, lang, extra @ ..] if extra.len() <= 1 => {
                let notes = extra.first().copied().unwrap_or("");
                let text = norm(&format!("{title} {variant} {set} {no} {grade} {tags} {notes}"));
                cards.push(Card {
                    handle: handle.to_string(),
                    title: title.to_string(),
                    variant: variant.to_string(),
                    set: set.to_string(),
                    no: no.to_string(),
                    price: price.parse().unwrap_or(0.0),
                    img: img.to_string(),
                    grade: grade.to_string(),
                    qty: qty.parse().unwrap_or(1),
                    binders: bs.split(',').filter_map(|x| x.parse().ok()).collect(),
                    tags: tags.to_string(),
                    sym: sym.to_string(),
                    finish: finish.to_string(),
                    lang: lang.to_string(),
                    notes: notes.to_string(),
                    text,
                    order: cards.len() as u32,
                });
            }
            _ => {}
        }
    }
    (binders, cards)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny(title: &str, variant: &str, finish: &str, lang: &str, price: f64) -> Card {
        Card { notes: String::new(), sym: String::new(), finish: finish.into(), lang: lang.into(), handle: norm(title), title: title.into(), variant: variant.into(), set: String::new(), no: String::new(), price, img: String::new(), grade: String::new(), qty: 1, binders: vec![], tags: String::new(), text: norm(title), order: 0 }
    }

    #[test]
    fn a_combination_needs_one_choice_from_every_list() {
        let cards = vec![
            tiny("Pikachu - 1/9", "Near Mint · Holofoil", "Holofoil", "en", 100.0),
            tiny("Pikachu - 2/9", "Lightly Played", "Normal", "en", 200.0),
            tiny("Mewtwo - 3/9", "Near Mint · Holofoil", "Holofoil", "ja", 300.0),
            tiny("Mew - 4/9", "Near Mint · Holofoil", "Holofoil", "en", 400.0),
            tiny("Eevee - 5/9", "Near Mint", "Normal", "en", 500.0),
        ];
        let run = |t: &str| { let mut f = Filter::default(); f.apply_tokens(&read_tokens(t, &|_| false)); let mut v: Vec<&str> = search(&cards, &f, Sort::Newest).hits.iter().map(|i| &*cards[*i].title).collect(); v.sort(); v };
        assert_eq!(run("near mint and holofoil, pikachu, mewtwo"), vec!["Mewtwo - 3/9", "Pikachu - 1/9"]);
        assert_eq!(run("near mint holofoil pikachu mewtwo"), vec!["Mewtwo - 3/9", "Pikachu - 1/9"], "commas are optional");
        assert_eq!(run("nm, lp, pikachu"), vec!["Pikachu - 1/9", "Pikachu - 2/9"], "two conditions means either");
        assert_eq!(run("mew"), vec!["Mew - 4/9"], "a whole word isn't Mewtwo");
        assert_eq!(run("pika"), vec!["Pikachu - 1/9", "Pikachu - 2/9"], "the start of a name still works");
        assert_eq!(run("pikachoo"), vec!["Pikachu - 1/9", "Pikachu - 2/9"], "and a misspelling");
        assert_eq!(run("日本語, holo"), vec!["Mewtwo - 3/9"]);
        assert_eq!(run("damaged pikachu"), Vec::<&str>::new());
        // Words that are one known name stay together.
        assert_eq!(read_tokens("flying pikachu", &|n| n == "flying pikachu"), vec![Token::Name("Flying Pikachu".into())]);
        assert_eq!(read_tokens("flying pikachu", &|_| false), vec![Token::Name("Flying".into()), Token::Name("Pikachu".into())]);
        assert_eq!(read_tokens("Reverse Holo, NM", &|_| false), vec![Token::Finish("Reverse Holo".into()), Token::Cond("nm".into())]);
    }

    #[test]
    fn the_best_match_comes_first() {
        let mut cards = vec![
            tiny("Pikachu & Zekrom GX - 33/181", "", "", "en", 900.0),
            tiny("Raichu - 2/9", "", "", "en", 50.0),
            tiny("Pikachu - 25/102", "", "", "en", 80.0),
            tiny("Surfing Pikachu V - 8/25", "", "", "en", 300.0),
        ];
        cards[1].text = norm("Raichu - 2/9 evolves from pikachu");
        for (i, c) in cards.iter_mut().enumerate() { c.order = i as u32; }
        let run = |t: &str, sort: Sort| { let mut f = Filter::default(); f.apply_tokens(&read_tokens(t, &|_| false)); search(&cards, &f, sort).hits.iter().map(|i| cards[*i].title.clone()).collect::<Vec<_>>() };
        let best = run("pikachu", Sort::Newest);
        assert_eq!(best[0], "Pikachu - 25/102", "the card that's just Pikachu first: {best:?}");
        assert_eq!(run("pikachu", Sort::PriceDesc)[0], "Pikachu & Zekrom GX - 33/181", "a chosen sort still wins");
        // A rare word matters more than a common one.
        let s = relevance(&["zekrom".into(), "pikachu".into()], &cards.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), &[0, 2], &[norm("Pikachu & Zekrom GX"), norm("Pikachu")]);
        assert!(s[0] > s[1], "{s:?}");
    }

    #[test]
    fn the_price_slider_curves_and_finds_its_way_back() {
        assert_eq!(price_at(0, 15.0, 6500.0), 15.0);
        assert_eq!(price_at(100, 15.0, 6500.0), 6500.0);
        let mid = price_at(50, 15.0, 6500.0);
        assert!(mid > 200.0 && mid < 400.0, "the middle of the slider is a typical card, not half the dearest: {mid}");
        assert_eq!(pos_for(price_at(50, 15.0, 6500.0), 15.0, 6500.0), 50);
        assert_eq!(finish_group("Reverse Holo", ""), "Reverse Holo");
        assert_eq!(finish_group("", "Holo Rare"), "Holofoil");
        assert_eq!(finish_group("", "Common"), "Normal");
        assert_eq!(finish_group("1st Ed.", ""), "1st Edition");
    }

    #[test]
    fn pills_read_every_way_a_condition_is_written() {
        assert_eq!(cond_class("Near Mint"), "nm");
        assert_eq!(cond_class("near-mint / normal / common"), "nm");
        assert_eq!(cond_class("Lightly Played · Holofoil"), "lp");
        assert_eq!(cond_class("Moderately Played"), "mp");
        assert_eq!(cond_class("PSA 10"), "gr");
        assert_eq!(cond_class("PSA10"), "gr");
        assert_eq!(cond_class("ACE 10"), "gr");
        assert_eq!(cond_class("TAG 9.5"), "gr");
        assert_eq!(cond_class("Ungraded"), "");
        assert_eq!(cond_class("Aceptable"), "");
        assert_eq!(cond_class("Booster Box (sealed)"), "sl");
        assert_eq!(cond_class("whatever"), "");
        assert_eq!(tile_condition("near-mint / normal / common"), "Near Mint");
        assert_eq!(tile_condition("Lightly Played / reverse-holofoil / Rare"), "Lightly Played · Reverse Holofoil");
        assert_eq!(tile_condition("Near Mint · Holofoil"), "Near Mint · Holofoil");
        assert_eq!(tile_condition("PSA 10"), "PSA 10");
        assert_eq!(tile_condition(""), "");
    }

    #[test]
    fn set_symbols_come_from_the_card_id() {
        assert_eq!(set_symbol("tcgdex:sv02-195"), "https://assets.tcgdex.net/en/sv/sv02/symbol.png");
        assert_eq!(set_symbol("tcgdex:sv08-147"), "https://images.pokemontcg.io/sv8/symbol.png");
        assert_eq!(set_symbol("tcgdex:sv08.5-124"), "https://images.pokemontcg.io/sv8pt5/symbol.png");
        assert_eq!(set_symbol("tcgdex:swsh10.5-065"), "https://assets.tcgdex.net/en/swsh/swsh10.5/symbol.png");
        assert_eq!(set_symbol("tcgdex:swshp-SWSH229"), "https://assets.tcgdex.net/en/swsh/swshp/symbol.png");
        assert_eq!(set_symbol("tcgdex:cel25-1"), "https://assets.tcgdex.net/en/swsh/cel25/symbol.png");
        assert_eq!(set_symbol("tcgdex-ja:SV2D-1"), "");
        assert_eq!(set_symbol(""), "");
        assert_eq!(set_symbol("tcgdex:30th-1"), "https://assets.tcgdex.net/en/me/30th/symbol.png");
        assert_eq!(set_symbol("tcgdex:unknown-1"), "", "no series known, so no broken picture");
    }

    #[test]
    fn the_shine_follows_the_finish() {
        assert_eq!(foil_class("Pikachu 1st Edition Holofoil"), "first");
        assert_eq!(foil_class("Zorua Near Mint · Reverse Holo"), "reverse");
        assert_eq!(foil_class("Mewtwo ex Near Mint"), "ex");
        assert_eq!(foil_class("Raichu Near Mint · Holofoil"), "holo");
        assert_eq!(foil_class("Dratini Near Mint"), "plain");
    }

    fn deck() -> Vec<Card> {
        let mut data = String::new();
        for (t, p, g, b) in [("Pikachu VMAX", 500.0, "", "0"), ("Charizard ex", 9000.0, "PSA 10", "0,1"), ("Umbreon VMAX", 1500.0, "", "1"), ("ピカチュウ", 200.0, "", "")] {
            data += &card_line(&Card { notes: String::new(), sym: String::new(), finish: String::new(), lang: String::new(), handle: norm(t).replace(' ', "-"), title: t.into(), variant: String::new(), set: "Test Set".into(), no: "7".into(), price: p, img: "/x.jpg".into(), grade: g.into(), qty: 1, binders: b.split(',').filter_map(|x| x.parse().ok()).collect(), tags: String::new(), text: String::new(), order: 0 });
        }
        parse(&data).1
    }

    #[test]
    fn plain_words_become_filters() {
        let f = read_words("cheapest graded charizard under 1,000");
        assert_eq!(f.words, vec!["charizard"]);
        assert_eq!(f.max_price, Some(1000.0));
        assert!(f.graded);
        assert_eq!(f.sort, Some(Sort::PriceAsc));
        let f = read_words("umbreon more than 2k");
        assert_eq!((f.words, f.min_price), (vec!["umbreon".to_string()], Some(2000.0)));
        assert_eq!(read_words("show me pikachu cards").words, vec!["pikachu"]);
        // "over" with no number is an ordinary word.
        assert_eq!(read_words("game over").words, vec!["game", "over"]);
    }

    #[test]
    fn finds_and_sorts() {
        let d = deck();
        let hit = search(&d, &read_words("vmax under 1000"), Sort::Newest);
        assert_eq!(hit.hits.iter().map(|&i| d[i].title.as_str()).collect::<Vec<_>>(), vec!["Pikachu VMAX"]);
        let hit = search(&d, &Filter::default(), Sort::PriceDesc);
        assert_eq!(d[hit.hits[0]].title, "Charizard ex");
        assert_eq!(search(&d, &read_words("graded"), Sort::Newest).hits.len(), 1);
        assert_eq!(search(&d, &Filter { binder: Some(1), ..Default::default() }, Sort::Newest).hits.len(), 2);
        assert_eq!(search(&d, &read_words("ピカチュウ"), Sort::Newest).hits.len(), 1);
        assert_eq!(search(&d, &read_words("psa 10"), Sort::Newest).hits.len(), 1);
    }

    #[test]
    fn typos_are_forgiven_only_when_nothing_matches() {
        let d = deck();
        let hit = search(&d, &read_words("pikahcu"), Sort::Newest);
        assert!(hit.typo);
        assert_eq!(d[hit.hits[0]].title, "Pikachu VMAX");
        assert!(!search(&d, &read_words("pikachu"), Sort::Newest).typo);
        assert!(search(&d, &read_words("zzzzzzzz"), Sort::Newest).hits.is_empty());
    }

    #[test]
    fn in_between_grades_count_as_the_lower_one() {
        assert_eq!(cond_class("LP-NM"), "lp");
        assert_eq!(cond_class("MP-LP"), "mp");
        assert_eq!(cond_class("HP-MP"), "hp");
        assert_eq!(cond_class("Damaged-HP"), "dm");
        assert_eq!(cond_class("Sealed"), "sl");
        assert_eq!(cond_class("Near Mint"), "nm");
        assert_eq!(cond_class("near-mint / holofoil"), "nm");
        assert_eq!(tile_condition("LP-NM"), "LP-NM");
        let old = "C\th\tT\t\t\t\t1\t\t\t1\t\t\t\t\t\n";
        assert_eq!(parse(old).1.len(), 1, "a file without notes still reads");
    }

    #[test]
    fn copies_are_counted_per_binder() {
        let line = "C\th\tT\t\t\t\t1\t\t\t3\t6,6,7\t\t\t\t\n";
        let (_, cs) = parse(line);
        assert_eq!((cs[0].copies_in(Some(6)), cs[0].copies_in(Some(7)), cs[0].copies_in(Some(1)), cs[0].copies_in(None)), (2, 1, 0, 3));
    }

    #[test]
    fn round_trip_survives_tabs_and_keeps_order() {
        let c = Card { sym: String::new(), finish: String::new(), lang: String::new(), notes: "LP-NM".into(), handle: "a".into(), title: "A\tB".into(), variant: "x\ny".into(), set: String::new(), no: String::new(), price: 12.5, img: String::new(), grade: String::new(), qty: 3, binders: vec![2, 5], tags: String::new(), text: String::new(), order: 0 };
        let b = Binder { handle: "h".into(), title: "T".into(), img: "/i".into(), count: 4 };
        let (bs, cs) = parse(&(binder_line(&b) + &card_line(&c) + &card_line(&c)));
        assert_eq!(bs, vec![b]);
        assert_eq!((cs[0].title.as_str(), cs[0].qty, cs[0].binders.clone(), cs[1].order), ("A B", 3, vec![2, 5], 1));
    }

    #[test]
    fn titles_split_from_sets() {
        assert_eq!(title_and_set("Pikachu - 017/71 - Clay Burst", "Clay Burst"), ("Pikachu - 017/71".into(), "Clay Burst".into()));
        assert_eq!(title_and_set("Pikachu - 017/71 - Clay Burst", ""), ("Pikachu - 017/71".into(), "Clay Burst".into()));
        assert_eq!(title_and_set("Sleeves - Clear", ""), ("Sleeves - Clear".into(), "".into()));
        assert_eq!(title_and_set("Pikachu", "Jungle"), ("Pikachu".into(), "Jungle".into()));
    }

    #[test]
    fn peso_formats() {
        assert_eq!(peso(1234567.4), "₱1,234,567");
        assert_eq!(peso(0.0), "₱0");
    }
}
