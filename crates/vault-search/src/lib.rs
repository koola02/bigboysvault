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
    /// Normalised text used for matching: the card's own words only, never its store's name (see `search_market`).
    /// Worked out when a card is read, not sent over the wire.
    pub text: String,
    /// Position in the catalogue: 0 is the newest card.
    pub order: u32,
    /// Who sells it: 0 is the vault, any other number is that place in the store list plus one (see `parse_market`).
    pub store: u16,
    /// A store's card is one listing (`/trainers/{seller}/cards/{lid}`). 0 for the vault's own cards.
    pub lid: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Binder {
    pub handle: String,
    pub title: String,
    pub img: String,
    pub count: u32,
}

/// A seller's store with cards for sale on The Vault page.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Store {
    /// The seller's member number.
    pub id: i64,
    /// The store's address (`/store/{slug}`). Can be empty.
    pub slug: String,
    pub title: String,
    /// The seller's picture.
    pub img: String,
    /// How many of the store's cards the file has. Counted when the file is read, not sent.
    pub count: u32,
}

impl Store {
    /// What the address bar calls this store (`?store=`): its address, or its number when it has none.
    /// ("vault" means the vault's own cards, so a store with that address goes by its number too.)
    pub fn key(&self) -> String {
        if self.slug.is_empty() || self.slug.eq_ignore_ascii_case("vault") { self.id.to_string() } else { self.slug.clone() }
    }
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
    /// Who sells it, as `Card::store`: 0 for the vault's own cards.
    pub store: Option<u16>,
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
    search_market::<C, Store>(cards, &[], f, sort)
}

/// `search` for The Vault page's list (`parse_market`): `stores` are the stores its cards' `store` numbers point to.
/// A store's name is not in its cards' text, so "pikachu" finds Pikachu cards only, never every card of a store called
/// "Pikachu Corner". A typed word that some card has in its own text must be in the card's own text. Only a word that no
/// card has, but a store's name or address does, asks for that store's cards: "cerulean" finds Cerulean Gym's cards,
/// and "charizard cerulean" the Charizards it sells.
pub fn search_market<C: Borrow<Card>, S: Borrow<Store>>(cards: &[C], stores: &[S], f: &Filter, sort: Sort) -> Found {
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
        // A card with no price yet ("Ask for a price") isn't in any price range.
        f.min_price.is_none_or(|m| c.price >= m)
            && f.max_price.is_none_or(|m| c.price <= m && c.price > 0.0)
            && (!f.graded || !c.grade.is_empty())
            && f.binder.is_none_or(|b| c.binders.contains(&b))
            && f.store.is_none_or(|s| c.store == s)
            && (f.conds.is_empty() || f.conds.iter().any(|x| x == cond_class(&c.variant) || (x == "gr" && !c.grade.is_empty())))
            && (f.finishes.is_empty() || f.finishes.iter().any(|x| *x == c.finish))
            && (f.langs.is_empty() || f.langs.iter().any(|x| *x == c.lang))
            && (names.is_empty() || names.iter().any(|(w, l)| name_is(&titles[i], w, *l)))
    };
    let picked = |test: &dyn Fn(&Card) -> bool| -> Vec<usize> { (0..cards.len()).filter(|&i| passes(i) && test(card(i))).collect() };
    // What each typed word asks for: None, the card's own words; Some, the cards of these stores (`Card::store`).
    // A word is a store's only when no card has it in its own text and a store's name or address does.
    let store_texts: Vec<String> = stores.iter().map(|s| { let s = s.borrow(); norm(&format!("{} {}", s.title, s.slug)) }).collect();
    let read = |w: &str, hit: &dyn Fn(&str, &str) -> bool| -> Option<Vec<u16>> {
        if store_texts.is_empty() || (0..cards.len()).any(|i| hit(&card(i).text, w)) {
            return None;
        }
        let from: Vec<u16> = store_texts.iter().enumerate().filter(|(_, t)| hit(t, w)).map(|(i, _)| i as u16 + 1).collect();
        (!from.is_empty()).then_some(from)
    };
    let exact = |t: &str, w: &str| word_hit(t, w);
    let near = |t: &str, w: &str| word_hit(t, w) || typo_hit(t, w);
    let fits = |c: &Card, from: &[Option<Vec<u16>>], hit: &dyn Fn(&str, &str) -> bool| f.words.iter().zip(from).all(|(w, s)| match s {
        Some(s) => s.contains(&c.store),
        None => hit(&c.text, w),
    });
    let mut from: Vec<Option<Vec<u16>>> = f.words.iter().map(|w| read(w, &exact)).collect();
    let mut hits = picked(&|c| fits(c, &from, &exact));
    let mut typo = !hits.is_empty() && names.iter().any(|(_, l)| *l == 2);
    if hits.is_empty() && !f.words.is_empty() {
        // Nothing matched exactly: near spellings count, of a card's words or of a store's name.
        for (w, s) in f.words.iter().zip(from.iter_mut()) {
            if s.is_none() {
                *s = read(w, &near);
            }
        }
        hits = picked(&|c| fits(c, &from, &near));
        typo = !hits.is_empty();
    }
    // Typed words and no sort asked for: the best match comes first, and the owner's order settles ties.
    // A word that only picked a store says nothing about which of its cards matches best, so it doesn't count here.
    let query: Vec<String> = f.words.iter().zip(&from).filter(|(_, s)| s.is_none()).map(|(w, _)| w.clone()).chain(names.iter().flat_map(|(w, _)| w.iter().cloned())).collect();
    if f.sort.is_none() && sort == Sort::Newest && !query.is_empty() && hits.len() > 1 {
        let all: Vec<&str> = (0..cards.len()).map(|i| card(i).text.as_str()).collect();
        let heads: Vec<String> = hits.iter().map(|&i| norm(&title_and_set(&card(i).title, &card(i).set).0)).collect();
        let scores = relevance(&query, &all, &hits, &heads);
        let mut ranked: Vec<(usize, f64)> = hits.iter().copied().zip(scores).collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(card(a.0).order.cmp(&card(b.0).order)));
        hits = ranked.into_iter().map(|(i, _)| i).collect();
        return Found { hits, typo };
    }
    // Either way round, cards with no price yet come last.
    let unpriced = |i: usize| card(i).price <= 0.0;
    match f.sort.unwrap_or(sort) {
        Sort::Newest => {
            hits.sort_by_key(|&i| card(i).order);
            hits = mix_in_stores(&hits, |i| card(i).store > 0);
        }
        Sort::PriceAsc => hits.sort_by(|&a, &b| unpriced(a).cmp(&unpriced(b)).then(card(a).price.total_cmp(&card(b).price)).then(card(a).order.cmp(&card(b).order))),
        Sort::PriceDesc => hits.sort_by(|&a, &b| unpriced(a).cmp(&unpriced(b)).then(card(b).price.total_cmp(&card(a).price)).then(card(a).order.cmp(&card(b).order))),
        Sort::Name => hits.sort_by_cached_key(|&i| (card(i).title.to_lowercase(), card(i).order)),
    }
    Found { hits, typo }
}

/// How many of the vault's cards come before each store card in the default order.
pub const VAULT_RUN: usize = 3;

/// The default order with the stores' cards in it: the vault's own order, with one store card (newest first) after
/// every `VAULT_RUN` of the vault's, until the store cards run out; the rest of the vault follows. Should there be more
/// store cards than that, the rest of them come last. `hits` is already in catalogue order; nothing else is reordered.
fn mix_in_stores(hits: &[usize], from_store: impl Fn(usize) -> bool) -> Vec<usize> {
    let (vault, stores): (Vec<usize>, Vec<usize>) = hits.iter().partition(|&&i| !from_store(i));
    let mut stores = stores.into_iter();
    let mut out = Vec::with_capacity(hits.len());
    for (n, v) in vault.into_iter().enumerate() {
        out.push(v);
        if (n + 1) % VAULT_RUN == 0 {
            out.extend(stores.next());
        }
    }
    out.extend(stores);
    out
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
        "base1" => Some("base1"),
        "sm3.5" => Some("sm35"),
        "sv07" => Some("sv7"),
        "sv08" => Some("sv8"),
        "sv08.5" => Some("sv8pt5"),
        "svp" => Some("svp"),
        _ => None,
    };
    if let Some(code) = elsewhere {
        return format!("https://images.pokemontcg.io/{code}/symbol.png");
    }
    // TCGdex keeps a few newer symbols in its folder for every language.
    if set == "me05" {
        return "https://assets.tcgdex.net/univ/me/me05/symbol.png".into();
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

/// A card's fields after its first one, tab-separated: what a `C` line and an `M` line share.
fn card_fields(c: &Card) -> String {
    let binders = c.binders.iter().map(u16::to_string).collect::<Vec<_>>().join(",");
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
        clean(&c.handle), clean(&c.title), clean(&c.variant), clean(&c.set), clean(&c.no), c.price, clean(&c.img), clean(&c.grade), c.qty, binders, clean(&c.tags), clean(&c.sym), clean(&c.finish), clean(&c.lang), clean(&c.notes)
    )
}

pub fn card_line(c: &Card) -> String {
    format!("C\t{}", card_fields(c))
}

/// A store on The Vault page's file. `at` is its place in the file's store list plus one: what its cards' `M` lines say.
pub fn store_line(at: u16, s: &Store) -> String {
    format!("S\t{at}\t{}\t{}\t{}\t{}\n", s.id, clean(&s.slug), clean(&s.title), clean(&s.img))
}

/// One card a store has for sale: its store (`Card::store`), its listing number, then the same fields as a vault card.
pub fn market_line(c: &Card) -> String {
    format!("M\t{}\t{}\t{}", c.store, c.lid, card_fields(c))
}

/// A card from the fields after a line's first ones. Older files have no notes at the end.
fn read_card(fields: &[&str], order: u32) -> Option<Card> {
    let [handle, title, variant, set, no, price, img, grade, qty, bs, tags, sym, finish, lang, extra @ ..] = fields else { return None };
    if extra.len() > 1 {
        return None;
    }
    let notes = extra.first().copied().unwrap_or("");
    // The finish is searchable too ("1st edition", "reverse holo"), but not "Normal", which nearly every card is.
    let fin = match *finish {
        "Normal" => "",
        "1st Edition" => "1st Edition first",
        f => f,
    };
    let text = norm(&format!("{title} {variant} {set} {no} {grade} {tags} {notes} {fin}"));
    Some(Card {
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
        order,
        store: 0,
        lid: 0,
    })
}

/// Read the catalogue file. It's plain tab-separated text, which keeps the browser download small and quick to read.
/// Only the vault's binders (`B`) and cards (`C`): any other line, like a store's (`S`, `M`), is left out.
pub fn parse(data: &str) -> (Vec<Binder>, Vec<Card>) {
    let (mut binders, mut cards) = (vec![], vec![]);
    for line in data.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.as_slice() {
            ["B", handle, title, img, count] => binders.push(Binder { handle: handle.to_string(), title: title.to_string(), img: img.to_string(), count: count.parse().unwrap_or(0) }),
            ["C", rest @ ..] => {
                if let Some(c) = read_card(rest, cards.len() as u32) {
                    cards.push(c);
                }
            }
            _ => {}
        }
    }
    (binders, cards)
}

/// Read The Vault page's file (`/vault.tsv`): the vault's binders and cards as `parse` reads them, then the stores
/// (`S` lines) and every card they have for sale (`M` lines). The stores' cards come after the vault's in the list, in
/// the file's order (newest first). Typing a store's name finds its cards when searched with `search_market`, which is
/// given the stores; the name is not in the cards' own text. The default order mixes them in among the vault's (see
/// `mix_in_stores`).
pub fn parse_market(data: &str) -> (Vec<Binder>, Vec<Store>, Vec<Card>) {
    let (binders, mut cards) = parse(data);
    let mut stores: Vec<Store> = vec![];
    // What the file numbers each store, and its place in `stores` plus one.
    let mut index: Vec<(u16, u16)> = vec![];
    for line in data.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.as_slice() {
            ["S", at, id, slug, title, img] => {
                let (Ok(at), Ok(id)) = (at.parse::<u16>(), id.parse::<i64>()) else { continue };
                if at == 0 || index.iter().any(|x| x.0 == at) || stores.len() >= usize::from(u16::MAX - 1) {
                    continue;
                }
                stores.push(Store { id, slug: slug.to_string(), title: title.to_string(), img: img.to_string(), count: 0 });
                index.push((at, stores.len() as u16));
            }
            ["M", at, lid, rest @ ..] => {
                let Some(store) = at.parse::<u16>().ok().and_then(|at| index.iter().find(|x| x.0 == at)).map(|x| x.1) else { continue };
                let Ok(lid) = lid.parse::<i64>() else { continue };
                if let Some(mut c) = read_card(rest, cards.len() as u32) {
                    c.store = store;
                    c.lid = lid;
                    stores[usize::from(store) - 1].count += 1;
                    cards.push(c);
                }
            }
            _ => {}
        }
    }
    (binders, stores, cards)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny(title: &str, variant: &str, finish: &str, lang: &str, price: f64) -> Card {
        Card { notes: String::new(), sym: String::new(), finish: finish.into(), lang: lang.into(), handle: norm(title), title: title.into(), variant: variant.into(), set: String::new(), no: String::new(), price, img: String::new(), grade: String::new(), qty: 1, binders: vec![], tags: String::new(), text: norm(title), order: 0, store: 0, lid: 0 }
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
        assert_eq!(set_symbol("tcgdex:base1-4"), "https://images.pokemontcg.io/base1/symbol.png");
        assert_eq!(set_symbol("tcgdex:sm3.5-9"), "https://images.pokemontcg.io/sm35/symbol.png");
        assert_eq!(set_symbol("tcgdex:me05-001"), "https://assets.tcgdex.net/univ/me/me05/symbol.png");
        assert_eq!(set_symbol("tcgdex:base2-1"), "https://assets.tcgdex.net/en/base/base2/symbol.png");
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
            data += &card_line(&Card { notes: String::new(), sym: String::new(), finish: String::new(), lang: String::new(), handle: norm(t).replace(' ', "-"), title: t.into(), variant: String::new(), set: "Test Set".into(), no: "7".into(), price: p, img: "/x.jpg".into(), grade: g.into(), qty: 1, binders: b.split(',').filter_map(|x| x.parse().ok()).collect(), tags: String::new(), text: String::new(), order: 0, store: 0, lid: 0 });
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
    fn the_finish_can_be_searched() {
        let mut data = String::new();
        for (t, v, fin) in [("Graveler - 037/62 - Fossil", "Lightly Played · 1st Ed.", "1st Edition"), ("Zorua - 1/9", "Near Mint · Reverse Holo", "Reverse Holo"), ("Eevee - 2/9", "Near Mint", "Normal")] {
            data += &card_line(&Card { title: t.into(), variant: v.into(), finish: fin.into(), handle: norm(t).replace(' ', "-"), price: 10.0, qty: 1, ..Default::default() });
        }
        let d = parse(&data).1;
        let titles = |q: &str| search(&d, &read_words(q), Sort::Newest).hits.iter().map(|&i| d[i].title.clone()).collect::<Vec<_>>();
        assert_eq!(titles("1st edition"), vec!["Graveler - 037/62 - Fossil"]);
        assert_eq!(titles("first edition graveler"), vec!["Graveler - 037/62 - Fossil"]);
        assert_eq!(titles("reverse holo"), vec!["Zorua - 1/9"]);
        assert!(titles("normal").is_empty(), "almost every card is plain, so the word finds nothing extra");
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
        let c = Card { sym: String::new(), finish: String::new(), lang: String::new(), notes: "LP-NM".into(), handle: "a".into(), title: "A\tB".into(), variant: "x\ny".into(), set: String::new(), no: String::new(), price: 12.5, img: String::new(), grade: String::new(), qty: 3, binders: vec![2, 5], tags: String::new(), text: String::new(), order: 0, store: 0, lid: 0 };
        let b = Binder { handle: "h".into(), title: "T".into(), img: "/i".into(), count: 4 };
        let (bs, cs) = parse(&(binder_line(&b) + &card_line(&c) + &card_line(&c)));
        assert_eq!(bs, vec![b]);
        assert_eq!((cs[0].title.as_str(), cs[0].qty, cs[0].binders.clone(), cs[1].order), ("A B", 3, vec![2, 5], 1));
    }

    /// The vault's file plus two stores: Ash's Shop with a priced Pikachu and an unpriced Mew, Misty's with a Pikachu.
    fn market() -> String {
        let mut data = binder_line(&Binder { handle: "h".into(), title: "T".into(), img: String::new(), count: 1 });
        data += &card_line(&Card { handle: "pikachu-vault-7".into(), title: "Pikachu - 1/9".into(), variant: "Near Mint".into(), price: 300.0, qty: 1, binders: vec![0], ..Default::default() });
        data += &card_line(&Card { handle: "eevee-8".into(), title: "Eevee - 2/9".into(), price: 50.0, qty: 0, ..Default::default() });
        data += &store_line(1, &Store { id: 41, slug: "ash-shop".into(), title: "Ash's\tShop".into(), img: "/a.jpg".into(), count: 99 });
        data += &store_line(2, &Store { id: 42, slug: String::new(), title: "Cerulean Gym".into(), img: String::new(), count: 0 });
        for (store, lid, title, price) in [(1, 501, "Pikachu - 1/9", 120.0), (2, 502, "Pikachu - 1/9", 900.0), (1, 503, "Mew - 3/9", 0.0)] {
            data += &market_line(&Card { store, lid, title: title.into(), variant: "Lightly Played".into(), price, qty: 2, notes: "line one\nline two".into(), ..Default::default() });
        }
        data
    }

    #[test]
    fn the_vault_page_file_adds_the_stores_after_the_vault() {
        let data = market();
        let (bs, stores, cards) = parse_market(&data);
        assert_eq!(bs.len(), 1);
        assert_eq!(stores.iter().map(|s| (s.id, s.title.as_str(), s.count)).collect::<Vec<_>>(), vec![(41, "Ash's Shop", 2), (42, "Cerulean Gym", 1)], "tabs cleaned, cards counted when read");
        assert_eq!((stores[0].key(), stores[1].key()), ("ash-shop".to_string(), "42".to_string()), "a store with no address goes by its number");
        let who: Vec<(u16, i64, u32)> = cards.iter().map(|c| (c.store, c.lid, c.order)).collect();
        assert_eq!(who, vec![(0, 0, 0), (0, 0, 1), (1, 501, 2), (2, 502, 3), (1, 503, 4)], "the vault's cards first, then the stores' in the file's order");
        assert_eq!((cards[2].qty, cards[2].price, cards[2].notes.as_str(), cards[2].variant.as_str()), (2, 120.0, "line one line two", "Lightly Played"));
        // The catalogue's own reader (Ask the Vault, /api/search, the AI tools) still sees only the vault.
        let (bs2, vault) = parse(&data);
        assert_eq!((bs2.len(), vault.len()), (1, 2));
        assert!(vault.iter().all(|c| c.store == 0 && c.lid == 0));
        assert_eq!(vault, cards[..2].to_vec(), "the vault's cards read the same either way");
        // A card line written by store_line/market_line survives another round.
        let again: String = stores.iter().enumerate().map(|(i, s)| store_line(i as u16 + 1, s)).chain(cards[2..].iter().map(market_line)).collect();
        let (_, s3, c3) = parse_market(&again);
        assert_eq!((s3.len(), c3.len(), c3[0].lid, c3[0].store, c3[1].store), (2, 3, 501, 1, 2));
        // A card from a store the file never named is left out.
        assert_eq!(parse_market("M\t9\t1\th\tT\t\t\t\t1\t\t\t1\t\t\t\t\t\t\n").2.len(), 0);
    }

    #[test]
    fn the_default_order_puts_a_store_card_after_every_three_of_the_vaults() {
        let mut data = String::new();
        for i in 0..7 {
            data += &card_line(&Card { handle: format!("v-{i}"), title: format!("v{i}"), price: 10.0 + f64::from(i), qty: 1, ..Default::default() });
        }
        data += &store_line(1, &Store { id: 9, slug: "shop".into(), title: "Shop".into(), ..Default::default() });
        for i in 0..3 {
            data += &market_line(&Card { store: 1, lid: 100 + i64::from(i), title: format!("s{i}"), price: 500.0 - 100.0 * f64::from(i), qty: 1, ..Default::default() });
        }
        let (_, stores, cards) = parse_market(&data);
        let run = |f: &Filter, sort: Sort| search_market(&cards, &stores, f, sort).hits.iter().map(|&i| cards[i].title.clone()).collect::<Vec<_>>().join(" ");
        assert_eq!(run(&Filter::default(), Sort::Newest), "v0 v1 v2 s0 v3 v4 v5 s1 v6 s2", "the vault's order, a store card (newest first) after every three");
        assert_eq!(run(&Filter { min_price: Some(13.0), ..Default::default() }, Sort::Newest), "v3 v4 v5 s0 v6 s1 s2", "the same rhythm in what a filter leaves; store cards left over come last");
        assert_eq!(run(&Filter { store: Some(1), ..Default::default() }, Sort::Newest), "s0 s1 s2");
        assert_eq!(run(&Filter { store: Some(0), ..Default::default() }, Sort::Newest), "v0 v1 v2 v3 v4 v5 v6", "the vault alone keeps its own order");
        // Only the default order mixes: the other orders and a typed search are as they were.
        assert_eq!(run(&Filter::default(), Sort::PriceAsc), "v0 v1 v2 v3 v4 v5 v6 s2 s1 s0");
        assert_eq!(run(&Filter::default(), Sort::PriceDesc), "s0 s1 s2 v6 v5 v4 v3 v2 v1 v0");
        assert_eq!(run(&Filter::default(), Sort::Name), "s0 s1 s2 v0 v1 v2 v3 v4 v5 v6");
        assert_eq!(run(&read_words("shop"), Sort::Newest), "s0 s1 s2", "a store's name finds its cards, by relevance");
        // The catalogue alone (/api/search, Ask the Vault) has no store cards, so its order is unchanged.
        assert_eq!(parse(&data).1.iter().map(|c| c.title.as_str()).collect::<Vec<_>>().join(" "), "v0 v1 v2 v3 v4 v5 v6");
    }

    #[test]
    fn a_store_is_found_by_its_name_and_unpriced_cards_go_last() {
        let (_, stores, cards) = parse_market(&market());
        let titles = |f: &Filter, sort: Sort| search_market(&cards, &stores, f, sort).hits.iter().map(|&i| (cards[i].store, cards[i].lid)).collect::<Vec<_>>();
        assert_eq!(titles(&read_words("cerulean"), Sort::Newest), vec![(2, 502)], "typing a store's name finds its cards");
        assert_eq!(titles(&read_words("ash shop"), Sort::Newest), vec![(1, 501), (1, 503)]);
        assert_eq!(titles(&Filter { store: Some(0), ..Default::default() }, Sort::Newest).len(), 2, "only the vault's");
        assert_eq!(titles(&Filter { store: Some(1), ..Default::default() }, Sort::Newest), vec![(1, 501), (1, 503)]);
        let asc = titles(&Filter::default(), Sort::PriceAsc);
        assert_eq!(asc.last(), Some(&(1, 503)), "no price comes last, not first: {asc:?}");
        assert_eq!(titles(&Filter::default(), Sort::PriceDesc).last(), Some(&(1, 503)));
        let under = titles(&Filter { max_price: Some(500.0), ..Default::default() }, Sort::Newest);
        assert!(!under.contains(&(1, 503)), "a card with no price isn't under any price: {under:?}");
        assert_eq!(titles(&Filter { binder: Some(0), ..Default::default() }, Sort::Newest), vec![(0, 0)], "a binder still holds only vault cards");
    }

    /// The vault's Pikachu, Charizard, Eevee and Dratini; "Pikachu Corner PH" sells a Charizard, an Umbreon and a
    /// Pikachu; Cerulean Gym (no address) a Charizard and a Mew with no price. Each card as "store:Name".
    fn corner() -> (Vec<Store>, Vec<Card>) {
        let mut data = String::new();
        for (i, t) in ["Pikachu - 25/102", "Charizard - 4/102", "Eevee - 51/64", "Dratini - 26/64"].into_iter().enumerate() {
            data += &card_line(&Card { handle: format!("v-{i}"), title: t.into(), price: 100.0 + 10.0 * i as f64, qty: 1, ..Default::default() });
        }
        data += &store_line(1, &Store { id: 7, slug: "pikachu-corner".into(), title: "Pikachu Corner PH".into(), ..Default::default() });
        data += &store_line(2, &Store { id: 8, slug: String::new(), title: "Cerulean Gym".into(), ..Default::default() });
        for (store, lid, t, price) in [(1, 11, "Charizard ex - 199/165", 900.0), (1, 12, "Umbreon VMAX - 215/203", 5000.0), (1, 13, "Pikachu V - 43/185", 50.0), (2, 21, "Charizard - 4/102", 2000.0), (2, 22, "Mew - 8/9", 0.0)] {
            data += &market_line(&Card { store, lid, title: t.into(), price, qty: 1, ..Default::default() });
        }
        let (_, stores, cards) = parse_market(&data);
        (stores, cards)
    }

    #[test]
    fn a_pokemon_in_a_stores_name_finds_only_that_pokemon() {
        let (stores, cards) = corner();
        let name = |i: usize| format!("{}:{}", cards[i].store, cards[i].title.split(' ').next().unwrap_or(""));
        let run = |f: &Filter, sort: Sort| search_market(&cards, &stores, f, sort).hits.iter().map(|&i| name(i)).collect::<Vec<_>>().join(" ");
        let find = |q: &str, sort: Sort| run(&read_words(q), sort);
        assert!(cards.iter().all(|c| !c.text.contains("corner") && !c.text.contains("cerulean") && !c.text.contains(" ph")), "a store's name isn't in its cards' text");
        // A word some card has must be on the card itself, in every order: the store's Charizard and Umbreon stay out.
        assert_eq!(find("pikachu", Sort::Newest), "0:Pikachu 1:Pikachu", "the card that is just Pikachu first");
        assert_eq!(find("pikachu", Sort::PriceAsc), "1:Pikachu 0:Pikachu");
        assert_eq!(find("pikachu", Sort::PriceDesc), "0:Pikachu 1:Pikachu");
        assert_eq!(find("pikachu", Sort::Name), "0:Pikachu 1:Pikachu");
        assert_eq!(find("cheapest pikachu", Sort::Newest), "1:Pikachu 0:Pikachu");
        assert_eq!(find("pika", Sort::PriceAsc), "1:Pikachu 0:Pikachu", "the start of a word too");
        // A word no card has, but a store's name or address does, finds that store's cards.
        assert_eq!(find("cerulean", Sort::Newest), "2:Charizard 2:Mew");
        assert_eq!(find("cerulean gym", Sort::PriceAsc), "2:Charizard 2:Mew", "a card with no price still comes last");
        assert_eq!(find("corner", Sort::Newest), "1:Charizard 1:Umbreon 1:Pikachu");
        assert_eq!(find("ph", Sort::Newest), "1:Charizard 1:Umbreon 1:Pikachu");
        // Both kinds together: the card's words on the card, from the stores the other words name.
        assert_eq!(find("charizard cerulean", Sort::Newest), "2:Charizard");
        assert_eq!(find("cerulean charizard", Sort::PriceDesc), "2:Charizard");
        assert_eq!(find("charizard corner", Sort::Newest), "1:Charizard");
        assert_eq!(find("pikachu corner", Sort::Newest), "1:Pikachu", "Pikachu cards from the store called Pikachu Corner");
        assert_eq!(find("umbreon cerulean", Sort::Newest), "", "Cerulean has no Umbreon");
        assert_eq!(find("charizard under 1000", Sort::PriceAsc), "0:Charizard 1:Charizard");
        assert_eq!(find("charizard", Sort::Name).split(' ').count(), 3, "every Charizard, whoever sells it");
        // With a seller picked, a store's word still only narrows: a different store's name finds nothing.
        assert_eq!(run(&Filter { store: Some(1), ..read_words("pikachu") }, Sort::Newest), "1:Pikachu");
        assert_eq!(run(&Filter { store: Some(1), ..read_words("cerulean") }, Sort::Newest), "");
        assert_eq!(run(&Filter { store: Some(0), ..read_words("charizard") }, Sort::Newest), "0:Charizard");
        // Near spellings: of a card's words, and of a store's name, only when nothing matched exactly.
        let typo = |q: &str| search_market(&cards, &stores, &read_words(q), Sort::Newest).typo;
        assert_eq!(find("cerulaen", Sort::Newest), "2:Charizard 2:Mew");
        assert!(typo("cerulaen") && !typo("cerulean"));
        assert_eq!(find("charizrd cerulean", Sort::Newest), "2:Charizard");
        assert!(typo("charizrd cerulean"));
        assert_eq!(find("zzzzzzzz", Sort::Newest), "");
        // Without the stores (the server's search of the vault's own catalogue) no word is a store's.
        assert!(search(&cards, &read_words("cerulean"), Sort::Newest).hits.is_empty());
        assert_eq!(search(&cards, &read_words("pikachu"), Sort::PriceAsc).hits.len(), 2);
    }

    #[test]
    fn the_default_order_is_the_same_with_store_names_left_out() {
        let (stores, cards) = corner();
        let name = |i: usize| format!("{}:{}", cards[i].store, cards[i].title.split(' ').next().unwrap_or(""));
        let run = |f: &Filter, sort: Sort| search_market(&cards, &stores, f, sort).hits.iter().map(|&i| name(i)).collect::<Vec<_>>().join(" ");
        let every = "0:Pikachu 0:Charizard 0:Eevee 1:Charizard 0:Dratini 1:Umbreon 1:Pikachu 2:Charizard 2:Mew";
        assert_eq!(run(&Filter::default(), Sort::Newest), every, "a store card after every three of the vault's, the rest last");
        assert_eq!(search(&cards, &Filter::default(), Sort::Newest).hits, search_market(&cards, &stores, &Filter::default(), Sort::Newest).hits, "no words, no difference");
        assert_eq!(run(&read_words(""), Sort::Newest), every);
        assert_eq!(run(&Filter { store: Some(1), ..Default::default() }, Sort::Newest), "1:Charizard 1:Umbreon 1:Pikachu");
        assert_eq!(run(&Filter::default(), Sort::PriceAsc), "1:Pikachu 0:Pikachu 0:Charizard 0:Eevee 0:Dratini 1:Charizard 2:Charizard 1:Umbreon 2:Mew");
        // The plain catalogue reader is as it was: the vault's cards, read the same.
        let (_, vault) = parse(&cards.iter().filter(|c| c.store == 0).map(card_line).collect::<String>());
        assert_eq!(vault.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), cards[..4].iter().map(|c| c.text.as_str()).collect::<Vec<_>>());
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
