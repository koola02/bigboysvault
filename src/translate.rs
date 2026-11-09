//! Card page text in English, Japanese and Chinese, on every device.
//!
//! General machine translation gets this vocabulary badly wrong (it turned "クレイバースト" into "Cancelled" and "Near Mint" into
//! "near peppermint"), so this uses official name lists instead: Pokémon and item names from PokeAPI (`market::name_table`),
//! plus the hand-checked tables below. Anything not in a list, such as an English set name with no official Japanese name, stays as written.
use crate::market::Names;

pub const LANGS: [&str; 3] = ["en", "ja", "zh"];

/// (English, Japanese, Chinese). Chinese is Traditional, the form used on the Chinese cards in the collection.
const WORDS: &[(&str, &str, &str)] = &[
    ("Near Mint", "ニアミント", "近全新"),
    ("Lightly Played", "軽度の使用感あり", "輕微使用"),
    ("Moderately Played", "中程度の使用感あり", "中度使用"),
    ("Heavily Played", "強い使用感あり", "重度使用"),
    ("Damaged", "ダメージあり", "損傷"),
    ("Normal", "ノーマル", "普通"),
    ("Holo", "ホロ", "閃卡"),
    ("Reverse Holo", "リバースホロ", "反閃"),
    ("Promo", "プロモ", "特典卡"),
    ("Holofoil", "ホロ", "閃卡"),
    ("Illustration rare", "イラストレア", "插畫稀有"),
    ("Special illustration rare", "スペシャルイラストレア", "特別插畫稀有"),
    ("Illustration Rare", "イラストレア", "插畫稀有"),
    ("Full Art", "フルアート", "全圖"),
    ("Full Art Trainer", "フルアートトレーナー", "全圖訓練家"),
    ("Double rare", "ダブルレア", "雙重稀有"),
    ("Ultra Rare", "ウルトラレア", "超稀有"),
    ("Hyper rare", "ハイパーレア", "超級稀有"),
    ("Rare Holo", "レアホロ", "稀有閃卡"),
    ("Rare", "レア", "稀有"),
    ("Uncommon", "アンコモン", "不常見"),
    ("Common", "コモン", "常見"),
    ("English", "英語版", "英文版"),
    ("Japanese", "日本語版", "日文版"),
    ("Chinese", "中国語版", "中文版"),
    ("Graded", "鑑定済み", "鑑定卡"),
];

/// Interface words on the page: English, Japanese, Chinese. `{n}` is a number.
const LABELS: &[(&str, &str, &str, &str)] = &[
    ("market", "Market value as of", "市場価格の更新日:", "市場價格更新於"),
    ("copies", "×{n} in the vault", "金庫に ×{n}", "金庫內 ×{n}"),
    ("more", "More like this", "関連カード", "相關卡片"),
    ("versions", "Versions in the vault", "金庫にあるバージョン", "金庫內的版本"),
    ("condition", "Condition", "状態", "狀態"),
    ("finish", "Finish", "仕上げ", "卡面"),
    ("set", "Set", "セット", "系列"),
    ("rarity", "Rarity", "レアリティ", "稀有度"),
    ("notes", "Notes", "メモ", "備註"),
    ("card", "Card", "カード", "卡片"),
];

/// Words that go in front of a name.
const PREFIXES: &[(&str, &str, &str)] = &[
    ("Mega ", "メガ", "超級"),
    ("Alolan ", "アローラ", "阿羅拉"),
    ("Galarian ", "ガラル", "伽勒爾"),
    ("Hisuian ", "ヒスイ", "洗翠"),
    ("Paldean ", "パルデア", "帕底亞"),
    ("Radiant ", "かがやく", "光輝"),
    ("Team Rocket's ", "ロケット団の", "火箭隊的"),
    ("Iono's ", "ナンジャモの", "奇樹的"),
    ("Ethan's ", "ヒビキの", "阿響的"),
    ("Cynthia's ", "シロナの", "竹蘭的"),
    ("Lillie's ", "リーリエの", "莉莉艾的"),
    ("Misty's ", "カスミの", "小霞的"),
    ("Steven's ", "ダイゴの", "大吾的"),
    ("Hop's ", "ホップの", "赫普的"),
    ("Marnie's ", "マリィの", "瑪俐的"),
    ("Arven's ", "ペパーの", "派帕的"),
    ("N's ", "Nの", "N的"),
];

/// Japanese and Chinese set names with the English names collectors use for them.
const SETS: &[(&str, &str)] = &[
    ("スカーレットex", "Scarlet ex"),
    ("バイオレットex", "Violet ex"),
    ("トリプレットビート", "Triplet Beat"),
    ("スノーハザード", "Snow Hazard"),
    ("クレイバースト", "Clay Burst"),
    ("ポケモンカード151", "Pokémon Card 151"),
    ("黒炎の支配者", "Ruler of the Black Flame"),
    ("レイジングサーフ", "Raging Surf"),
    ("古代の咆哮", "Ancient Roar"),
    ("未来の一閃", "Future Flash"),
    ("シャイニートレジャーex", "Shiny Treasure ex"),
    ("ワイルドフォース", "Wild Force"),
    ("サイバージャッジ", "Cyber Judge"),
    ("クリムゾンヘイズ", "Crimson Haze"),
    ("変幻の仮面", "Mask of Change"),
    ("ナイトワンダラー", "Night Wanderer"),
    ("ステラミラクル", "Stellar Miracle"),
    ("楽園ドラゴーナ", "Paradise Dragona"),
    ("超電ブレイカー", "Super Electric Breaker"),
    ("テラスタルフェスex", "Terastal Festival ex"),
    ("バトルパートナーズ", "Battle Partners"),
    ("熱風のアリーナ", "Heat Wave Arena"),
    ("ロケット団の栄光", "Glory of Team Rocket"),
    ("ブラックボルト", "Black Bolt"),
    ("ホワイトフレア", "White Flare"),
    ("VMAXクライマックス", "VMAX Climax"),
    ("VSTARユニバース", "VSTAR Universe"),
    ("白熱のアルカナ", "Incandescent Arcana"),
    ("25週年收藏款", "25th Anniversary Collection"),
];

fn pick<'a>(row: (&'a str, &'a str, &'a str), to: &str) -> &'a str {
    match to {
        "ja" => row.1,
        "zh" => row.2,
        _ => row.0,
    }
}

fn is_cjk(s: &str) -> bool {
    s.chars().any(|c| matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{9fff}'))
}

/// The language a card's text is written in.
pub fn language_of(title: &str, api_id: &str, tags: &str) -> &'static str {
    let t = tags.to_lowercase();
    if api_id.starts_with("tcgdex-ja") || t.contains("japanese") || title.chars().any(|c| matches!(c, '\u{3040}'..='\u{30ff}')) {
        "ja"
    } else if api_id.starts_with("tcgdex-zh") || t.contains("chinese") || is_cjk(title) {
        "zh"
    } else {
        "en"
    }
}

/// A word from the tables, recognised in any of the three languages. Unknown text comes back unchanged.
pub fn word(text: &str, to: &str) -> String {
    let k = text.trim();
    let flat = k.replace('-', " ").to_lowercase();
    WORDS.iter().find(|r| r.0.to_lowercase() == flat || r.1 == k || r.2 == k).map(|r| pick(*r, to).to_string()).unwrap_or_else(|| k.to_string())
}

/// "near-mint / normal / promo": each piece on its own.
pub fn variant(text: &str, to: &str) -> String {
    text.split(" / ").map(|p| word(p, to)).collect::<Vec<_>>().join(" / ")
}

pub fn set_name(text: &str, to: &str) -> String {
    let k = text.trim();
    let found = SETS.iter().find(|(native, en)| *native == k || en.eq_ignore_ascii_case(k));
    match (found, to) {
        (Some((_, en)), "en") => en.to_string(),
        // A Japanese set for a Chinese reader: the English name is the one they can look up.
        (Some((native, en)), "zh") if !native.ends_with('款') => en.to_string(),
        (Some((native, _)), _) => native.to_string(),
        (None, _) => k.to_string(),
    }
}

fn latin_boundary(c: Option<char>) -> bool {
    c.is_none_or(|c| !c.is_alphanumeric())
}

/// Replace known Pokémon and item names inside `text`, longest first, so "Mewtwo" isn't read as "Mew".
fn names_in(text: &str, to: &str, names: &Names) -> String {
    #[derive(Clone)]
    struct Hit {
        at: usize,
        len: usize,
        out: String,
    }
    let lower = text.to_lowercase();
    let same_len = lower.len() == text.len();
    let mut hits: Vec<Hit> = vec![];
    for r in names {
        let out = match to {
            "ja" => r.ja.first().unwrap_or(&r.en),
            "zh" => r.zh.first().unwrap_or(&r.en),
            _ => &r.en,
        };
        if same_len && r.en_lower.chars().count() >= 3 {
            for (at, _) in lower.match_indices(r.en_lower.as_str()) {
                let before = lower[..at].chars().next_back();
                let after = lower[at + r.en_lower.len()..].chars().next();
                if latin_boundary(before) && latin_boundary(after) {
                    hits.push(Hit { at, len: r.en_lower.len(), out: out.clone() });
                }
            }
        }
        for n in r.ja.iter().chain(r.zh.iter()) {
            if n.chars().count() >= 2 {
                for (at, _) in text.match_indices(n.as_str()) {
                    hits.push(Hit { at, len: n.len(), out: out.clone() });
                }
            }
        }
    }
    hits.sort_by(|a, b| b.len.cmp(&a.len).then(a.at.cmp(&b.at)));
    let mut kept: Vec<Hit> = vec![];
    for h in hits {
        if kept.iter().all(|k| h.at + h.len <= k.at || k.at + k.len <= h.at) {
            kept.push(h);
        }
    }
    kept.sort_by_key(|h| h.at);
    let (mut out, mut pos) = (String::new(), 0);
    for h in kept {
        out.push_str(&text[pos..h.at]);
        out.push_str(&h.out);
        pos = h.at + h.len;
    }
    out.push_str(&text[pos..]);
    out
}

/// "ピカチュウ (Pikachu)" means the same thing twice: the bracket is only a label added for English readers.
fn drop_label(name: &str) -> String {
    if let (Some(a), true) = (name.rfind(" ("), name.ends_with(')')) {
        let inner = &name[a + 2..name.len() - 1];
        if is_cjk(&name[..a]) && !is_cjk(inner) {
            return name[..a].to_string();
        }
    }
    name.to_string()
}

pub fn card_name(name: &str, to: &str, names: &Names) -> String {
    let mut s = drop_label(name.trim());
    // Prefix words: to a placeholder first, so they can't interfere with the name lookup.
    let mut front = String::new();
    for p in PREFIXES {
        for from in [p.0, p.1, p.2] {
            if let Some(i) = s.find(from) {
                let (before, after) = (s[..i].to_string(), s[i + from.len()..].to_string());
                front = pick(*p, to).to_string();
                s = format!("{before}\u{1}{after}");
                break;
            }
        }
    }
    let done = names_in(&s, to, names);
    // Latin and Japanese or Chinese read better without a gap after the prefix, and with one before "ex" only in English.
    let joined = done.replace('\u{1}', &front);
    if to == "en" {
        // "カビゴンex" -> "Snorlax ex"
        let t = joined.trim_end();
        for tail in ["VMAX", "VSTAR", "ex", "EX", "GX", "V"] {
            if let Some(head) = t.strip_suffix(tail) {
                if head.ends_with(|c: char| c.is_ascii_lowercase()) && !head.is_empty() && tail != "V" {
                    return format!("{head} {tail}");
                }
            }
        }
    }
    joined
}

/// A whole title: "Name - 017/71 - Set". Numbers stay, and each other piece uses its own list.
pub fn title(text: &str, to: &str, names: &Names) -> String {
    let parts: Vec<&str> = text.split(" - ").collect();
    let mut out = vec![card_name(parts[0], to, names)];
    for (i, p) in parts.iter().enumerate().skip(1) {
        out.push(if i >= 2 || !p.chars().any(|c| c.is_ascii_digit()) { set_name(p, to) } else { p.trim().to_string() });
    }
    out.join(" - ")
}

/// Standard lines used in imported notes. Free-form notes stay as written.
pub fn note_line(line: &str, to: &str) -> String {
    const LINES: &[(&str, &str, &str)] = &[("Set:", "セット:", "系列:"), ("Card number:", "カード番号:", "卡片編號:"), ("Rarity:", "レアリティ:", "稀有度:"), ("Finish:", "仕上げ:", "工藝:"), ("Language:", "言語:", "語言:")];
    let l = line.trim();
    for row in LINES {
        if let Some(rest) = l.strip_prefix(row.0).or_else(|| l.strip_prefix(row.1)).or_else(|| l.strip_prefix(row.2)) {
            let rest = rest.trim();
            let val = word(rest, to);
            return format!("{} {}", pick(*row, to), if val == rest { set_name(rest, to) } else { val });
        }
    }
    const WHOLE: (&str, &str, &str) = ("Pick your condition from the options. Prices in Philippine pesos.", "状態は選択肢から選んでください。価格はフィリピン・ペソです。", "請從選項中選擇卡況。價格以菲律賓披索計。");
    if l == WHOLE.0 || l == WHOLE.1 || l == WHOLE.2 {
        return pick(WHOLE, to).to_string();
    }
    line.to_string()
}

pub fn labels(to: &str) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for (key, en, ja, zh) in LABELS {
        m.insert((*key).to_string(), serde_json::Value::String(pick((en, ja, zh), to).to_string()));
    }
    serde_json::Value::Object(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::NameRow;

    fn row(en: &str, ja: &str, zh: &str) -> NameRow {
        NameRow { en: en.into(), en_lower: en.to_lowercase(), ja: vec![ja.into()], zh: vec![zh.into()] }
    }
    fn table() -> Names {
        vec![row("Pikachu", "ピカチュウ", "皮卡丘"), row("Mew", "ミュウ", "夢幻"), row("Mewtwo", "ミュウツー", "超夢"), row("Snorlax", "カビゴン", "卡比獸"), row("Bellibolt", "ハラバリー", "電肚蛙"), row("Floette", "フラエッテ", "花葉蒂")]
    }

    #[test]
    fn titles_in_all_three_languages() {
        let n = table();
        let jp = "ピカチュウ (Pikachu) - 017/71 - クレイバースト";
        assert_eq!(title(jp, "en", &n), "Pikachu - 017/71 - Clay Burst");
        assert_eq!(title(jp, "ja", &n), "ピカチュウ - 017/71 - クレイバースト");
        assert_eq!(title(jp, "zh", &n), "皮卡丘 - 017/71 - Clay Burst");
        let zh = "皮卡丘 (Pikachu) - 001/28 - 25週年收藏款";
        assert_eq!(title(zh, "en", &n), "Pikachu - 001/28 - 25th Anniversary Collection");
        assert_eq!(title(zh, "ja", &n), "ピカチュウ - 001/28 - 25週年收藏款");
        assert_eq!(title(zh, "zh", &n), "皮卡丘 - 001/28 - 25週年收藏款");
        let en = "Mega Floette ex - 035/86 - Chaos Rising";
        assert_eq!(title(en, "ja", &n), "メガフラエッテ ex - 035/86 - Chaos Rising");
        assert_eq!(title(en, "zh", &n), "超級花葉蒂 ex - 035/86 - Chaos Rising");
        assert_eq!(title(en, "en", &n), en);
    }

    #[test]
    fn longest_name_wins_and_words_stay_whole() {
        let n = table();
        assert_eq!(card_name("Mewtwo", "ja", &n), "ミュウツー");
        assert_eq!(card_name("ミュウツー", "en", &n), "Mewtwo");
        assert_eq!(card_name("Mew", "zh", &n), "夢幻");
        assert_eq!(card_name("Mewling Sleeves", "ja", &n), "Mewling Sleeves");
    }

    #[test]
    fn names_with_prefixes_and_endings() {
        let n = table();
        assert_eq!(card_name("カビゴンex", "en", &n), "Snorlax ex");
        assert_eq!(card_name("ナンジャモのハラバリー", "en", &n), "Iono's Bellibolt");
        assert_eq!(card_name("Iono's Bellibolt ex", "zh", &n), "奇樹的電肚蛙 ex");
        assert_eq!(card_name("メガフラエッテex", "en", &n), "Mega Floette ex");
        assert_eq!(card_name("PSA 10 Pikachu", "ja", &n), "PSA 10 ピカチュウ");
    }

    #[test]
    fn conditions_tags_notes_and_language() {
        assert_eq!(word("Near Mint", "zh"), "近全新");
        assert_eq!(word("ニアミント", "en"), "Near Mint");
        assert_eq!(variant("near-mint / normal / promo", "ja"), "ニアミント / ノーマル / プロモ");
        assert_eq!(word("Something Else", "ja"), "Something Else");
        assert_eq!(note_line("Rarity: Promo", "zh"), "稀有度: 特典卡");
        assert_eq!(note_line("Set: Clay Burst", "ja"), "セット: クレイバースト");
        assert_eq!(note_line("A free-form note.", "ja"), "A free-form note.");
        assert_eq!(language_of("ピカチュウ", "", ""), "ja");
        assert_eq!(language_of("黒炎", "tcgdex-ja:SV3-1", ""), "ja");
        assert_eq!(language_of("皮卡丘", "", ""), "zh");
        assert_eq!(language_of("Pikachu", "", ""), "en");
    }
}
