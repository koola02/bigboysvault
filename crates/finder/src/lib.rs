//! The card finder: every card in the vault, filtered on the visitor's own device as they type.
//!
//! The page arrives complete from the server and works without this. Once the catalogue has downloaded, the finder takes
//! over the page's search box and card grid, so results change on every keystroke with no trip to the server.
use leptos::mount::mount_to;
use leptos::prelude::*;
use std::sync::Arc;
use vault_search::{norm, parse, peso, pos_for, price_at, read_tokens, read_words, search, title_and_set, Binder, Card, Found, Sort, Token, COND_LABELS, FINISH_LABELS, LANG_LABELS};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

const STEP: usize = 48;


async fn fetch_text(url: &str) -> Result<String, JsValue> {
    let w = web_sys::window().ok_or("no window")?;
    let resp: web_sys::Response = JsFuture::from(w.fetch_with_str(url)).await?.dyn_into()?;
    if !resp.ok() {
        return Err(JsValue::from_f64(resp.status() as f64));
    }
    JsFuture::from(resp.text()?).await?.as_string().ok_or_else(|| "not text".into())
}

fn url_param(name: &str) -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    web_sys::UrlSearchParams::new_with_str(&search).ok()?.get(name)
}

/// Keep the address bar in step, so links can be shared and the back button returns to the same results.
fn write_url(q: &str, sort: Sort, binder: Option<&str>, tokens: &[Token], min: Option<f64>, max: Option<f64>) {
    let Some(w) = web_sys::window() else { return };
    let Ok(p) = web_sys::UrlSearchParams::new() else { return };
    if !q.is_empty() {
        p.set("q", q);
    }
    if sort != Sort::Newest {
        p.set("sort", sort.key());
    }
    if let Some(b) = binder {
        p.set("binder", b);
    }
    let pick = |want: fn(&Token) -> Option<&String>| tokens.iter().filter_map(want).cloned().collect::<Vec<_>>().join(",");
    for (key, value) in [
        ("cond", pick(|t| if let Token::Cond(v) = t { Some(v) } else { None })),
        ("fin", pick(|t| if let Token::Finish(v) = t { Some(v) } else { None })),
        ("lang", pick(|t| if let Token::Lang(v) = t { Some(v) } else { None })),
        ("names", pick(|t| if let Token::Name(v) = t { Some(v) } else { None })),
    ] {
        if !value.is_empty() {
            p.set(key, &value);
        }
    }
    if let Some(v) = min {
        p.set("min", &v.to_string());
    }
    if let Some(v) = max {
        p.set("max", &v.to_string());
    }
    let qs = String::from(p.to_string());
    let path = w.location().pathname().unwrap_or_default();
    let url = if qs.is_empty() { path } else { format!("{path}?{qs}") };
    if let Ok(h) = w.history() {
        let _ = h.replace_state_with_url(&JsValue::NULL, "", Some(&url));
    }
}

/// What a shared link asked for.
fn tokens_from_url() -> Vec<Token> {
    let list = |k: &str| url_param(k).map(|v| v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>()).unwrap_or_default();
    let mut out = vec![];
    out.extend(list("cond").into_iter().filter(|c| COND_LABELS.iter().any(|x| x.0 == c)).map(Token::Cond));
    out.extend(list("fin").into_iter().filter(|f| FINISH_LABELS.contains(&f.as_str())).map(Token::Finish));
    out.extend(list("lang").into_iter().filter(|l| LANG_LABELS.iter().any(|x| x.0 == l)).map(Token::Lang));
    out.extend(list("names").into_iter().map(Token::Name));
    out
}

/// The Pokémon's name from a card's title: "Pikachu V - SWSH285/307" becomes "Pikachu V", and "Dratini (148)" becomes "Dratini".
fn pokemon_name(c: &Card) -> String {
    let short = title_and_set(&c.title, &c.set).0;
    let head = short.split(" - ").next().unwrap_or(&short);
    head.split(" (").next().unwrap_or(head).trim().to_string()
}

fn cond_data(t: &Token) -> &'static str {
    match t {
        Token::Cond(c) => COND_LABELS.iter().find(|x| x.0 == c).map(|x| x.0).unwrap_or(""),
        _ => "",
    }
}

/// Tell the server what was searched for, so its suggestions keep learning.
fn note_search(q: &str) {
    if q.trim().chars().count() < 2 {
        return;
    }
    if let Some(w) = web_sys::window() {
        let enc = String::from(js_sys::encode_uri_component(q));
        let _ = w.navigator().send_beacon(&format!("/api/searched?q={enc}"));
    }
}

#[component]
fn Tile(card: Arc<Card>) -> impl IntoView {
    let href = format!("/products/{}", card.handle);
    let price = peso(card.price);
    // The product's number is the tail of its handle; the cart (static/cart.js) wants it on the tile.
    let pid: i64 = card.handle.rsplit('-').next().and_then(|t| t.parse().ok()).unwrap_or(0);
    let (buy, buy_title, buy_price, buy_img, buy_url, buy_label) = (pid > 0 && card.price > 0.0 && card.qty > 0, title_and_set(&card.title, &card.set).0, format!("{}", card.price), card.img.clone(), format!("/products/{}", card.handle), format!("Add {} to cart", title_and_set(&card.title, &card.set).0));
    let (title, title2, img, grade, qty) = (title_and_set(&card.title, &card.set).0, card.title.clone(), card.img.clone(), card.grade.clone(), card.qty);
    let (set, cond_c, cond, sym) = (card.set.clone(), vault_search::cond_class(&card.variant), vault_search::tile_condition(&card.variant), card.sym.clone());
    let foil = vault_search::foil_class(&format!("{} {}", card.title, card.variant));
    let notes = card.notes.clone();
    let tags: Vec<String> = card.tags.split(',').map(str::trim).filter(|t| !t.is_empty() && !["binder-", "finish-", "rarity-", "set-"].iter().any(|p| t.to_lowercase().starts_with(p))).map(str::to_string).collect();
    view! {
        <a class="card" href=href data-foil=foil data-c=cond_c>
            <div class="card-img">
                {(!img.is_empty()).then(|| view! { <img src=img loading="lazy" decoding="async" alt=title2/> })}
                {(!grade.is_empty()).then(|| view! { <span class="grade">{grade}</span> })}
                {(qty > 1).then(|| view! { <span class="qty">{format!("×{qty}")}</span> })}
            </div>
            <div class="card-t">{title}</div>
            <div class="card-m">
                {(!set.is_empty()).then(|| view! { <span class="card-set">{(!sym.is_empty()).then(|| view! { <img class="card-sym" src=sym loading="lazy" decoding="async" alt="" width="16" height="16"/> })}{set}</span> })}
                {(!cond.is_empty()).then(|| view! { <span class="card-cond" data-c=cond_c>{cond}</span> })}
            </div>
            {(!tags.is_empty()).then(|| view! { <div class="card-tags">{tags.into_iter().map(|t| view! { <span class="tag-hl">{t}</span> }).collect_view()}</div> })}
            {(!notes.is_empty()).then(|| view! { <div class="card-note">{notes}</div> })}
            <div class="card-p">{price}{buy.then(|| view! { <span class="cart-mini" role="button" tabindex="0" data-cart-vault=pid data-title=buy_title data-price=buy_price data-image=buy_img data-url=buy_url aria-label=buy_label title="Add to cart">"+"</span> })}</div>
        </a>
    }
}

#[component]
fn Finder(binders: Vec<Binder>, cards: Vec<Arc<Card>>, embedded: bool) -> impl IntoView {
    // Inside a binder's floating window the finder keeps to itself: it leaves the page's search box, heading and address alone.
    let param = move |k: &str| if embedded { None } else { url_param(k) };
    let input: Option<web_sys::HtmlInputElement> = if embedded { None } else { document().query_selector("form.search input").ok().flatten().and_then(|e| e.dyn_into().ok()) };
    let start_binder = param("binder").and_then(|h| binders.iter().position(|b| b.handle == h)).map(|i| i as u16);
    let q = RwSignal::new(param("q").unwrap_or_default());
    let sort = RwSignal::new(Sort::parse(&param("sort").unwrap_or_default()));
    let binder = RwSignal::new(start_binder);
    let shown = RwSignal::new(STEP);
    let total_cards: usize = cards.iter().map(|c| c.qty as usize).sum();

    // What the filters can offer: only what's really in the vault.
    let cond_options: Vec<Token> = COND_LABELS.iter().filter(|(k, _)| cards.iter().any(|c| vault_search::cond_class(&c.variant) == *k || (*k == "gr" && !c.grade.is_empty()))).map(|(k, _)| Token::Cond((*k).to_string())).collect();
    let finish_options: Vec<Token> = FINISH_LABELS.iter().filter(|f| cards.iter().any(|c| c.finish == **f)).map(|f| Token::Finish((*f).to_string())).collect();
    let lang_options: Vec<Token> = LANG_LABELS.iter().filter(|(k, _)| cards.iter().any(|c| c.lang == *k)).map(|(k, _)| Token::Lang((*k).to_string())).collect();
    let price_lo = cards.iter().map(|c| c.price).filter(|p| *p > 0.0).fold(f64::INFINITY, f64::min);
    let price_lo = if price_lo.is_finite() { price_lo } else { 1.0 };
    let price_hi = cards.iter().map(|c| c.price).fold(0.0, f64::max);

    // Pokémon names for the advanced search's suggestions, most common first.
    let mut tally: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for c in &cards {
        let n = pokemon_name(c);
        if !n.is_empty() {
            *tally.entry(n).or_default() += c.qty;
        }
    }
    let mut known: Vec<(String, u32)> = tally.into_iter().collect();
    known.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let known_norm: Vec<String> = known.iter().map(|k| norm(&k.0)).collect();
    let known = StoredValue::new(known);
    let known_norm = StoredValue::new(known_norm);

    // `applied` is what the cards are filtered by. `draft` is what the advanced search bar holds until Confirm. The filter buttons change both, so they never disagree.
    let start = if embedded { vec![] } else { tokens_from_url() };
    let applied = RwSignal::new(start.clone());
    let draft = RwSignal::new(start);
    let typed = RwSignal::new(String::new());
    let picked_at = RwSignal::new(None::<usize>);
    let pmin = RwSignal::new(param("min").and_then(|v| v.parse().ok()).map_or(0, |v| pos_for(v, price_lo, price_hi)));
    let pmax = RwSignal::new(param("max").and_then(|v| v.parse().ok()).map_or(100, |v| pos_for(v, price_lo, price_hi)));
    let min_price = move || { let p = pmin.get(); (p > 0).then(|| price_at(p, price_lo, price_hi)) };
    let max_price = move || { let p = pmax.get(); (p < 100).then(|| price_at(p, price_lo, price_hi)) };
    let has_url_filters = !applied.get_untracked().is_empty() || pmin.get_untracked() > 0 || pmax.get_untracked() < 100;
    let all = StoredValue::new(cards);
    let handles: Vec<String> = binders.iter().map(|b| b.handle.clone()).collect();
    let handles = StoredValue::new(handles);

    // The search box in the page header drives the finder, so there is only ever one box to type in.
    if let Some(input) = input.clone() {
        input.set_value(&q.get_untracked());
        let reader = input.clone();
        let on_input = Closure::<dyn FnMut()>::new(move || {
            q.set(reader.value());
            shown.set(STEP);
        });
        let _ = input.add_event_listener_with_callback("input", on_input.as_ref().unchecked_ref());
        on_input.forget();
        if let Some(form) = input.form() {
            let reader = input.clone();
            let on_submit = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
                e.prevent_default();
                note_search(&reader.value());
                let _ = reader.blur();
            });
            let _ = form.add_event_listener_with_callback("submit", on_submit.as_ref().unchecked_ref());
            on_submit.forget();
        }
    }

    // On-device AI (static/browser-ai.js) hands its reading of a sentence over as plain search words.
    let box_for_event = input.clone();
    if !embedded {
    window_event_listener_untyped("vault:ask", move |ev: web_sys::Event| {
        let Some(ev) = ev.dyn_ref::<web_sys::CustomEvent>() else { return };
        let detail = ev.detail();
        let words = js_sys::Reflect::get(&detail, &"q".into()).ok().and_then(|v| v.as_string()).unwrap_or_default();
        let wanted = js_sys::Reflect::get(&detail, &"binder".into()).ok().and_then(|v| v.as_string());
        if let Some(i) = box_for_event.as_ref() {
            i.set_value(&words);
        }
        q.set(words);
        if let Some(h) = wanted {
            binder.set(handles.with_value(|hs| hs.iter().position(|x| *x == h)).map(|i| i as u16));
        }
        shown.set(STEP);
    });
    }

    let found = Memo::new(move |_| {
        let mut f = read_words(&q.get());
        f.binder = binder.get();
        f.min_price = f.min_price.or(min_price());
        f.max_price = f.max_price.or(max_price());
        applied.with(|t| f.apply_tokens(t));
        let s = sort.get();
        let r: Found = all.with_value(|c| search(c, &f, s));
        (Arc::new(r.hits), r.typo)
    });

    if !embedded {
    Effect::new(move |_| {
        let (qv, s, b) = (q.get(), sort.get(), binder.get());
        let h = b.and_then(|i| handles.with_value(|hs| hs.get(i as usize).cloned()));
        applied.with(|t| write_url(qv.trim(), s, h.as_deref(), t, min_price(), max_price()));
        if let Some(h1) = document().query_selector("main h1").ok().flatten() {
            let t = qv.trim();
            h1.set_text_content(Some(&if t.is_empty() { "All cards".to_string() } else { format!("Results for “{t}”") }));
        }
    });

    // Load more cards as the visitor nears the bottom of the page.
    window_event_listener_untyped("scroll", move |_| {
        let Some(w) = web_sys::window() else { return };
        let Some(el) = w.document().and_then(|d| d.document_element()) else { return };
        let (y, h) = (w.scroll_y().unwrap_or(0.0), w.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(0.0));
        if y + h > f64::from(el.scroll_height()) - 1400.0 && shown.get_untracked() < found.get_untracked().0.len() {
            shown.update(|n| *n += STEP);
        }
    });
    }

    let toggle = move |t: Token| {
        let on = applied.with_untracked(|a| a.contains(&t));
        let change = |list: &mut Vec<Token>| { list.retain(|x| *x != t); if !on { list.push(t.clone()); } };
        applied.update(change);
        draft.update(change);
        shown.set(STEP);
    };
    let is_name = move |n: &str| known_norm.with_value(|k| k.iter().any(|x| x == n));
    let add_typed = move || {
        let text = typed.get_untracked();
        if text.trim().is_empty() { return; }
        let found = read_tokens(&text, &is_name);
        draft.update(|d| for t in found { if !d.contains(&t) { d.push(t); } });
        typed.set(String::new());
        picked_at.set(None);
    };
    let confirm = move || {
        add_typed();
        applied.set(draft.get_untracked());
        shown.set(STEP);
    };
    let waiting = move || !typed.get().trim().is_empty() || draft.with(|d| applied.with(|a| d.len() != a.len() || d.iter().any(|t| !a.contains(t))));

    // Suggestions while typing: conditions, finishes and languages first, then Pokémon.
    let suggestions = Memo::new(move |_| {
        let text = typed.get();
        let want = norm(text.rsplit(',').next().unwrap_or(&text));
        if want.is_empty() { return vec![]; }
        let mut out: Vec<(Token, String)> = vec![];
        let fixed = COND_LABELS.iter().map(|(k, _)| Token::Cond((*k).to_string())).chain(FINISH_LABELS.iter().map(|f| Token::Finish((*f).to_string()))).chain(LANG_LABELS.iter().map(|(k, _)| Token::Lang((*k).to_string())));
        for t in fixed {
            if norm(&t.label()).contains(&want) && !draft.with(|d| d.contains(&t)) {
                let hint = match t { Token::Cond(_) => "Condition", Token::Finish(_) => "Finish", _ => "Language" };
                out.push((t, hint.to_string()));
            }
        }
        known.with_value(|list| {
            for (name, n) in list {
                if out.len() >= 8 { break; }
                let t = Token::Name(name.clone());
                if norm(name).contains(&want) && !draft.with(|d| d.contains(&t)) {
                    out.push((t, format!("{n} card{}", if *n == 1 { "" } else { "s" })));
                }
            }
        });
        out.truncate(8);
        out
    });
    let take_suggestion = move |t: Token| {
        // Only the words after the last comma were being completed.
        let keep = typed.with_untracked(|x| x.rsplit_once(',').map(|(a, _)| a.to_string()).unwrap_or_default());
        typed.set(keep);
        add_typed();
        draft.update(|d| if !d.contains(&t) { d.push(t.clone()); });
        typed.set(String::new());
        picked_at.set(None);
    };
    let box_ref = NodeRef::<leptos::html::Input>::new();
    let adv_id = if embedded { "fdAdvBinder" } else { "fdAdv" };

    let filter_count = move || applied.with(Vec::len) + usize::from(pmin.get() > 0 || pmax.get() < 100);
    // Anything at all narrowing the cards, or waiting to: words in either search box, a name not confirmed yet, a binder, a filter or a price.
    let anything_set = move || filter_count() > 0 || !draft.with(Vec::is_empty) || !typed.get().trim().is_empty() || !q.get().trim().is_empty() || binder.get().is_some();

    let reset = move |_| {
        q.set(String::new());
        binder.set(None);
        applied.set(vec![]);
        draft.set(vec![]);
        typed.set(String::new());
        picked_at.set(None);
        pmin.set(0);
        pmax.set(100);
        shown.set(STEP);
        if !embedded {
            if let Some(el) = document().query_selector("form.search input").ok().flatten().and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok()) {
                el.set_value("");
            }
        }
    };

    let binder_chips = binders
        .into_iter()
        .enumerate()
        .filter(|(_, b)| b.count > 0)
        .map(|(i, b)| {
            let i = i as u16;
            view! {
                <button type="button" class="fd-b" class:on=move || binder.get() == Some(i) aria-pressed=move || (binder.get() == Some(i)).to_string() on:click=move |_| { binder.update(|c| *c = if *c == Some(i) { None } else { Some(i) }); shown.set(STEP); }>
                    <span class="fd-b-pic">{(!b.img.is_empty()).then(|| view! { <img src=b.img.clone() loading="lazy" decoding="async" alt=""/> })}</span>
                    <span class="fd-b-t">{b.title.clone()}</span>
                    <small>{b.count}</small>
                </button>
            }
        })
        .collect_view();

    let chips = move |options: Vec<Token>| {
        options
            .into_iter()
            .map(|t| {
                let (t1, t2, t3) = (t.clone(), t.clone(), t.clone());
                let c = cond_data(&t);
                view! { <button type="button" class="fd-chip" data-c=c class:on=move || applied.with(|a| a.contains(&t1)) aria-pressed=move || applied.with(|a| a.contains(&t2)).to_string() on:click=move |_| toggle(t3.clone())>{t.label()}</button> }
            })
            .collect_view()
    };
    let (cond_chips, finish_chips, lang_chips) = (chips(cond_options), chips(finish_options), chips(lang_options));

    view! {
        <div class="fd-tools">
            {(!embedded).then(|| view! { <div class="fd-binders" role="group" aria-label="Pick a binder">{binder_chips}</div> })}
            <form class="fd-adv" on:submit=move |e: leptos::ev::SubmitEvent| { e.prevent_default(); confirm(); }>
                <label class="fd-adv-h" for=adv_id>"Advanced search"<small>"Combine anything, like Near Mint, Holofoil, Pikachu, Mewtwo"</small></label>
                <div class="fd-bar">
                    <div class="fd-field" on:click=move |_| { if let Some(i) = box_ref.get() { let _ = i.focus(); } }>
                        {move || draft.get().into_iter().map(|t| {
                            let c = cond_data(&t);
                            let (t1, label) = (t.clone(), t.label());
                            let aria = format!("Take out {label}");
                            view! { <span class="fd-tok" data-c=c>{label}<button type="button" aria-label=aria on:click=move |_| { draft.update(|d| d.retain(|x| *x != t1)); applied.update(|a| a.retain(|x| *x != t1)); shown.set(STEP); }>"✕"</button></span> }
                        }).collect_view()}
                        <input id=adv_id type="text" node_ref=box_ref autocomplete="off" autocapitalize="off" spellcheck="false" enterkeyhint="search"
                            placeholder=move || if draft.with(Vec::is_empty) { "Type a condition, finish or Pokémon" } else { "Add more" }
                            prop:value=move || typed.get()
                            on:input=move |e| { typed.set(event_target_value(&e)); picked_at.set(None); }
                            on:blur=move |_| picked_at.set(None)
                            on:keydown=move |e: web_sys::KeyboardEvent| {
                                let n = suggestions.with_untracked(Vec::len);
                                match e.key().as_str() {
                                    "ArrowDown" if n > 0 => { e.prevent_default(); picked_at.update(|p| *p = Some(p.map_or(0, |i| (i + 1) % n))); }
                                    "ArrowUp" if n > 0 => { e.prevent_default(); picked_at.update(|p| *p = Some(p.map_or(n - 1, |i| (i + n - 1) % n))); }
                                    "Enter" | "," | "Tab" => {
                                        let pick = picked_at.get_untracked().and_then(|i| suggestions.with_untracked(|s| s.get(i).map(|x| x.0.clone())));
                                        if let Some(t) = pick { e.prevent_default(); take_suggestion(t); }
                                        else if !typed.get_untracked().trim().is_empty() { e.prevent_default(); add_typed(); }
                                    }
                                    "Backspace" if typed.get_untracked().is_empty() => {
                                        let gone = draft.try_update(|d| d.pop()).flatten();
                                        if let Some(t) = gone { applied.update(|a| a.retain(|x| *x != t)); shown.set(STEP); }
                                    }
                                    _ => {}
                                }
                            }
                        />
                    </div>
                    <button type="submit" class="btn fd-go" class:waiting=waiting>"Confirm"</button>
                </div>
                <Show when=move || !suggestions.with(Vec::is_empty)>
                    <ul class="fd-suggest" role="listbox">
                        {move || suggestions.get().into_iter().enumerate().map(|(i, (t, hint))| {
                            let t1 = t.clone();
                            let c = cond_data(&t);
                            view! { <li role="option" class:on=move || picked_at.get() == Some(i) aria-selected=move || (picked_at.get() == Some(i)).to_string() data-c=c on:mousedown=move |e| { e.prevent_default(); take_suggestion(t1.clone()); }><b>{t.label()}</b><small>{hint}</small></li> }
                        }).collect_view()}
                    </ul>
                </Show>
            </form>
            <details class="fd-filters" prop:open=has_url_filters>
                <summary>
                    <span class="fd-f-t">"Filters"</span>
                    <Show when=move || { filter_count() > 0 }><span class="fd-f-n">{filter_count}</span></Show>
                </summary>
                <div class="fd-f-rows">
                    <div class="fd-frow"><span class="fd-flabel">"Condition"</span><div class="fd-opts" role="group" aria-label="Condition">{cond_chips}</div></div>
                    <div class="fd-frow"><span class="fd-flabel">"Finish"</span><div class="fd-opts" role="group" aria-label="Finish">{finish_chips}</div></div>
                    <div class="fd-frow"><span class="fd-flabel">"Language"</span><div class="fd-opts" role="group" aria-label="Language">{lang_chips}</div></div>
                    <div class="fd-frow"><span class="fd-flabel">"Price"</span>
                        <div class="fd-price">
                            <output class="fd-p-v">{move || peso(price_at(pmin.get(), price_lo, price_hi))}</output>
                            <div class="fd-range" style=move || format!("--a:{};--b:{}", pmin.get(), pmax.get())>
                                <span class="fd-track"></span><span class="fd-fill"></span>
                                <input type="range" min="0" max="100" step="1" aria-label="Lowest price" class:top=move || { pmin.get() > 90 }
                                    prop:value=move || pmin.get().to_string()
                                    aria-valuetext=move || peso(price_at(pmin.get(), price_lo, price_hi))
                                    on:input=move |e| { let v: u32 = event_target_value(&e).parse().unwrap_or(0); pmin.set(v.min(pmax.get_untracked().saturating_sub(1))); shown.set(STEP); }/>
                                <input type="range" min="0" max="100" step="1" aria-label="Highest price"
                                    prop:value=move || pmax.get().to_string()
                                    aria-valuetext=move || peso(price_at(pmax.get(), price_lo, price_hi))
                                    on:input=move |e| { let v: u32 = event_target_value(&e).parse().unwrap_or(100); pmax.set(v.max(pmin.get_untracked() + 1)); shown.set(STEP); }/>
                            </div>
                            <output class="fd-p-v">{move || peso(price_at(pmax.get(), price_lo, price_hi))}</output>
                        </div>
                    </div>
                    <div class="fd-frow"><span class="fd-flabel">"Sort"</span>
                        <label class="fd-sort">
                            <select aria-label="Sort" on:change=move |e| { sort.set(Sort::parse(&event_target_value(&e))); shown.set(STEP); }>
                                <option value="" prop:selected=move || sort.get() == Sort::Newest>"Featured"</option>
                                <option value="price-asc" prop:selected=move || sort.get() == Sort::PriceAsc>"Price: low to high"</option>
                                <option value="price-desc" prop:selected=move || sort.get() == Sort::PriceDesc>"Price: high to low"</option>
                                <option value="title" prop:selected=move || sort.get() == Sort::Name>"Name"</option>
                            </select>
                        </label>
                    </div>
                </div>
            </details>
        </div>
        <div class="fd-status">
            <p class="sub fd-count" aria-live="polite">
                {move || {
                    let (hits, typo) = found.get();
                    let n: usize = all.with_value(|c| hits.iter().map(|i| c[*i].copies_in(binder.get()) as usize).sum());
                    let word = if n == 1 { "card" } else { "cards" };
                    if n == total_cards { format!("{n} {word}") } else if typo { format!("Closest spelling: {n} {word}") } else { format!("{n} of {total_cards} {word}") }
                }}
            </p>
            <Show when=anything_set>
                <button type="button" class="fd-clear" on:click=reset>"✕ Clear all"</button>
            </Show>
        </div>
        <div class="grid fd-grid">
            <For
                each=move || {
                    let (hits, _) = found.get();
                    hits.iter().take(shown.get()).copied().collect::<Vec<_>>()
                }
                key=|i| *i
                children=move |i| { let card = all.with_value(|c| c[i].clone()); view! { <Tile card=card/> } }
            />
        </div>
        <Show when=move || found.get().0.is_empty()>
            <div class="fd-none">
                <p>"Nothing in the vault matches that."</p>
                <button type="button" class="btn" on:click=reset>"Show all cards"</button>
            </div>
        </Show>
        <Show when=move || { shown.get() < found.get().0.len() }>
            <div class="fd-more"><button type="button" class="linkbtn" on:click=move |_| shown.update(|n| *n += STEP)>"Show more cards"</button></div>
        </Show>
    }
}

/// Called by static/finder-boot.js. Returns false if the page has no finder slot.
#[wasm_bindgen]
pub async fn start(slot_id: String, data_url: String) -> Result<bool, JsValue> {
    let Some(slot) = document().get_element_by_id(&slot_id).and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) else {
        return Ok(false);
    };
    let text = fetch_text(&data_url).await?;
    let (binders, cards) = parse(&text);
    if cards.is_empty() {
        return Ok(false);
    }
    // The finder replaces the server's list only now that it has everything it needs.
    for sel in ["main .filters", "main #grid", "main .sub:not(.fd-count)"] {
        if let Some(e) = document().query_selector(sel).ok().flatten() {
            e.remove();
        }
    }
    document().document_element().ok_or("no root")?.set_attribute("data-finder", "on")?;
    let cards: Vec<Arc<Card>> = cards.into_iter().map(Arc::new).collect();
    mount_to(slot, move || view! { <Finder binders=binders cards=cards embedded=false/> }).forget();
    Ok(true)
}

/// A finder running in a binder's floating window. Freeing this from JavaScript takes it away again.
#[wasm_bindgen]
pub struct Mounted {
    _keep: Box<dyn std::any::Any>,
}

/// Search and filter one binder's cards, inside `slot`. Only offers what's in that binder. Called by static/card-float.js.
#[wasm_bindgen]
pub async fn start_in_binder(slot: web_sys::HtmlElement, data_url: String, handle: String) -> Result<Option<Mounted>, JsValue> {
    let text = fetch_text(&data_url).await?;
    let (binders, cards) = parse(&text);
    let Some(at) = binders.iter().position(|b| b.handle == handle) else { return Ok(None) };
    // Only what this binder holds, counted by the copies in this binder.
    let cards: Vec<Arc<Card>> = cards
        .into_iter()
        .filter(|c| c.binders.contains(&(at as u16)))
        .map(|mut c| {
            c.qty = c.copies_in(Some(at as u16));
            Arc::new(c)
        })
        .collect();
    if cards.is_empty() {
        return Ok(None);
    }
    let keep = mount_to(slot, move || view! { <Finder binders=vec![] cards=cards embedded=true/> });
    Ok(Some(Mounted { _keep: Box::new(keep) }))
}
