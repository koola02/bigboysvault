//! "Ask the Vault": a guide that answers plain questions from the real catalogue, in the visitor's words, with no outside service.
//! It works out what's being asked, filters the cards with the same search the site uses, and can solve "what can I get for ₱1,000"
//! as a knapsack problem: the set of cards that uses the budget best.
use vault_search::{amount, cond_class, peso, read_tokens, read_words, search, Binder, Card, Sort, Token};

#[derive(Clone, Debug)]
pub struct Trainer {
    pub name: String,
    pub stars: i64,
    pub worth: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Answer {
    pub say: String,
    pub picks: Vec<usize>,
    pub chips: Vec<String>,
    pub link: Option<(String, String)>,
    pub ctx: Option<String>,
}

const START: [&str; 5] = ["Show me Near Mint Pikachu", "What can I get for ₱1,000?", "What's your most expensive card?", "Cheapest holo cards", "Who are the top trainers?"];

fn start_chips() -> Vec<String> {
    START.iter().map(|s| (*s).to_string()).collect()
}

fn has(text: &str, any: &[&str]) -> bool {
    let words: Vec<&str> = text.split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty()).collect();
    any.iter().any(|p| if p.contains(' ') { text.contains(p) } else { words.contains(p) })
}

fn preprocess_text(t: &str) -> String {
    let mut reps: Vec<(&str, &str)> = vec![
        ("meron ba kayong", "do you have"),
        ("meron ba kayo", "do you have"),
        ("meron kayong", "do you have"),
        ("meron ba", "do you have"),
        ("meron", "do you have"),
        ("magkano", "how much"),
        ("pinakamura", "cheapest"),
        ("pinakamahal", "most expensive"),
        ("mura", "cheap"),
        ("mahal", "expensive"),
        ("1st ed", "1st edition"),
        ("first ed", "first edition"),
        ("jp", "japanese"),
        ("japan", "japanese"),
        ("cheaper", "cheapest"),
        ("pa", "more"),
    ];
    reps.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    let is_word_char = |c: char| c.is_alphanumeric() || c == '\'';
    let mut result = String::new();
    let chars: Vec<char> = t.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let mut replaced = false;
        for (old, new) in &reps {
            let old_c: Vec<char> = old.chars().collect();
            let ol = old_c.len();
            if i + ol <= chars.len() && &chars[i..i + ol] == old_c.as_slice() {
                let before_ok = i == 0 || !is_word_char(chars[i - 1]);
                let after_ok = i + ol == chars.len() || !is_word_char(chars[i + ol]);
                if before_ok && after_ok {
                    result.push_str(new);
                    i += ol;
                    replaced = true;
                    break;
                }
            }
        }
        if !replaced {
            result.push(chars[i]);
            i += 1;
        }
    }
    let mut s = result;
    s = s.replace(" lang ", " only ").replace(" na ", " only ");
    s
}

/// Words that say what kind of question it is, not what to look for.
const FILLER: [&str; 64] = [
    "many",
    "do", "does", "did", "you", "your", "yours", "have", "has", "got", "get", "got", "any", "in", "stock", "looking", "look", "please", "can", "could", "would", "what", "whats", "what's", "which",
    "is", "are", "there", "how", "much", "cost", "costs", "price", "prices", "sell", "buy", "need", "my", "hi", "hello", "hey", "see", "list", "give", "from", "on", "at",
    "may", "meron", "ba", "kayo", "kayong", "magkano", "mura", "pinakamura", "mahal", "pinakamahal", "lang", "pa", "piso", "pesos", "php", "ko", "na",
];

/// Words that are never part of a set's name in a question: the question's own words and card types.
const SET_SKIP: [&str; 17] = ["cards", "card", "show", "me", "from", "in", "the", "pokemon", "pokémon", "ex", "gx", "v", "vmax", "vstar", "played", "all", "a"];

/// The words left once conditions, finishes and languages are taken out: "near mint base set" is "base set".
fn set_phrase(words: &str) -> String {
    read_tokens(words, &|_| false)
        .into_iter()
        .filter_map(|t| if let Token::Name(n) = t { Some(n.to_lowercase()) } else { None })
        .collect::<Vec<_>>()
        .join(" ")
}

/// True when the set's name holds the phrase as whole words: "holo" is not in "Holon Phantoms".
fn set_has(set: &str, phrase: &str) -> bool {
    let words = set.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    format!(" {words} ").contains(&format!(" {phrase} "))
}

/// True when some card's own name (not its set) has every word of the phrase.
fn names_a_card(cards: &[&Card], phrase: &str) -> bool {
    let want: Vec<&str> = phrase.split(' ').filter(|w| !w.is_empty() && !matches!(*w, "set" | "sets")).collect();
    !want.is_empty()
        && cards.iter().any(|c| {
            let head = vault_search::norm(&vault_search::title_and_set(&c.title, &c.set).0);
            want.iter().all(|w| head.split(' ').any(|t| t == *w))
        })
}

/// The cards that use a budget best. Prices are rounded up to a unit, so the result can never go over. Ties go to more cards.
pub fn best_for_budget(prices: &[f64], budget: f64) -> Vec<usize> {
    let unit = (budget / 20_000.0).max(10.0);
    let cap = (budget / unit).floor() as usize;
    let items: Vec<(usize, usize)> = prices.iter().enumerate().filter(|(_, p)| **p > 0.0 && **p <= budget).map(|(i, p)| (i, (p / unit).ceil() as usize)).filter(|(_, w)| *w <= cap).take(600).collect();
    if items.is_empty() || cap == 0 {
        return vec![];
    }
    let mut best = vec![-1i32; cap + 1];
    best[0] = 0;
    let words = cap / 64 + 1;
    let mut took = vec![0u64; items.len() * words];
    for (k, (_, w)) in items.iter().enumerate() {
        for b in (*w..=cap).rev() {
            if best[b - *w] >= 0 && best[b - *w] + 1 > best[b] {
                best[b] = best[b - *w] + 1;
                took[k * words + b / 64] |= 1 << (b % 64);
            }
        }
    }
    let Some(mut b) = (0..=cap).rev().find(|b| best[*b] > 0) else { return vec![] };
    let mut out = vec![];
    for k in (0..items.len()).rev() {
        if took[k * words + b / 64] >> (b % 64) & 1 == 1 {
            out.push(items[k].0);
            b -= items[k].1;
        }
    }
    out.reverse();
    out
}

fn budget_in(text: &str) -> Option<f64> {
    if !has(text, &["budget", "spend", "afford", "i have", "for", "with", "got"]) {
        return None;
    }
    text.split_whitespace().find_map(|w| {
        let w = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '₱' && c != '$');
        amount(w).filter(|v| *v >= 10.0 && *v < 100_000_000.0 && (w.starts_with('₱') || !w.chars().any(|c| c.is_alphabetic()) || w.ends_with('k')))
    })
}

fn listing(cards: &[&Card], hits: &[usize], typo: bool, what: &str) -> Answer {
    let shown: Vec<usize> = hits.iter().copied().take(8).collect();
    let copies: u32 = hits.iter().map(|i| cards[*i].qty.max(1)).sum();
    let prices: Vec<f64> = hits.iter().map(|i| cards[*i].price).filter(|p| *p > 0.0).collect();
    let (lo, hi) = prices.iter().fold((f64::INFINITY, 0.0f64), |(a, b), p| (a.min(*p), b.max(*p)));
    let span = if prices.is_empty() { String::new() } else if (hi - lo).abs() < 1.0 { format!(", all at {}", peso(lo)) } else { format!(", from {} to {}", peso(lo), peso(hi)) };
    let say = if typo {
        format!("I couldn't find that exact name. These are the closest spellings: {copies} card{}{span}.", if copies == 1 { "" } else { "s" })
    } else if copies == 1 {
        format!("I found 1 {what}{span}.")
    } else {
        format!("I found {copies} {what}{span}.{}", if hits.len() > shown.len() { " Here are the first 8." } else { "" })
    };
    Answer { say, picks: shown, ..Default::default() }
}

pub fn answer(question: &str, prev_ctx: Option<&str>, cards: &[Card], binders: &[Binder], trainers: &[Trainer]) -> Answer {
    // A card with every copy sold is only a record now: the guide talks about what can be bought.
    let keep: Vec<usize> = (0..cards.len()).filter(|&i| cards[i].qty > 0).collect();
    let pool: Vec<&Card> = keep.iter().map(|&i| &cards[i]).collect();
    let mut a = answer_in(question, prev_ctx, &pool, binders, trainers);
    a.picks = a.picks.into_iter().map(|k| keep[k]).collect();
    a
}

fn answer_in(question: &str, prev_ctx: Option<&str>, cards: &[&Card], binders: &[Binder], trainers: &[Trainer]) -> Answer {
    let text = question.trim().to_lowercase().replace('?', " ").replace(['“', '”'], "\"");
    let text = text.trim().to_string();
    let text = preprocess_text(&text);
    if text.is_empty() || has(&text, &["help", "hi", "hello", "hey"]) && text.split_whitespace().count() <= 3 || text.contains("what can you") {
        return Answer { say: "I know every card in the vault. Ask me for a card, a condition, a price or a budget, or ask how something here works.".into(), chips: start_chips(), ..Default::default() };
    }

    // How the site works.
    if has(&text, &["vouch", "vouches"]) && has(&text, &["how", "where", "write", "post", "leave"]) {
        return Answer { say: "Sign in as a trainer, then tap the Vouch button on any page. Your words and photos go up straight away.".into(), link: Some(("Read the vouches".into(), "/vouches".into())), ..Default::default() };
    }
    if has(&text, &["join", "sign up", "signup", "account", "become", "register"]) {
        return Answer { say: "Pick a trainer name and a password. There's no email to give. You get a page and a binder for your favorite cards.".into(), link: Some(("Become a trainer".into(), "/join".into())), ..Default::default() };
    }
    if has(&text, &["chat", "talk", "message"]) {
        return Answer { say: "The chat is open to everyone to read. Sign in as a trainer to join in.".into(), link: Some(("Open the chat".into(), "/chat".into())), ..Default::default() };
    }
    if text.contains("what does") || text.contains("what is") || text.contains("mean") || text.contains("difference") {
        let meaning = [
            (&["near mint", "nm"][..], "Near Mint means the card looks almost new, with at most tiny flaws."),
            (&["lightly played", "lp"][..], "Lightly Played means small wear you can spot on a close look, like light edge marks."),
            (&["moderately played", "mp"][..], "Moderately Played means clear wear, such as scuffs or small creases, but still solid in a sleeve."),
            (&["heavily played", "hp"][..], "Heavily Played means heavy wear, with creases or marks you can't miss."),
            (&["damaged"][..], "Damaged means a major flaw, like a tear, water damage or a big crease."),
            (&["graded", "psa", "cgc", "bgs", "slab"][..], "Graded means a company has inspected the card, scored it out of 10 and sealed it in a case."),
            (&["reverse holo", "reverse"][..], "Reverse Holo means the shine is on the card's body, with the picture itself left plain."),
            (&["holo", "holofoil"][..], "Holofoil means the picture has a shiny, rainbow foil."),
        ];
        if let Some((_, say)) = meaning.iter().find(|(words, _)| has(&text, words)) {
            return Answer { say: (*say).into(), chips: start_chips(), ..Default::default() };
        }
    }

// The vault itself.
    let total: u32 = cards.iter().map(|c| c.qty.max(1)).sum();
    let value: f64 = cards.iter().map(|c| c.price * f64::from(c.qty.max(1))).sum();
    if has(&text, &["trainer", "trainers", "collector", "collectors", "members"]) {
        if trainers.is_empty() {
            return Answer { say: "No trainers yet. You could be the first.".into(), link: Some(("Become a trainer".into(), "/join".into())), ..Default::default() };
        }
        let mut by_stars = trainers.to_vec();
        by_stars.sort_by(|a, b| b.stars.cmp(&a.stars));
        let mut by_worth = trainers.to_vec();
        by_worth.sort_by(|a, b| b.worth.total_cmp(&a.worth));
        let stars = by_stars.iter().take(3).map(|t| format!("{} (★ {})", t.name, t.stars)).collect::<Vec<_>>().join(", ");
        let richest = by_worth.first().filter(|t| t.worth > 0.0).map(|t| format!(" The most valuable binder is {}'s, at {}.", t.name, peso(t.worth))).unwrap_or_default();
        return Answer { say: format!("There are {} trainers. Most stars: {stars}.{richest}", trainers.len()), link: Some(("See all trainers".into(), "/trainers".into())), ..Default::default() };
    }
    if has(&text, &["binder", "binders", "collections"]) {
        let names = binders.iter().map(|b| format!("{} ({})", b.title, b.count)).collect::<Vec<_>>().join(", ");
        return Answer { say: format!("There are {} binders: {names}.", binders.len()), ..Default::default() };
    }
    let only_about_the_vault = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !FILLER.contains(w))
        .all(|w| matches!(w, "many" | "card" | "cards" | "vault" | "total" | "collectibles" | "items" | "altogether" | "the" | "all" | "i" | "we"));
    if text.contains("how many") && only_about_the_vault {
        return Answer { say: format!("The vault holds {total} cards and collectibles in {} binders.", binders.len()), chips: start_chips(), ..Default::default() };
    }
    if has(&text, &["worth", "value", "total"]) && has(&text, &["vault", "collection", "total", "everything", "all"]) {
        return Answer { say: format!("Everything in the vault adds up to {}, across {total} cards.", peso(value)), ..Default::default() };
    }

    // Set questions ("how many base set cards", "what do you have from 151"): a count per set, and some of its cards.
    // A set name only counts as whole words. Words about the card itself (holo, near mint, ex, vmax) are never a set,
    // and unless the question really names a set, a card name wins: "how many pikachu" is about Pikachu, not Detective Pikachu.
    if has(&text, &["how many", "what do you have", "show me", "from", "in", "set", "sets"]) {
        let all: Vec<String> = text.split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '₱' && c != '$').to_lowercase())
            .filter(|w| !w.is_empty())
            .collect();
        // After "from" or "in", the rest is the set: "pikachu from detective pikachu".
        let after = all.iter().rposition(|w| w == "from" || w == "in").map_or(0, |k| k + 1);
        let named = after > 0 || all.iter().any(|w| w == "set" || w == "sets");
        let picked: Vec<&str> = all[after..].iter().map(String::as_str).filter(|w| !FILLER.contains(w) && !SET_SKIP.contains(w)).collect();
        let phrase = set_phrase(&picked.join(" "));
        if !phrase.is_empty() && (named || !names_a_card(cards, &phrase)) {
            let short: String = phrase.split(' ').filter(|w| !matches!(*w, "set" | "sets")).collect::<Vec<_>>().join(" ");
            for sw in [phrase.clone(), short] {
                if sw.is_empty() { continue; }
                let hits: Vec<usize> = (0..cards.len()).filter(|&i| set_has(&cards[i].set, &sw)).collect();
                if hits.is_empty() { continue; }
                let mut groups: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
                for &i in &hits {
                    *groups.entry(cards[i].set.clone()).or_default() += cards[i].qty.max(1);
                }
                let mut set_list: Vec<(String, u32)> = groups.into_iter().collect();
                set_list.sort_by(|(sa, _), (sb, _)| {
                    let a_ex = sa.to_lowercase() == sw;
                    let b_ex = sb.to_lowercase() == sw;
                    b_ex.cmp(&a_ex).then_with(|| sa.cmp(sb))
                });
                let parts: Vec<String> = set_list.iter().map(|(s, c)| format!("{}: {}", s, c)).collect();
                let say = parts.join(", ");
                let shown: Vec<usize> = hits.iter().copied().take(8).collect();
                return Answer { say, picks: shown, ctx: Some(sw), ..Default::default() };
            }
        }
    }

    // Follow-up context (no server state): combine prev effective search if this looks like a short refiner
    // (e.g. "cheaper ones", "under 500", "only holo", "near mint lang", "more", "japanese", "1st edition").
    // Any question naming a fresh card/Pokémon starts over.
    let effective = if let Some(p) = prev_ctx.filter(|p| !p.trim().is_empty()) {
        let q_words: Vec<String> = text.split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '₱' && c != '$').to_string())
            .filter(|w| !w.is_empty() && !FILLER.contains(&w.as_str()))
            .collect();
        let refiners: &[&str] = &["cheapest", "cheap", "expensive", "most", "more", "only", "near", "mint", "holo", "under", "over", "less", "budget", "japanese", "chinese", "english", "1st", "first", "edition", "unlimited", "shadowless", "may", "how", "much", "piso", "pesos", "php", "ones"];
        let has_new = q_words.iter().any(|w| {
            !refiners.contains(&w.as_str()) && amount(w).is_none() && !w.parse::<f64>().is_ok() && !w.chars().all(|c| c.is_numeric() || c == '.' || c == 'k' || c == '₱' || c == ',')
        });
        if !has_new {
            format!("{} {}", p, text)
        } else {
            text.clone()
        }
    } else {
        text.clone()
    };

    // Strip pure follow-up filler from effective query (but keep conds/finishes/langs that become tokens and price words).
    let effective: String = effective.split_whitespace().filter(|&w| {
        let c = w.trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '₱' && ch != '$');
        !matches!(c, "ones" | "pa" | "lang" | "na" | "ko" | "ba" | "kayo" | "may" | "how" | "much" | "only")
    }).collect::<Vec<_>>().join(" ");

    // The search itself.
    let budget = budget_in(&effective);
    let all_words: Vec<String> = effective
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '₱' && c != '$').to_string())
        .filter(|w| !w.is_empty() && !FILLER.contains(&w.as_str()))
        .filter(|w| budget.is_none() || !(matches!(w.as_str(), "budget" | "spend" | "afford" | "for" | "with" | "i") || amount(w).is_some()))
        .collect();
    let mut search_words = vec![];
    let mut sort = Sort::Newest;
    let mut i = 0;
    while i < all_words.len() {
        if i + 1 < all_words.len() && all_words[i] == "most" && all_words[i + 1] == "expensive" {
            sort = Sort::PriceDesc;
            i += 2;
            if i < all_words.len() && (all_words[i] == "card" || all_words[i] == "cards") {
                i += 1;
            }
            continue;
        }
        if all_words[i] == "cheapest" || all_words[i] == "cheap" {
            sort = Sort::PriceAsc;
            i += 1;
            if i < all_words.len() && (all_words[i] == "card" || all_words[i] == "cards") {
                i += 1;
            }
            continue;
        }
        if all_words[i] == "expensive" {
            sort = Sort::PriceDesc;
            i += 1;
            if i < all_words.len() && (all_words[i] == "card" || all_words[i] == "cards") {
                i += 1;
            }
            continue;
        }
        search_words.push(all_words[i].clone());
        i += 1;
    }
    let joined = search_words.join(" ");
    let mut f = read_words(&joined);
    if f.sort.is_some() {
        sort = f.sort.unwrap();
    }
    if effective.contains("best") && f.sort.is_none() {
        sort = Sort::PriceDesc;
    }
    if matches!(sort, Sort::PriceAsc | Sort::PriceDesc) {
        f.sort = Some(sort);
    }

    // Conditions, finishes and languages come out of the words; whatever's left stays a search word.
    let tokens: Vec<Token> = read_tokens(&f.words.join(" "), &|_| false).into_iter().filter(|t| !matches!(t, Token::Name(_))).collect();
    if !tokens.is_empty() {
        let names: Vec<String> = read_tokens(&f.words.join(" "), &|_| false).into_iter().filter_map(|t| if let Token::Name(n) = t { Some(n.to_lowercase()) } else { None }).collect();
        f.words = names;
        f.apply_tokens(&tokens);
    }
    let what = {
        let mut bits: Vec<String> = tokens.iter().map(|t| {
            if let Token::Lang(l) = t {
                match l.as_str() {
                    "ja" | "japanese" | "jp" | "日本語" => "Japanese".to_string(),
                    "zh" | "chinese" | "中文" => "Chinese".to_string(),
                    _ => l.clone(),
                }
            } else {
                Token::label(t)
            }
        }).collect();
        bits.extend(f.words.iter().map(|w| { let mut c = w.chars(); c.next().map(|x| x.to_uppercase().chain(c).collect::<String>()).unwrap_or_default() }));
        bits.push("cards".into());
        bits.join(" ")
    };

    let new_ctx = if f.words.is_empty() && tokens.is_empty() && f.min_price.is_none() && f.max_price.is_none() && !f.graded && f.sort.is_none() {
        None
    } else {
        let mut parts = f.words.clone();
        for t in &tokens {
            match t {
                Token::Cond(c) => parts.push(c.clone()),
                Token::Finish(fi) => parts.push(fi.clone()),
                Token::Lang(l) => parts.push(match l.as_str() {
                    "ja" => "japanese".to_string(),
                    "zh" => "chinese".to_string(),
                    "en" => "english".to_string(),
                    _ => l.clone(),
                }),
                Token::Name(n) => parts.push(n.clone()),
            }
        }
        if parts.is_empty() { None } else { Some(parts.join(" ")) }
    };

// The search itself (budget uses effective for follow-ups like "under 500").
    if let Some(money) = budget {
        f.max_price = Some(money);
        let found = search(cards, &f, Sort::PriceAsc);
        let prices: Vec<f64> = found.hits.iter().map(|i| cards[*i].price).collect();
        let chosen: Vec<usize> = best_for_budget(&prices, money).into_iter().map(|k| found.hits[k]).collect();
        if chosen.is_empty() {
            return Answer { say: format!("Nothing in the vault fits {}{}.", peso(money), if f.words.is_empty() { String::new() } else { format!(" for {}", f.words.join(" ")) }), chips: start_chips(), ctx: new_ctx, ..Default::default() };
        }
        let spent: f64 = chosen.iter().map(|i| cards[*i].price).sum();
        let mut picks = chosen.clone();
        picks.sort_by(|a, b| cards[*b].price.total_cmp(&cards[*a].price));
        let extra = if picks.len() > 12 { format!(" Here are the 12 biggest of them.") } else { String::new() };
        picks.truncate(12);
        return Answer { say: format!("For {} you could take {} card{}, totalling {}, which leaves {}.{extra}", peso(money), chosen.len(), if chosen.len() == 1 { "" } else { "s" }, peso(spent), peso(money - spent)), picks, chips: vec![format!("Holo cards for {}", peso(money)), format!("Near Mint for {}", peso(money))], ctx: new_ctx, ..Default::default() };
    }

    if f.words.is_empty() && tokens.is_empty() && f.min_price.is_none() && f.max_price.is_none() && !f.graded && f.sort.is_none() && !effective.contains("best") {
        return Answer { say: "I'm not sure what you're after. Try a Pokémon's name, a condition or a price.".into(), chips: start_chips(), ctx: None, ..Default::default() };
    }
    let found = search(cards, &f, sort);
    if found.hits.is_empty() {
        return Answer { say: format!("Nothing in the vault matches {what} right now."), chips: start_chips(), ctx: None, ..Default::default() };
    }
    let mut a = listing(cards, &found.hits, found.typo, &what);
    if let Some(first) = found.hits.first().filter(|_| matches!(f.sort, Some(Sort::PriceAsc) | Some(Sort::PriceDesc)) && found.hits.len() > 1) {
        let c = &cards[*first];
        let top = format!("The {} is {} at {}.", if f.sort == Some(Sort::PriceAsc) { "cheapest" } else { "most expensive" }, vault_search::title_and_set(&c.title, &c.set).0, peso(c.price));
        a.say = if f.words.is_empty() && tokens.is_empty() { format!("{top} Here are the next ones too.") } else { format!("{top} {}", a.say) };
    }
    let base = question.trim().trim_end_matches('?').to_string();
    let mut chips = vec![];
    if !tokens.iter().any(|t| matches!(t, Token::Cond(_))) && found.hits.iter().any(|i| cond_class(&cards[*i].variant) == "nm") {
        chips.push(format!("{base} near mint"));
    }
    if f.sort.is_none() {
        chips.push(format!("Cheapest {base}"));
    }
    a.chips = chips;
    a.ctx = new_ctx;
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(title: &str, price: f64, cond: &str) -> Card {
        let text = vault_search::norm(&format!("{title} {cond}"));
        Card { handle: title.to_lowercase(), title: title.into(), variant: cond.into(), price, qty: 1, text, ..Default::default() }
    }
    fn deck() -> Vec<Card> {
        vec![card("Pikachu", 500.0, "Near Mint"), card("Mew", 300.0, "Lightly Played"), card("Mewtwo", 900.0, "Near Mint"), card("Eevee", 100.0, "Near Mint")]
    }

    #[test]
    fn budgets_are_solved_exactly_and_never_go_over() {
        assert_eq!(best_for_budget(&[500.0, 300.0, 900.0, 100.0], 900.0), vec![0, 1, 3]);
        assert_eq!(best_for_budget(&[500.0, 600.0], 400.0), Vec::<usize>::new());
        let spent: f64 = best_for_budget(&[333.0, 333.0, 333.0], 1000.0).iter().map(|_| 333.0).sum();
        assert!(spent <= 1000.0);
    }

// Tiny card maker for tests (sets finish/lang so printings/languages/set tests work).
    fn make_card(title: &str, price: f64, cond: &str, set: &str, finish: &str, lang: &str) -> Card {
        let text = vault_search::norm(&format!("{title} {cond} {set} {finish}"));
        Card {
            handle: title.to_lowercase().replace(' ', "-"),
            title: title.into(),
            variant: cond.into(),
            set: set.into(),
            no: "".into(),
            price,
            img: "".into(),
            grade: "".into(),
            qty: 1,
            binders: vec![],
            tags: "".into(),
            sym: "".into(),
            finish: finish.into(),
            lang: lang.into(),
            notes: "".into(),
            text,
            order: 0,
        }
    }

    #[test]
    fn it_understands_plain_questions() {
        let d = deck();
        let a = answer("do you have any pikachu?", None, &d, &[], &[]);
        assert_eq!(a.picks, vec![0], "{a:?}");
        let a = answer("show me near mint cards", None, &d, &[], &[]);
        assert_eq!(a.picks.len(), 3, "{a:?}");
        let a = answer("what's your most expensive card?", None, &d, &[], &[]);
        assert_eq!(a.picks[0], 2, "{a:?}");
        assert!(a.say.contains("Mewtwo"), "{a:?}");
        let a = answer("what can I get for ₱900", None, &d, &[], &[]);
        assert_eq!(a.picks.len(), 3, "{a:?}");
        assert!(a.say.contains("₱900"), "{a:?}");
        let a = answer("how many cards do you have?", None, &d, &[], &[]);
        assert!(a.say.starts_with("The vault holds 4"), "{a:?}");
        let a = answer("how many pikachu do you have", None, &d, &[], &[]);
        assert_eq!(a.picks, vec![0], "{a:?}");
        let a = answer("what does near mint mean", None, &d, &[], &[]);
        assert!(a.say.starts_with("Near Mint means"), "{a:?}");
        let a = answer("zzzzqq", None, &d, &[], &[]);
        assert!(a.picks.is_empty(), "{a:?}");
        let a = answer("who are the top trainers", None, &d, &[], &[Trainer { name: "Hay".into(), stars: 5, worth: 100.0 }]);
        assert!(a.say.contains("Hay"), "{a:?}");
    }

    #[test]
    fn followups_combine_with_context() {
        let d = vec![
            make_card("Pikachu", 500.0, "Near Mint", "", "", ""),
            make_card("Pikachu", 400.0, "Lightly Played", "", "", ""),
            make_card("Charizard", 900.0, "Near Mint", "", "", ""),
        ];
        let a1 = answer("pikachu", None, &d, &[], &[]);
        assert_eq!(a1.picks, vec![0, 1]);
        let ctx = a1.ctx.as_deref().unwrap_or("pikachu");
        let a2 = answer("cheaper ones", Some(ctx), &d, &[], &[]);
        assert_eq!(a2.picks[0], 1, "{a2:?}");
        let a3 = answer("charizard", Some(ctx), &d, &[], &[]);
        assert_eq!(a3.picks[0], 2);
    }

    #[test]
    fn taglish_and_printings() {
        let d = vec![
            make_card("Pikachu", 500.0, "Near Mint", "", "", ""),
            make_card("Charizard (1st Edition)", 1200.0, "Near Mint", "", "1st Edition", ""),
            make_card("Mew", 300.0, "Near Mint", "", "Holofoil", "ja"),
        ];
        let a = answer("magkano pikachu", None, &d, &[], &[]);
        assert_eq!(a.picks, vec![0]);
        let a = answer("meron ba kayong charizard", None, &d, &[], &[]);
        assert!(a.picks.contains(&1), "{a:?}");
        let a = answer("pinakamura na holo", None, &d, &[], &[]);
        assert_eq!(a.picks, vec![2]);
        let a = answer("1st edition charizard", None, &d, &[], &[]);
        assert_eq!(a.picks, vec![1], "{a:?}");
        let a = answer("japanese mew", None, &d, &[], &[]);
        assert_eq!(a.picks, vec![2]);
    }

    #[test]
    fn set_count_question() {
        let d = vec![
            make_card("Pikachu", 100.0, "", "Base Set", "", ""),
            make_card("Bulbasaur", 150.0, "", "Base Set", "", ""),
            make_card("Mew", 300.0, "", "151", "", ""),
        ];
        let a = answer("how many base set cards do you have", None, &d, &[], &[]);
        assert!(a.say.contains("2"), "{a:?}");
        assert!(a.say.contains("Base Set"), "{a:?}");
        assert_eq!(a.picks.len(), 2);
        let a = answer("what do you have from 151", None, &d, &[], &[]);
        assert!(!a.picks.is_empty());
    }

    #[test]
    fn card_words_are_not_set_names() {
        let d = vec![
            make_card("Pikachu", 100.0, "Near Mint", "Jungle", "", ""),
            make_card("Detective Pikachu", 150.0, "Near Mint", "Detective Pikachu", "", ""),
            make_card("Mew", 300.0, "Near Mint", "Holon Phantoms", "Holofoil", ""),
            make_card("Charizard ex", 900.0, "Near Mint", "Obsidian Flames", "Holofoil", ""),
            make_card("Zekrom", 50.0, "Near Mint", "Dark Explorers", "", ""),
            make_card("Snorlax", 20.0, "Near Mint", "Base Set", "", ""),
        ];
        let a = answer("Show me Near Mint Pikachu", None, &d, &[], &[]);
        assert!(a.picks.contains(&0) && a.picks.contains(&1), "every Pikachu, not just the Detective Pikachu set: {a:?}");
        let a = answer("how many pikachu do you have", None, &d, &[], &[]);
        assert!(a.picks.contains(&0), "{a:?}");
        let a = answer("show me holo cards", None, &d, &[], &[]);
        assert!(!a.say.contains("Holon Phantoms:"), "{a:?}");
        assert!(a.picks.contains(&3), "{a:?}");
        let a = answer("show me charizard ex", None, &d, &[], &[]);
        assert_eq!(a.picks, vec![3], "{a:?}");
        let a = answer("how many base set cards do you have", None, &d, &[], &[]);
        assert!(a.say.starts_with("Base Set: 1"), "{a:?}");
        let a = answer("show me holon phantoms", None, &d, &[], &[]);
        assert!(a.say.starts_with("Holon Phantoms: 1"), "a set name nobody's card has still finds the set: {a:?}");
        let a = answer("do you have pikachu from detective pikachu", None, &d, &[], &[]);
        assert!(a.say.starts_with("Detective Pikachu: 1"), "{a:?}");
    }

    #[test]
    fn sold_cards_are_not_offered() {
        let mut d = vec![make_card("Pikachu", 100.0, "Near Mint", "", "", ""), make_card("Pikachu", 200.0, "Near Mint", "", "", ""), make_card("Mew", 50.0, "Near Mint", "", "", "")];
        d[0].qty = 0;
        let a = answer("pikachu", None, &d, &[], &[]);
        assert_eq!(a.picks, vec![1], "the sold one is left out, and the pick still points at the right card: {a:?}");
        let a = answer("how many cards do you have?", None, &d, &[], &[]);
        assert!(a.say.starts_with("The vault holds 2"), "{a:?}");
    }

    #[test]
    fn fix1_language_english_in_answer() {
        let d = vec![
            make_card("Mew", 300.0, "Near Mint", "", "", "ja"),
        ];
        let a = answer("japanese cards", None, &d, &[], &[]);
        assert!(a.say.contains("Japanese"), "{a:?}");
        assert!(!a.say.contains("日本語"), "{a:?}");
    }

    #[test]
    fn fix2_order_followups_keep_ctx() {
        let d = vec![
            make_card("Pikachu", 500.0, "Near Mint", "", "", "ja"),
            make_card("Pikachu", 100.0, "Near Mint", "", "", "ja"),
            make_card("Charizard", 900.0, "Near Mint", "", "", ""),
        ];
        let a1 = answer("japanese pikachu", None, &d, &[], &[]);
        let ctx = a1.ctx.as_ref().unwrap();
        let a2 = answer("most expensive", Some(ctx), &d, &[], &[]);
        assert_eq!(a2.picks[0], 0, "{a2:?}");
        let a3 = answer("pinakamahal", Some(ctx), &d, &[], &[]);
        assert_eq!(a3.picks[0], 0, "{a3:?}");
        let a4 = answer("charizard", Some(ctx), &d, &[], &[]);
        assert_eq!(a4.picks[0], 2, "{a4:?}");
    }

    #[test]
    fn fix3_sets_all_matching_exact_first() {
        let d = vec![
            make_card("X", 10.0, "", "A Base Set", "", ""),
            make_card("Pikachu", 100.0, "", "Base Set", "", ""),
            make_card("Mew", 300.0, "", "Base Set (Japanese)", "", ""),
        ];
        let a = answer("how many base set cards do you have", None, &d, &[], &[]);
        assert!(a.say.contains("Base Set: 1"), "{a:?}");
        assert!(a.say.contains("A Base Set: 1"), "{a:?}");
        assert!(a.say.contains("Base Set (Japanese): 1"), "{a:?}");
        assert!(a.say.starts_with("Base Set: 1, A Base Set: 1, Base Set (Japanese): 1"), "exact first even if alpha would differ: {a:?}");
    }
}
