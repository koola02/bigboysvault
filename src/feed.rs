//! The home feed (new look): posts from marcuru_rs::social, drawn with live cards and prices, ten or so at a time.
//! It never ends: when the posts run out, the feed goes on with sets of cards from the vault, binder after binder,
//! round and round. Between posts sit the stores carousel and, at the very top, the showcase as a row of stories.
//!
//! GET /           the feed page (signed-out visitors get a kept copy; signed-in ones see their own reactions)
//! GET /feed?c=    the next items, as HTML (the page's last element asks for them when it scrolls into view)
//! POST /posts     a trainer's post (words, up to four photos, maybe a card)
//! POST /posts/{id}/react · GET|POST /posts/{id}/comments · POST /posts/{id}/delete|hide · POST /comments/{id}/delete
use crate::community::{acting, current, fail, gated, html, ok, photo_cap_mb, render, same_site, store_photo, TOO_BIG};
use crate::{admin, cached, look, messages, page, with_db, App, Chrome, Res, Seo};
use askama::Template;
use axum::{
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
    Json,
};
use marcuru_rs::{blocks, db, dm, members, sellers, social};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};

const PAGE: i64 = 8;
static SYNCED: AtomicI64 = AtomicI64::new(0);

pub struct Who {
    pub name: String,
    pub avatar: String,
    pub initial: String,
    pub url: String,
}

pub struct Tile {
    pub title: String,
    pub img: String,
    pub price: String,
    pub url: String,
    pub cond: String,
    pub c: String,
    pub sub: String,
}

pub struct Move {
    pub title: String,
    pub url: String,
    pub img: String,
    pub pct: String,
    pub up: bool,
    pub price: String,
}

pub struct PostView {
    pub id: i64,
    pub kind: String,
    pub who: Who,
    pub headline: String,
    pub when: String,
    pub text: String,
    pub tiles: Vec<Tile>,
    /// Cards beyond the ones shown ("+12" on the last picture).
    pub more: i64,
    pub link: String,
    pub link_text: String,
    /// (photo id, full size, small). The id is 0 for a vouch's photos, which are edited on the vouch itself.
    pub photos: Vec<(i64, String, String)>,
    pub member: i64,
    pub edited: bool,
    pub attach: Option<Tile>,
    pub moves: Vec<Move>,
    /// The trainers a welcome post is about.
    pub people: Vec<Who>,
    pub like_n: i64,
    pub comment_n: i64,
    pub mine: String,
    pub tops: Vec<&'static str>,
    pub pinned: bool,
    /// The viewer wrote it and may delete it.
    pub own: bool,
    /// A one-line item with nothing under it ("joined").
    pub small: bool,
}

impl PostView {
    pub fn mine_emoji(&self) -> &'static str {
        social::emoji(&self.mine)
    }
    pub fn mine_word(&self) -> String {
        if self.mine.is_empty() { "Like".into() } else { let mut w = self.mine.clone(); if let Some(f) = w.get_mut(0..1) { f.make_ascii_uppercase(); } w }
    }
    pub fn grid(&self) -> usize {
        self.tiles.len().min(4)
    }
    pub fn comments_text(&self) -> String {
        format!("{} comment{}", self.comment_n, if self.comment_n == 1 { "" } else { "s" })
    }
}

pub struct StoreView {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub avatar: String,
    pub initial: String,
    pub pic: String,
    pub price: String,
    pub sub: String,
}

pub struct VaultUnit {
    pub title: String,
    pub url: String,
    pub tiles: Vec<Tile>,
    pub total: i64,
}

pub struct Item {
    pub post: Option<PostView>,
    pub stores: Vec<StoreView>,
    pub vault: Option<VaultUnit>,
}

pub struct Story {
    pub title: String,
    pub img: String,
    pub url: String,
}

#[derive(Template)]
#[template(path = "feed.html")]
struct FeedPage {
    nav: Chrome,
    stories: Vec<Story>,
    items: Vec<Item>,
    next: String,
    tagline: String,
    featured: Option<Tile>,
    moves: Vec<Move>,
    stores: Vec<StoreView>,
    /// Only one post is shown (its own page), with its comments open.
    single: bool,
    end: bool,
}

#[derive(Template)]
#[template(path = "feed_items.html")]
struct FeedPart {
    items: Vec<Item>,
    next: String,
    /// The feed is over (only when the admins switched the vault sets off).
    end: bool,
}

fn product_tile(p: &db::Product) -> Tile {
    Tile { title: p.short_title(), img: p.img(360), price: if p.price > 0.0 { p.price_text() } else { String::new() }, url: format!("/products/{}", p.handle), cond: p.tile_cond(), c: p.cond_class().to_string(), sub: p.set_text() }
}

fn who(c: &rusqlite::Connection, id: i64, brand: &str, store: bool) -> rusqlite::Result<Option<Who>> {
    if id == 0 {
        return Ok(Some(Who { name: brand.to_string(), avatar: "/static/img/logo-sm-144.png?v=1".into(), initial: "B".into(), url: "/".into() }));
    }
    Ok(members::get(c, id)?.filter(|m| !m.blocked).map(|m| Who {
        name: if store { m.store_title() } else { m.name.clone() },
        avatar: m.avatar.clone(),
        initial: m.initial(),
        url: if store { m.store_url() } else { format!("/trainers/{}", m.id) },
    }))
}

/// One post, drawn from what the site holds right now. None: nothing left to show (cards sold, trainer gone).
fn post_view(c: &rusqlite::Connection, p: &social::Post, brand: &str, viewer: i64) -> rusqlite::Result<Option<PostView>> {
    let Some(author) = who(c, p.member_id, brand, p.kind == "listing")? else { return Ok(None) };
    let mut v = PostView {
        id: p.id,
        kind: p.kind.clone(),
        who: author,
        headline: String::new(),
        when: members::ago(p.created_at),
        // An automatic post shows the words its seller or an admin added, if any.
        text: if p.written() || p.kind == "vouch" { p.text.clone() } else { p.note.clone() },
        tiles: vec![],
        more: 0,
        link: String::new(),
        link_text: String::new(),
        photos: vec![],
        member: p.member_id,
        edited: p.edited_at > 0,
        attach: None,
        moves: vec![],
        people: vec![],
        like_n: p.like_n,
        comment_n: p.comment_n,
        mine: p.mine.clone(),
        tops: social::top_reactions(c, p.id)?.iter().map(|k| social::emoji(k)).collect(),
        pinned: p.pinned,
        own: viewer != 0 && viewer == p.member_id && (p.kind == "post" || p.kind == "share"),
        small: false,
    };
    let ids = |key: &str| -> Vec<i64> { p.payload[key].as_array().map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default() };
    match p.kind.as_str() {
        "arrivals" => {
            for id in ids("ids") {
                if let Some(pr) = db::product_by_id(c, id)?.filter(|x| x.available) {
                    v.tiles.push(product_tile(&pr));
                }
            }
            if v.tiles.is_empty() {
                return Ok(None);
            }
            let count = p.payload["count"].as_i64().unwrap_or(v.tiles.len() as i64).max(v.tiles.len() as i64);
            v.headline = if count == 1 { "added a card to the vault".into() } else { format!("added {} cards to the vault", db::thousands(count)) };
            v.tiles.truncate(4);
            v.more = count - v.tiles.len() as i64;
            v.link = "/all".into();
            v.link_text = "See what's new".into();
        }
        "listing" => {
            for id in ids("cards") {
                if let Some(mc) = members::card(c, p.member_id, id)?.filter(|x| x.kind == "sale" && x.status == "Active") {
                    v.tiles.push(Tile { title: mc.title.clone(), img: mc.image.clone(), price: if mc.has_price() { mc.price_text() } else { "Ask".into() }, url: format!("/trainers/{}/cards/{}", p.member_id, mc.id), c: vault_search::cond_class(&mc.cond).to_string(), cond: mc.cond.clone(), sub: mc.card_set.clone() });
                }
            }
            if v.tiles.is_empty() {
                return Ok(None);
            }
            let n = v.tiles.len();
            v.headline = if n == 1 { "listed a card for sale".into() } else { format!("listed {n} cards for sale") };
            v.tiles.truncate(4);
            v.more = (n - v.tiles.len()) as i64;
            v.link = v.who.url.clone();
            v.link_text = "Visit the store".into();
        }
        "vouch" => {
            v.headline = format!("vouched for {brand}");
            if let Some(vid) = p.payload["vouch"].as_i64() {
                v.photos = c.prepare_cached("SELECT path, thumb FROM vouch_images WHERE vouch_id=?1 ORDER BY id LIMIT 4")?.query_map([vid], |r| Ok((0, r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
            }
        }
        "prices" => {
            v.headline = "Price moves this week".into();
            for m in p.payload["moves"].as_array().cloned().unwrap_or_default() {
                let (Some(id), Some(pct)) = (m["id"].as_i64(), m["pct"].as_i64()) else { continue };
                if let Some(pr) = db::product_by_id(c, id)?.filter(|x| x.available) {
                    v.moves.push(Move { title: pr.short_title(), url: format!("/products/{}", pr.handle), img: pr.img(360), pct: format!("{} {}%", if pct >= 0 { "Up" } else { "Down" }, pct.abs()), up: pct >= 0, price: pr.price_text() });
                }
            }
            if v.moves.is_empty() {
                return Ok(None);
            }
        }
        "joined" => {
            for id in ids("members") {
                if let Some(w) = who(c, id, brand, false)? {
                    v.people.push(w);
                }
            }
            if v.people.is_empty() {
                return Ok(None);
            }
            v.headline = if v.people.len() == 1 { "welcomed a new trainer".into() } else { format!("welcomed {} new trainers", v.people.len()) };
            v.small = true;
        }
        _ => {
            v.photos = social::photos_ids(c, p.id)?;
            let card = &p.payload["card"];
            if let Some(title) = card["title"].as_str() {
                let s = |k: &str| card[k].as_str().unwrap_or("").to_string();
                v.attach = Some(Tile { title: title.to_string(), img: s("image"), price: s("price"), url: s("url"), c: vault_search::cond_class(&s("cond")).to_string(), cond: s("cond"), sub: s("set") });
                v.headline = "shared a card".into();
            }
        }
    }
    Ok(Some(v))
}

fn store_views(c: &rusqlite::Connection, turn: usize) -> rusqlite::Result<Vec<StoreView>> {
    let mut out: Vec<StoreView> = sellers::directory(c)?
        .into_iter()
        .map(|(m, stats, cards)| {
            let first = cards.first();
            StoreView {
                id: m.id,
                name: m.store_title(),
                url: m.store_url(),
                avatar: m.avatar.clone(),
                initial: m.initial(),
                pic: first.map(|x| x.image.clone()).filter(|i| !i.is_empty()).unwrap_or_else(|| m.cover_photo.clone()),
                price: first.filter(|x| x.has_price()).map(|x| x.price_text()).unwrap_or_default(),
                sub: format!("{} on sale{}", stats.on_sale, first.map(|x| format!(" · {}", x.title)).unwrap_or_default()),
            }
        })
        .collect();
    if !out.is_empty() {
        let k = turn % out.len();
        out.rotate_left(k);
    }
    Ok(out)
}

/// The n-th set of cards from the vault: binder after binder, then round again with the next cards of each.
fn vault_unit(c: &rusqlite::Connection, nav: &Chrome, n: usize) -> rusqlite::Result<Option<VaultUnit>> {
    let binders: Vec<&db::Collection> = nav.links.iter().filter(|b| b.count > 0).collect();
    let (title, url, handle, total, round) = if binders.is_empty() {
        ("From the vault".to_string(), "/all".to_string(), None, 0, n)
    } else {
        let b = binders[n % binders.len()];
        (format!("From the vault · {}", b.title), format!("/collections/{}", b.handle), Some(b.handle.clone()), b.count, n / binders.len())
    };
    let get = |page: usize| db::list(c, &db::Query { collection: handle.clone(), q: String::new(), sort: String::new(), in_stock: true, page });
    // (The card list's pages count from 0.)
    let (mut items, _) = get(round)?;
    if items.is_empty() {
        items = get(0)?.0;
        if !items.is_empty() {
            let k = (round * 8) % items.len();
            items.rotate_left(k);
        }
    }
    let tiles: Vec<Tile> = items.iter().filter(|p| p.has_image()).take(8).map(product_tile).collect();
    Ok(if tiles.is_empty() { None } else { Some(VaultUnit { title, url, tiles, total }) })
}

/// Where a page of the feed starts: the top, after a post (time, id), or at the n-th vault set.
enum Cursor {
    Top,
    After(i64, i64),
    Vault(usize),
}

fn cursor(s: &str) -> Cursor {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.as_slice() {
        ["p", t, id] => match (t.parse(), id.parse()) {
            (Ok(t), Ok(id)) => Cursor::After(t, id),
            _ => Cursor::Top,
        },
        ["v", n] => Cursor::Vault(n.parse::<usize>().unwrap_or(0).min(100_000)),
        _ => Cursor::Top,
    }
}

/// One page of the feed and where the next one starts. There is always a next one.
fn build(c: &rusqlite::Connection, nav: &Chrome, viewer: i64, at: Cursor) -> rusqlite::Result<(Vec<Item>, String)> {
    let post_item = |p: PostView| Item { post: Some(p), stores: vec![], vault: None };
    let mut items: Vec<Item> = vec![];
    let vault_on = !social::auto_off(c)?.iter().any(|k| k == "vault");
    let (before, top) = match at {
        Cursor::Vault(_) if !vault_on => return Ok((items, String::new())),
        Cursor::Vault(n) => {
            for k in n..n + 2 {
                if let Some(u) = vault_unit(c, nav, k)? {
                    items.push(Item { post: None, stores: vec![], vault: Some(u) });
                }
            }
            if n % 6 == 4 {
                let stores = store_views(c, n / 6 + 1)?;
                if !stores.is_empty() {
                    items.insert(1.min(items.len()), Item { post: None, stores, vault: None });
                }
            }
            return Ok((items, format!("v:{}", n + 2)));
        }
        Cursor::Top => (None, true),
        Cursor::After(t, id) => (Some((t, id)), false),
    };
    if top {
        for p in social::pinned(c, viewer)? {
            if let Some(v) = post_view(c, &p, &nav.brand, viewer)? {
                items.push(post_item(v));
            }
        }
    }
    let posts = social::feed(c, viewer, before, PAGE)?;
    let last = posts.last().map(|p| (p.created_at, p.id));
    let full = posts.len() as i64 == PAGE;
    let mut shown = 0;
    for p in &posts {
        if let Some(v) = post_view(c, p, &nav.brand, viewer)? {
            items.push(post_item(v));
            shown += 1;
            // The stores carousel sits after the second post of the first page.
            if top && shown == 2 {
                let stores = store_views(c, 0)?;
                if !stores.is_empty() {
                    items.push(Item { post: None, stores, vault: None });
                }
            }
        }
    }
    let next = match (full, last) {
        (true, Some((t, id))) => format!("p:{t}:{id}"),
        _ if !vault_on => String::new(),
        _ => {
            // The posts are done: start on the vault, so the page never ends.
            if let Some(u) = vault_unit(c, nav, 0)? {
                items.push(Item { post: None, stores: vec![], vault: Some(u) });
            }
            "v:1".to_string()
        }
    };
    Ok((items, next))
}

/// The showcase, as the row of stories at the top: the hand-picked cards, the rows the owner put on the home page,
/// and the binders shown there.
fn stories(c: &rusqlite::Connection, nav: &Chrome) -> rusqlite::Result<Vec<Story>> {
    let mut out: Vec<Story> = vec![];
    let mut add = |title: String, img: String, url: String| {
        if !img.is_empty() && !title.is_empty() && !out.iter().any(|s| s.url == url) {
            out.push(Story { title, img, url });
        }
    };
    for b in blocks::blocks(c)?.iter().filter(|b| b.visible) {
        let d = &b.data;
        let text = |k: &str| d[k].as_str().unwrap_or("").trim().to_string();
        match b.kind.as_str() {
            "showcase" => {
                for id in d["ids"].as_array().cloned().unwrap_or_default().iter().filter_map(Value::as_i64).take(4) {
                    if let Some(p) = db::product_by_id(c, id)?.filter(|p| p.available && p.has_image()) {
                        add(p.short_title(), p.img(360), format!("/products/{}", p.handle));
                    }
                }
            }
            "row" => {
                let handle = text("handle");
                let Some(col) = nav.links.iter().find(|x| x.handle == handle && x.count > 0) else { continue };
                let (items, _) = db::list(c, &db::Query { collection: Some(handle.clone()), q: String::new(), sort: text("sort"), in_stock: true, page: 0 })?;
                let img = items.iter().find(|p| p.has_image()).map(|p| p.img(360)).unwrap_or_else(|| col.image.clone());
                let title = if text("title").is_empty() { col.title.clone() } else { text("title") };
                add(title, img, format!("/collections/{handle}"));
            }
            _ => {}
        }
    }
    for col in nav.links.iter().filter(|x| x.on_home && x.count > 0) {
        add(col.title.clone(), col.image.clone(), format!("/collections/{}", col.handle));
    }
    out.truncate(14);
    Ok(out)
}

/// The automatic posts are brought up to date at most every five minutes, when someone looks at the feed.
async fn sync_if_due(app: &App) {
    let now = crate::unix_now();
    let last = SYNCED.load(Ordering::Relaxed);
    if now - last < 300 || SYNCED.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_err() {
        return;
    }
    if let Err(e) = with_db(app, move |c| social::sync(c, now)).await {
        tracing::warn!("feed sync: {}", e.1);
    }
}

fn seo(nav: &Chrome) -> Seo {
    let site = nav.site.clone();
    Seo {
        title: format!("{} · Pokémon cards, graded slabs & collectibles in the Philippines", nav.brand),
        desc: crate::clip(&format!("{}: Pokémon cards and collectibles in one vault, priced in pesos. New arrivals, trainers' stores, vouches and price moves, every day.", nav.brand), 158),
        path: "/".into(),
        kind: "website",
        ld: crate::ld(json!({ "@context": "https://schema.org", "@graph": [
            { "@type": "Organization", "@id": format!("{site}/#org"), "name": nav.brand, "url": format!("{site}/"), "logo": format!("{site}/static/img/icon-512.png") },
            { "@type": "WebSite", "@id": format!("{site}/#site"), "name": nav.brand, "url": format!("{site}/"), "inLanguage": "en", "publisher": { "@id": format!("{site}/#org") },
              "potentialAction": { "@type": "SearchAction", "target": { "@type": "EntryPoint", "urlTemplate": format!("{site}/search?q={{search_term_string}}") }, "query-input": "required name=search_term_string" } }
        ] })),
        ..Default::default()
    }
}

async fn feed_html(app: &App, viewer: i64, only: Option<i64>) -> Result<String, (StatusCode, String)> {
    let t = with_db(app, move |c| {
        let mut nav = crate::chrome(c)?;
        let (items, next, single) = match only {
            Some(id) => {
                let item = match social::post(c, viewer, id)?.filter(|p| !p.hidden) {
                    Some(p) => post_view(c, &p, &nav.brand, viewer)?,
                    None => None,
                };
                (item.into_iter().map(|p| Item { post: Some(p), stores: vec![], vault: None }).collect::<Vec<_>>(), String::new(), true)
            }
            None => {
                let (items, next) = build(c, &nav, viewer, Cursor::Top)?;
                (items, next, false)
            }
        };
        let stories = if single { vec![] } else { stories(c, &nav)? };
        let hero = blocks::blocks(c)?.into_iter().find(|b| b.kind == "hero");
        // The hero's words can hold "{count}": the number of cards in the vault.
        let tagline = hero.as_ref().and_then(|b| b.data["subtitle"].as_str()).unwrap_or("").trim().replace("{count}", &db::thousands(admin::in_vault(c)?.0));
        let (top, _) = db::list(c, &db::Query { collection: None, q: String::new(), sort: "price-desc".into(), in_stock: true, page: 0 })?;
        let featured = top.iter().find(|p| p.has_image()).map(product_tile);
        let moves = match c.query_row("SELECT id FROM posts WHERE kind='prices' AND hidden=0 ORDER BY created_at DESC LIMIT 1", [], |r| r.get::<_, i64>(0)) {
            Ok(id) => match social::post(c, 0, id)? {
                Some(p) => post_view(c, &p, &nav.brand, 0)?.map(|v| v.moves).unwrap_or_default(),
                None => vec![],
            },
            Err(_) => vec![],
        };
        let stores = store_views(c, 0)?.into_iter().take(6).collect();
        nav.seo = if single { Seo { title: format!("A post · {}", nav.brand), ..Default::default() } } else { seo(&nav) };
        let end = !single && next.is_empty();
        Ok(FeedPage { nav, stories, items, next, tagline, featured, moves: moves.into_iter().take(4).collect(), stores, single, end })
    })
    .await?;
    render(t)
}

/// A like or a comment changes only the kept copy of the feed (the one signed-out visitors get).
fn stale(app: &App) {
    app.cache.invalidate("fb|feed");
}

/// GET / in the new look.
pub async fn home(app: App, h: HeaderMap) -> Res {
    sync_if_due(&app).await;
    match current(&app, &h).await? {
        Some(m) => Ok(page(feed_html(&app, m.id, None).await?)),
        None => {
            let a = app.clone();
            cached(&app, "feed".into(), || async move { feed_html(&a, 0, None).await }).await
        }
    }
}

#[derive(Deserialize)]
pub struct MoreQs {
    #[serde(default)]
    c: String,
}

/// GET /feed?c=: the next items.
pub async fn more(State(app): State<App>, h: HeaderMap, Query(q): Query<MoreQs>) -> Res {
    let viewer = current(&app, &h).await?.map(|m| m.id).unwrap_or(0);
    let at = q.c.clone();
    let (items, next) = with_db(&app, move |c| {
        let nav = crate::chrome(c)?;
        build(c, &nav, viewer, cursor(&at))
    })
    .await?;
    let end = next.is_empty();
    Ok(html(render(FeedPart { items, next, end })?))
}

/// GET /posts/{id}: one post on its own page (what a shared link opens).
pub async fn one(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>) -> Res {
    if !look::is_new() {
        return Ok(Redirect::to("/").into_response());
    }
    let viewer = current(&app, &h).await?.map(|m| m.id).unwrap_or(0);
    Ok(page(feed_html(&app, viewer, Some(id)).await?))
}

fn part(items: Vec<Item>) -> Result<String, (StatusCode, String)> {
    render(FeedPart { items, next: String::new(), end: false })
}

/// POST /posts (a form: text, photo ×4, vault or card): a trainer's own post. Answers with the post, drawn.
pub async fn create(State(app): State<App>, h: HeaderMap, mut mp: Multipart) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if !m.profile_ok {
        return Ok(fail(StatusCode::FORBIDDEN, "You can post once your trainer is approved."));
    }
    let (id, now) = (m.id, crate::unix_now());
    let (recent, room) = with_db(&app, move |c| Ok((social::posts_since(c, id, now - 3600)?, members::photo_bytes(c)? + dm::media_bytes(c)? + social::photo_bytes(c)?))).await?;
    if recent >= social::POSTS_PER_HOUR {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a lot of posts for one hour. Please take a short break."));
    }
    let (mut text, mut files, mut card, mut as_vault) = (String::new(), Vec::<Vec<u8>>::new(), messages::CardRef::default(), false);
    let bad = |_| (StatusCode::BAD_REQUEST, "That didn't upload. Please try again.".to_string());
    while let Some(f) = mp.next_field().await.map_err(bad)? {
        match f.name().unwrap_or("") {
            "text" => text = f.text().await.map_err(bad)?,
            "photo" => {
                let b = f.bytes().await.map_err(bad)?;
                if !b.is_empty() && files.len() < social::PHOTOS_MAX {
                    files.push(b.to_vec());
                }
            }
            "as" => as_vault = f.text().await.map_err(bad)?.trim() == "vault",
            "vault" => card.vault = f.text().await.map_err(bad)?.trim().parse().ok(),
            "card" => card.card = f.text().await.map_err(bad)?.trim().parse().ok(),
            _ => {}
        }
    }
    let text = members::clean_text(&text, social::TEXT_MAX);
    if text.is_empty() && files.is_empty() && card.vault.is_none() && card.card.is_none() {
        return Ok(fail(StatusCode::BAD_REQUEST, "Write something or add a photo first."));
    }
    if !files.is_empty() && room >= photo_cap_mb() * 1024 * 1024 {
        return Ok(fail(StatusCode::INSUFFICIENT_STORAGE, "The vault's photo space is full right now. Words still work."));
    }
    let mut photos: Vec<social::Photo> = vec![];
    for bytes in files {
        let dir = PathBuf::from(&*app.uploads);
        let stem = format!("p{id}-{}{}", members::temp_password(), members::temp_password());
        match gated(move || store_photo(bytes, dir, stem)).await? {
            Ok((l, s, w, hh, size)) => photos.push((l, s, w, hh, size)),
            Err(e) if e == TOO_BIG => return Ok(fail(StatusCode::BAD_REQUEST, "One photo is too big. Please use a smaller one.")),
            Err(_) => return Ok(fail(StatusCode::BAD_REQUEST, "One photo couldn't be read. Try a JPEG or PNG.")),
        }
    }
    // A full admin may post as the vault itself (the logo and the shop's name), like a Page on Facebook.
    if as_vault && m.role != "admin" {
        return Ok(fail(StatusCode::FORBIDDEN, "Only a full admin can post as the vault."));
    }
    let author = if as_vault { 0 } else { id };
    let brand_viewer = id;
    let out = with_db(&app, move |c| {
        let mut payload = json!({});
        let mut kind = "post";
        if card.vault.is_some() || card.card.is_some() {
            match messages::card_copy(c, &card)? {
                Some((_, copy)) => {
                    payload["card"] = copy;
                    kind = "share";
                }
                None => return Ok(Err("That card isn't here any more.")),
            }
        }
        let pid = social::post_add(c, author, kind, &text, &photos, &payload, now)?;
        let nav = crate::chrome(c)?;
        let p = social::post(c, brand_viewer, pid)?;
        Ok(Ok(match p {
            Some(p) => post_view(c, &p, &nav.brand, brand_viewer)?,
            None => None,
        }))
    })
    .await?;
    app.forget();
    match out {
        Ok(Some(v)) => Ok(html(part(vec![Item { post: Some(v), stores: vec![], vault: None }])?)),
        Ok(None) => Ok(fail(StatusCode::INTERNAL_SERVER_ERROR, "That post didn't save. Please try again.")),
        Err(e) => Ok(fail(StatusCode::BAD_REQUEST, e)),
    }
}

/// May this person change this post's words? The author of a written post; a seller on the post about their own
/// listings; a full admin on anything the vault posted.
fn may_edit(p: &social::Post, me: i64, full: bool) -> bool {
    (p.member_id != 0 && me == p.member_id && (p.written() || p.kind == "listing")) || (full && p.member_id == 0)
}

/// POST /posts/{id}/edit (a form: text, keep = ids of the photos that stay, photo × new ones). Answers with the
/// post, drawn again.
pub async fn edit(State(app): State<App>, h: HeaderMap, Path(pid): Path<i64>, mut mp: Multipart) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let (me, full, now) = (m.id, m.role == "admin", crate::unix_now());
    let Some(p) = with_db(&app, move |c| social::post(c, me, pid)).await?.filter(|p| may_edit(p, me, full)) else {
        return Ok(fail(StatusCode::FORBIDDEN, "That post isn't yours to edit."));
    };
    let (mut text, mut keep, mut files) = (String::new(), Vec::<i64>::new(), Vec::<Vec<u8>>::new());
    let bad = |_| (StatusCode::BAD_REQUEST, "That didn't upload. Please try again.".to_string());
    while let Some(f) = mp.next_field().await.map_err(bad)? {
        match f.name().unwrap_or("") {
            "text" => text = f.text().await.map_err(bad)?,
            "keep" => keep = f.text().await.map_err(bad)?.split(',').filter_map(|x| x.trim().parse().ok()).collect(),
            "photo" => {
                let b = f.bytes().await.map_err(bad)?;
                if !b.is_empty() && files.len() < social::PHOTOS_MAX {
                    files.push(b.to_vec());
                }
            }
            _ => {}
        }
    }
    let text = members::clean_text(&text, social::TEXT_MAX);
    let written = p.written();
    let mut photos: Vec<social::Photo> = vec![];
    if written {
        let has_card = p.payload["card"]["title"].is_string();
        files.truncate(social::PHOTOS_MAX.saturating_sub(keep.len()));
        if text.is_empty() && keep.is_empty() && files.is_empty() && !has_card {
            return Ok(fail(StatusCode::BAD_REQUEST, "A post needs words or a photo. Delete it instead?"));
        }
        for bytes in files {
            let dir = PathBuf::from(&*app.uploads);
            let stem = format!("p{}-{}{}", p.member_id, members::temp_password(), members::temp_password());
            match gated(move || store_photo(bytes, dir, stem)).await? {
                Ok((l, s, w, hh, size)) => photos.push((l, s, w, hh, size)),
                Err(e) if e == TOO_BIG => return Ok(fail(StatusCode::BAD_REQUEST, "One photo is too big. Please use a smaller one.")),
                Err(_) => return Ok(fail(StatusCode::BAD_REQUEST, "One photo couldn't be read. Try a JPEG or PNG.")),
            }
        }
    }
    let (gone, view) = with_db(&app, move |c| {
        let gone = if written {
            social::post_edit(c, pid, &text, &keep, &photos, now)?
        } else {
            social::post_note(c, pid, &text, now)?;
            vec![]
        };
        let nav = crate::chrome(c)?;
        let view = match social::post(c, me, pid)? {
            Some(p) => post_view(c, &p, &nav.brand, me)?,
            None => None,
        };
        Ok((gone, view))
    })
    .await?;
    for f in gone {
        admin::remove_upload(&app.uploads, &f);
    }
    app.forget();
    match view {
        Some(v) => Ok(html(part(vec![Item { post: Some(v), stores: vec![], vault: None }])?)),
        None => Ok(fail(StatusCode::INTERNAL_SERVER_ERROR, "That didn't save. Please try again.")),
    }
}

#[derive(Deserialize)]
pub struct CommentEdit {
    #[serde(default)]
    text: String,
}

/// POST /comments/{id}/edit {text}: your own comment.
pub async fn comment_edit(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<CommentEdit>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let text = members::clean_text(&req.text, social::COMMENT_MAX);
    if text.is_empty() {
        return Ok(fail(StatusCode::BAD_REQUEST, "Write something first."));
    }
    let (me, now) = (m.id, crate::unix_now());
    let staff = admin::power(&app, &h) != admin::Power::Nothing;
    let out = with_db(&app, move |c| {
        let Some(k) = social::comment(c, id)?.filter(|k| k.member_id == me) else { return Ok(None) };
        social::comment_edit(c, k.id, &text, now)?;
        social::comment(c, id)
    })
    .await?;
    Ok(match out {
        Some(k) => ok(json!({ "ok": true, "comment": comment_json(&k, me, staff) })),
        None => fail(StatusCode::FORBIDDEN, "That comment isn't yours."),
    })
}

#[derive(Deserialize)]
pub struct ReactReq {
    #[serde(default)]
    kind: String,
}

/// POST /posts/{id}/react {kind}: like, another reaction, or "" to take it back.
pub async fn react(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<ReactReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if !req.kind.is_empty() && !social::reaction_ok(&req.kind) {
        return Ok(fail(StatusCode::BAD_REQUEST, "That reaction isn't one of ours."));
    }
    let (me, name, now, kind) = (m.id, m.name.clone(), crate::unix_now(), req.kind.clone());
    let out = with_db(&app, move |c| {
        let Some(n) = social::react(c, id, me, &kind, now)? else { return Ok(None) };
        // The author hears about it: one line per post, brought up to date instead of repeated.
        if !kind.is_empty() {
            if let Some(p) = social::post(c, 0, id)?.filter(|p| p.member_id != 0 && p.member_id != me) {
                let link = format!("/posts/{id}");
                let open: Option<i64> = rusqlite::OptionalExtension::optional(c.query_row("SELECT id FROM notifications WHERE member_id=?1 AND kind='social' AND link=?2 AND is_read=0 AND message LIKE '%reacted%' LIMIT 1", rusqlite::params![p.member_id, link], |r| r.get(0)))?;
                match open {
                    Some(nid) if n > 1 => {
                        c.execute("UPDATE notifications SET message=?2, created_at=?3 WHERE id=?1", rusqlite::params![nid, format!("{name} and {} other{} reacted to your post.", n - 1, if n == 2 { "" } else { "s" }), now])?;
                    }
                    Some(_) => {}
                    None => sellers::notify(c, p.member_id, "social", &format!("{name} reacted {} to your post.", social::emoji(&kind)), &link)?,
                }
            }
        }
        let tops: Vec<&'static str> = social::top_reactions(c, id)?.iter().map(|k| social::emoji(k)).collect();
        Ok(Some((n, tops)))
    })
    .await?;
    stale(&app);
    Ok(match out {
        Some((n, tops)) => ok(json!({ "ok": true, "n": n, "mine": req.kind, "emoji": social::emoji(&req.kind), "tops": tops })),
        None => fail(StatusCode::NOT_FOUND, "That post isn't here any more."),
    })
}

fn comment_json(k: &social::Comment, viewer: i64, staff: bool) -> Value {
    json!({ "edited": k.edited_at > 0, "id": k.id, "parent": k.parent_id, "member": k.member_id, "name": k.name, "avatar": k.avatar, "initial": members::initial(&k.name), "role": k.role, "text": k.text, "at": k.created_at, "when": members::ago(k.created_at), "own": viewer == k.member_id, "can_remove": viewer == k.member_id || staff })
}

/// GET /posts/{id}/comments
pub async fn comments(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>) -> Res {
    let viewer = current(&app, &h).await?.map(|m| m.id).unwrap_or(0);
    let staff = admin::power(&app, &h) != admin::Power::Nothing;
    let list = with_db(&app, move |c| social::comments(c, id, 200)).await?;
    Ok(ok(json!({ "comments": list.iter().map(|k| comment_json(k, viewer, staff)).collect::<Vec<_>>() })))
}

#[derive(Deserialize)]
pub struct CommentReq {
    #[serde(default)]
    text: String,
    parent: Option<i64>,
}

/// POST /posts/{id}/comments {text, parent?}
pub async fn comment(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<CommentReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let text = members::clean_text(&req.text, social::COMMENT_MAX);
    if text.is_empty() {
        return Ok(fail(StatusCode::BAD_REQUEST, "Write something first."));
    }
    let (me, name, now, parent) = (m.id, m.name.clone(), crate::unix_now(), req.parent);
    let staff = admin::power(&app, &h) != admin::Power::Nothing;
    let out = with_db(&app, move |c| {
        if social::comments_since(c, me, now - 3600)? >= social::COMMENTS_PER_HOUR {
            return Ok(Err("That's a lot of comments for one hour. Please take a short break."));
        }
        let Some(cid) = social::comment_add(c, id, me, parent, &text, now)? else { return Ok(Err("That post isn't here any more.")) };
        let short: String = if text.chars().count() > 80 { format!("{}…", text.chars().take(79).collect::<String>()) } else { text.clone() };
        let link = format!("/posts/{id}");
        let mut told = vec![me];
        if let Some(p) = social::post(c, 0, id)? {
            if p.member_id != 0 && !told.contains(&p.member_id) {
                sellers::notify(c, p.member_id, "social", &format!("{name} commented on your post: “{short}”"), &link)?;
                told.push(p.member_id);
            }
        }
        if let Some(k) = parent.and_then(|pid| social::comment(c, pid).ok().flatten()) {
            if !told.contains(&k.member_id) {
                sellers::notify(c, k.member_id, "social", &format!("{name} replied to your comment: “{short}”"), &link)?;
            }
        }
        let n: i64 = c.query_row("SELECT comment_n FROM posts WHERE id=?1", [id], |r| r.get(0))?;
        Ok(Ok((social::comment(c, cid)?, n)))
    })
    .await?;
    stale(&app);
    Ok(match out {
        Ok((Some(k), n)) => ok(json!({ "ok": true, "comment": comment_json(&k, me, staff), "n": n })),
        Ok((None, _)) => fail(StatusCode::INTERNAL_SERVER_ERROR, "That comment didn't save. Please try again."),
        Err(e) => fail(StatusCode::BAD_REQUEST, e),
    })
}

/// POST /comments/{id}/delete: your own comment, or any comment for staff.
pub async fn comment_remove(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>) -> Res {
    if !same_site(&h) {
        return Ok(fail(StatusCode::FORBIDDEN, "That request didn't come from this site."));
    }
    let me = current(&app, &h).await?.map(|m| m.id).unwrap_or(0);
    let staff = admin::power(&app, &h) != admin::Power::Nothing;
    let out = with_db(&app, move |c| {
        let Some(k) = social::comment(c, id)? else { return Ok(None) };
        if !(staff || (me != 0 && me == k.member_id)) {
            return Ok(Some(Err(())));
        }
        let post = social::comment_remove(c, id)?.unwrap_or(k.post_id);
        let n: i64 = rusqlite::OptionalExtension::optional(c.query_row("SELECT comment_n FROM posts WHERE id=?1", [post], |r| r.get(0)))?.unwrap_or(0);
        Ok(Some(Ok(n)))
    })
    .await?;
    stale(&app);
    Ok(match out {
        Some(Ok(n)) => ok(json!({ "ok": true, "n": n })),
        Some(Err(())) => fail(StatusCode::FORBIDDEN, "That comment isn't yours."),
        None => ok(json!({ "ok": true, "n": 0 })),
    })
}

/// POST /posts/{id}/delete: the author deletes their own post; staff take any post out of the feed.
/// (An automatic post is hidden, not deleted: the site would only write it again.)
pub async fn remove(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>) -> Res {
    if !same_site(&h) {
        return Ok(fail(StatusCode::FORBIDDEN, "That request didn't come from this site."));
    }
    let me = current(&app, &h).await?.map(|m| m.id).unwrap_or(0);
    let staff = admin::power(&app, &h) != admin::Power::Nothing;
    let out = with_db(&app, move |c| {
        let Some(p) = social::post(c, 0, id)? else { return Ok(Some(vec![])) };
        let written = p.kind == "post" || p.kind == "share";
        if written && (staff || (me != 0 && me == p.member_id)) {
            // (Staff includes the full admins, who own what the vault itself posted.)
            return Ok(Some(social::post_remove(c, id)?));
        }
        if staff {
            // 2: hidden by a person, so the automatic refresh never brings it back.
            c.execute("UPDATE posts SET hidden=2 WHERE id=?1", [id])?;
            return Ok(Some(vec![]));
        }
        Ok(None)
    })
    .await?;
    let Some(files) = out else { return Ok(fail(StatusCode::FORBIDDEN, "That post isn't yours.")) };
    for f in files {
        admin::remove_upload(&app.uploads, &f);
    }
    app.forget();
    Ok(ok(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct AutoReq {
    kind: String,
    on: bool,
}

/// POST /admin/feed/auto {kind, on}: full admins switch one kind of automatic post on or off.
pub async fn auto(State(app): State<App>, h: HeaderMap, Json(req): Json<AutoReq>) -> Res {
    if !same_site(&h) || !matches!(admin::power(&app, &h), admin::Power::Admin(_)) {
        return Ok(fail(StatusCode::FORBIDDEN, "Only a full admin can switch automatic posts."));
    }
    let now = crate::unix_now();
    let done = with_db(&app, move |c| social::set_auto(c, &req.kind, req.on, now)).await?;
    app.forget();
    Ok(if done { ok(json!({ "ok": true })) } else { fail(StatusCode::BAD_REQUEST, "There's no automatic post of that kind.") })
}

#[derive(Deserialize)]
pub struct PinReq {
    on: bool,
}

/// POST /posts/{id}/pin {on}: full admins keep a post at the top of the feed.
pub async fn pin(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<PinReq>) -> Res {
    if !same_site(&h) || !matches!(admin::power(&app, &h), admin::Power::Admin(_)) {
        return Ok(fail(StatusCode::FORBIDDEN, "Only a full admin can pin a post."));
    }
    with_db(&app, move |c| social::set_pinned(c, id, req.on)).await?;
    app.forget();
    Ok(ok(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_knows_where_it_starts() {
        assert!(matches!(cursor(""), Cursor::Top));
        assert!(matches!(cursor("p:1794400000:12"), Cursor::After(1_794_400_000, 12)));
        assert!(matches!(cursor("v:7"), Cursor::Vault(7)));
        assert!(matches!(cursor("p:x:y"), Cursor::Top));
        assert!(matches!(cursor("v:99999999999"), Cursor::Vault(100_000)), "a silly page number is capped");
    }
}
