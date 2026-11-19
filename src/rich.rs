//! Formatted words in a post (the owner, 19 Nov 2026: "give the option to bold, change font size in 3 levels of any
//! text within a post"): bold, italic, underline and three sizes, on any part of the words.
//!
//! A post's words are kept twice. Plain, in `posts.text` (or `note`), for everything that reads them: notices,
//! previews, sharing, search. And, when any of them is formatted, as runs in `posts.rich` (JSON): the same words cut
//! into pieces, each with its own style. Nothing a person typed is ever stored or sent as HTML. The page is drawn
//! from the runs, every piece escaped, so only the few tags written in this file can ever appear.
use serde::{Deserialize, Serialize};

/// More pieces than anyone formats by hand. Past this the words are kept plain.
pub const RUNS_MAX: usize = 400;

/// One piece of a post's words and how it looks.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct Run {
    #[serde(default)]
    pub t: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub b: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub i: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub u: bool,
    /// "" is the normal size, "s" small, "l" large.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub s: String,
}

/// Bold, italic, underlined, and the size (0 normal, 1 small, 2 large).
type Style = (bool, bool, bool, u8);

impl Run {
    fn style(&self) -> Style {
        (self.b, self.i, self.u, match self.s.as_str() { "s" => 1, "l" => 2, _ => 0 })
    }
}

fn run_of(t: String, st: Style) -> Run {
    Run { t, b: st.0, i: st.1, u: st.2, s: match st.3 { 1 => "s", 2 => "l", _ => "" }.to_string() }
}

/// Tidies what the editor sent (runs, as JSON). Gives back the plain words, exactly as `members::clean_text` would
/// keep them (lines trimmed, spaces squeezed, one blank line at most, `max` letters), and the runs for those same
/// words as JSON: "" when nothing in them is formatted. `None` when `raw` isn't runs at all.
pub fn clean(raw: &str, max: usize) -> Option<(String, String)> {
    let runs: Vec<Run> = serde_json::from_str(raw).ok()?;
    // Every letter with its style, cut into lines. Far more than `max` is never looked at.
    let room = max.saturating_mul(4) + 1000;
    let mut lines: Vec<Vec<(char, Style)>> = vec![vec![]];
    let mut seen = 0usize;
    'all: for r in &runs {
        let st = r.style();
        for ch in r.t.chars() {
            seen += 1;
            if seen > room {
                break 'all;
            }
            // A line ends at "\n". The "\r" some keyboards send before it is a space like any other, and goes with the
            // rest of the line's end.
            match ch {
                '\n' => lines.push(vec![]),
                _ => lines.last_mut().expect("there is always a line").push((ch, st)),
            }
        }
    }
    // The same tidying as `members::clean_text`, letter by letter so each keeps its style.
    let mut out: Vec<(char, Style)> = vec![];
    let mut blank = 0;
    for line in lines {
        let mut l: Vec<(char, Style)> = vec![];
        let mut words = 0;
        let mut in_word = false;
        // The style of the first space after a word: the one space kept between two words wears it, so an underline
        // or a size ends where its writer ended it.
        let mut gap: Option<Style> = None;
        for (ch, st) in line {
            if ch.is_whitespace() {
                if in_word {
                    gap = Some(st);
                }
                in_word = false;
                continue;
            }
            if !in_word {
                if words > 0 {
                    l.push((' ', gap.unwrap_or(st)));
                }
                words += 1;
                in_word = true;
            }
            if !ch.is_control() {
                l.push((ch, st));
            }
        }
        if l.is_empty() {
            blank += 1;
            continue;
        }
        if let Some(last) = out.last().map(|x| x.1) {
            out.push(('\n', last));
            if blank > 0 {
                out.push(('\n', last));
            }
        }
        out.extend(l);
        blank = 0;
    }
    out.truncate(max);
    while out.last().is_some_and(|x| x.0.is_whitespace()) {
        out.pop();
    }
    let text: String = out.iter().map(|x| x.0).collect();
    let mut tidy: Vec<Run> = vec![];
    for (ch, st) in out {
        match tidy.last_mut() {
            Some(r) if r.style() == st => r.t.push(ch),
            _ => tidy.push(run_of(ch.to_string(), st)),
        }
    }
    let plain = tidy.iter().all(|r| r.style() == (false, false, false, 0));
    let rich = if plain || tidy.len() > RUNS_MAX { String::new() } else { serde_json::to_string(&tidy).unwrap_or_default() };
    Some((text, rich))
}

fn escape(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
}

/// The words as HTML for the page, when `rich` holds runs that spell exactly `text` (the plain words shown
/// otherwise). "" when there are none, or they are about other words: then the page shows the plain ones.
pub fn html_for(rich: &str, text: &str) -> String {
    if rich.is_empty() {
        return String::new();
    }
    let Ok(runs) = serde_json::from_str::<Vec<Run>>(rich) else { return String::new() };
    if runs.is_empty() || runs.len() > RUNS_MAX || runs.iter().map(|r| r.t.as_str()).collect::<String>() != text {
        return String::new();
    }
    let mut out = String::with_capacity(text.len() + runs.len() * 24);
    for r in &runs {
        let (b, i, u, size) = r.style();
        if size != 0 {
            out.push_str(if size == 1 { "<span class=\"rt-s\">" } else { "<span class=\"rt-l\">" });
        }
        if b {
            out.push_str("<b>");
        }
        if i {
            out.push_str("<i>");
        }
        if u {
            out.push_str("<u>");
        }
        escape(&r.t, &mut out);
        if u {
            out.push_str("</u>");
        }
        if i {
            out.push_str("</i>");
        }
        if b {
            out.push_str("</b>");
        }
        if size != 0 {
            out.push_str("</span>");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::members::clean_text;

    fn runs(pieces: &[(&str, &str)]) -> String {
        let v: Vec<Run> = pieces.iter().map(|(t, how)| Run { t: t.to_string(), b: how.contains('b'), i: how.contains('i'), u: how.contains('u'), s: if how.contains('s') { "s".into() } else if how.contains('l') { "l".into() } else { String::new() } }).collect();
        serde_json::to_string(&v).unwrap()
    }

    #[test]
    fn formatted_words_keep_their_styles_and_draw_as_safe_html() {
        let raw = runs(&[("Big pull ", "bl"), ("today!", "l"), ("\n\n\n\n  Look at   ", ""), ("this", "iu"), (" one  \n", ""), ("tiny note", "s")]);
        let (text, rich) = clean(&raw, 2000).unwrap();
        assert_eq!(text, "Big pull today!\n\nLook at this one\ntiny note");
        let html = html_for(&rich, &text);
        assert_eq!(html, "<span class=\"rt-l\"><b>Big pull </b></span><span class=\"rt-l\">today!\n\n</span>Look at <i><u>this</u></i> one\n<span class=\"rt-s\">tiny note</span>");
        // The runs spell the plain words, letter for letter.
        let back: Vec<Run> = serde_json::from_str(&rich).unwrap();
        assert_eq!(back.iter().map(|r| r.t.as_str()).collect::<String>(), text);
        // Words with no style at all keep no runs: the plain words are enough.
        assert_eq!(clean(&runs(&[("Just ", ""), ("words", "")]), 2000).unwrap(), ("Just words".to_string(), String::new()));
        // What a person typed is never HTML: tags and quotes come out as letters.
        let (text, rich) = clean(&runs(&[("<script>alert(1)</script> & \"q\" 'x'", "b")]), 2000).unwrap();
        let html = html_for(&rich, &text);
        assert!(!html.contains("<script") && html.starts_with("<b>&lt;script&gt;alert(1)&lt;/script&gt; &amp; &quot;q&quot; &#39;x&#39;</b>"), "{html}");
        // Runs that aren't runs, or are about other words, are not used.
        assert!(clean("<b>hello</b>", 2000).is_none() && clean("{\"t\":\"x\"}", 2000).is_none());
        assert_eq!(html_for(&rich, "Other words"), "");
        assert_eq!(html_for("not json", "x"), "");
        assert_eq!(html_for("", "x"), "");
        // A style the page doesn't know is the normal size; unknown keys are ignored.
        let (text, rich) = clean("[{\"t\":\"Hi \",\"s\":\"huge\",\"color\":\"red\"},{\"t\":\"there\",\"b\":true}]", 2000).unwrap();
        assert_eq!((text.as_str(), html_for(&rich, &text).as_str()), ("Hi there", "Hi <b>there</b>"));
    }

    #[test]
    fn the_plain_words_are_what_plain_tidying_would_keep() {
        let cases: Vec<Vec<(&str, &str)>> = vec![
            vec![("  Hello   ", "b"), ("world  \r\n\r\n\r\n", ""), ("  next\tline ", "i"), ("\n\n", ""), ("", "b"), ("end   ", "l")],
            vec![("\n\n\n", "b"), ("  ", ""), ("a", "u"), ("\n \n", "s"), ("b", "")],
            vec![("one two", ""), (" three", "b"), ("\u{0}four\u{7} \u{0} five", "i")],
            vec![("   ", "b"), ("\n", ""), ("   ", "i")],
            vec![("ünï cödé 🎴 ok", "b"), ("\u{a0}\u{a0}wide\u{3000}space", "l")],
            vec![("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", "b"), (" y", ""), ("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzz", "s")],
            vec![("a\rb", "b"), ("\r\nc\r", ""), ("\r\n\r\nd", "u")],
        ];
        for pieces in cases {
            let whole: String = pieces.iter().map(|p| p.0).collect();
            for max in [5, 12, 40, 2000] {
                let (text, rich) = clean(&runs(&pieces), max).unwrap();
                assert_eq!(text, clean_text(&whole, max), "{pieces:?} at {max}");
                if !rich.is_empty() {
                    let back: Vec<Run> = serde_json::from_str(&rich).unwrap();
                    assert_eq!(back.iter().map(|r| r.t.as_str()).collect::<String>(), text);
                    assert!(back.iter().all(|r| !r.t.is_empty()) && !html_for(&rich, &text).is_empty());
                }
            }
        }
        // Too many pieces to be by hand: the words stay, plain.
        let many: Vec<(&str, &str)> = (0..RUNS_MAX + 50).map(|n| if n % 2 == 0 { ("a", "b") } else { ("b", "") }).collect();
        let (text, rich) = clean(&runs(&many), 2000).unwrap();
        assert_eq!((text.chars().count(), rich.as_str()), (RUNS_MAX + 50, ""));
    }
}
