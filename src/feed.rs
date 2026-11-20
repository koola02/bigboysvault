//! The home feed: posts from marcuru_rs::social, drawn with live cards, ten or so at a time. No prices: the owner
//! (18 Nov 2026) wants prices only where cards are shopped (stores, their binders, the search, a card's window).
//! It never ends: when the posts run out, the feed goes on with sets of cards from the stores' binders, the stores
//! taking turns (the vault's store is one of them, like any other), round and round. Between posts sit the stores
//! carousel and, at the very top, the showcase as a row of stories.
//!
//! GET /           the feed page (signed-out visitors get a kept copy; signed-in ones see their own reactions)
//! GET /feed?c=    the next items, as HTML (the page's last element asks for them when it scrolls into view)
//! POST /posts     a trainer's post (words, up to four photos, maybe a card)
//! POST /posts/{id}/react · GET|POST /posts/{id}/comments · POST /posts/{id}/delete|hide · POST /comments/{id}/delete
//! GET /posts/{id}/photos/{image}   one photo of a written post with its own likes and comments (the photo window)
use crate::community::{acting, current, fail, gated, html, ok, over_limit, photo_cap_mb, render, same_site, store_photo, TOO_BIG};
use crate::{admin, cached, messages, page, with_db, App, Chrome, Res, Seo};
use askama::Template;
use axum::{
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use marcuru_rs::{blocks, db, dm, members, rich, sellers, social};
use marcuru_rs::members::MemberCard;
use crate::store::StoreCard;
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

#[derive(Clone)]
pub struct Tile {
    pub title: String,
    pub img: String,
    pub url: String,
    pub cond: String,
    pub c: String,
    pub sub: String,
}

pub struct PostView {
    pub id: i64,
    pub kind: String,
    pub who: Who,
    pub headline: String,
    pub when: String,
    pub text: String,
    /// The same words as HTML when any are formatted (bold, italic, underline, a size); "" shows `text` as it is.
    pub rich: String,
    pub tiles: Vec<Tile>,
    /// Cards beyond the ones shown ("+12" on the last picture).
    pub more: i64,
    pub link: String,
    pub link_text: String,
    /// (photo id, full size, small). The id is 0 for a vouch's photos, which are edited on the vouch itself.
    pub photos: Vec<(i64, String, String)>,
    /// Each photo's caption, in the same order ("" for none).
    pub caps: Vec<String>,
    pub member: i64,
    pub edited: bool,
    pub attach: Option<Tile>,
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
    /// Its photos each have their own likes and comments (a written post with two or more): opening one shows that
    /// photo's, not the post's.
    pub photo_threads: bool,
    /// This is one photo of post `id`, drawn as a small post of its own for the photo window (`post_images.id`; 0 for
    /// a post), and which one it is ("Photo 2 Of 3").
    pub image: i64,
    pub image_at: String,
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
    /// The caption of photo `i` ("" when it has none).
    pub fn cap(&self, i: &usize) -> &str {
        self.caps.get(*i).map(String::as_str).unwrap_or("")
    }
    pub fn comments_text(&self) -> String {
        format!("{} Comment{}", self.comment_n, if self.comment_n == 1 { "" } else { "s" })
    }
}

/// A store in the stores carousel and in the right column's list. `id` 0 is the vault's store.
#[derive(Clone)]
pub struct StoreView {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub avatar: String,
    pub initial: String,
    pub pic: String,
    pub sub: String,
}

/// A set of cards from one store's binder, after the posts: "From {store} · {binder}".
pub struct BinderUnit {
    pub title: String,
    pub url: String,
    pub tiles: Vec<Tile>,
    pub total: i64,
}

pub struct Item {
    pub post: Option<PostView>,
    pub stores: Vec<StoreView>,
    pub binder: Option<BinderUnit>,
}

/// One store as the feed shows it, the vault's like any other: in the stores carousel, in the right column's list,
/// and its binders in the sets of cards after the posts.
struct Shop {
    view: StoreView,
    shelves: Vec<Shelf>,
}

/// One of a store's public binders with cards for sale. After the posts, the feed shows sets of its cards.
struct Shelf {
    /// "From Big Boys Vault · Pikachu Set"
    title: String,
    /// The binder's own page.
    url: String,
    /// How many cards its page shows.
    total: i64,
    cards: ShelfCards,
}

enum ShelfCards {
    /// A vault binder, by its handle: its cards are read a page at a time when a set is shown.
    Vault(String),
    /// A seller's binder: its cards for sale (with a picture), in the binder's order.
    Seller(Vec<Tile>),
}

/// How many cards one set shows.
const SET: usize = 8;

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
    Tile { title: p.short_title(), img: p.img(360), url: format!("/products/{}", p.handle), cond: p.tile_cond(), c: p.cond_class().to_string(), sub: p.set_text() }
}

/// A seller's card for sale. Its link opens it in the card window.
fn listing_tile(seller: i64, k: &MemberCard) -> Tile {
    Tile { title: k.title.clone(), img: k.image.clone(), url: format!("/trainers/{seller}/cards/{}", k.id), c: k.cond_class().to_string(), cond: k.cond.clone(), sub: k.card_set.clone() }
}

/// Who wrote a post (0: the vault itself). `store`: the post is about a store's cards, so the name is the store's and
/// it opens the store, the vault's (/store/vault) as any seller's.
fn who(c: &rusqlite::Connection, id: i64, brand: &str, store: bool) -> rusqlite::Result<Option<Who>> {
    if id == 0 {
        let url = if store { format!("/store/{}", sellers::VAULT_SLUG) } else { "/".into() };
        return Ok(Some(Who { name: brand.to_string(), avatar: "/static/img/logo-sm-144.png?v=1".into(), initial: "B".into(), url }));
    }
    Ok(members::get(c, id)?.filter(|m| !m.blocked).map(|m| Who {
        name: if store { m.store_title() } else { m.name.clone() },
        avatar: if store { m.store_pic().to_string() } else { m.avatar.clone() },
        initial: m.initial(),
        url: if store { m.store_url() } else { format!("/trainers/{}", m.id) },
    }))
}

/// One post, drawn from what the site holds right now. None: nothing left to show (cards sold, trainer gone).
fn post_view(c: &rusqlite::Connection, p: &social::Post, brand: &str, viewer: i64) -> rusqlite::Result<Option<PostView>> {
    let Some(author) = who(c, p.member_id, brand, matches!(p.kind.as_str(), "listing" | "arrivals"))? else { return Ok(None) };
    // An automatic post shows the words its seller or an admin added, if any.
    let text = if p.written() || p.kind == "vouch" { p.text.clone() } else { p.note.clone() };
    let mut v = PostView {
        id: p.id,
        kind: p.kind.clone(),
        who: author,
        headline: String::new(),
        when: members::ago(p.created_at),
        rich: if p.kind == "vouch" { String::new() } else { rich::html_for(&p.rich, &text) },
        text,
        tiles: vec![],
        more: 0,
        link: String::new(),
        link_text: String::new(),
        photos: vec![],
        caps: vec![],
        member: p.member_id,
        edited: p.edited_at > 0,
        attach: None,
        people: vec![],
        like_n: p.like_n,
        comment_n: p.comment_n,
        mine: p.mine.clone(),
        tops: social::top_reactions(c, p.id)?.iter().map(|k| social::emoji(k)).collect(),
        pinned: p.pinned,
        own: viewer != 0 && viewer == p.member_id && (p.kind == "post" || p.kind == "share"),
        small: false,
        photo_threads: false,
        image: 0,
        image_at: String::new(),
    };
    let ids = |key: &str| -> Vec<i64> { p.payload[key].as_array().map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default() };
    match p.kind.as_str() {
        // The vault's new cards read like any store's listings: its name opens its store, and so does the link.
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
            v.headline = if count == 1 { "listed a card for sale".into() } else { format!("listed {} cards for sale", db::thousands(count)) };
            v.tiles.truncate(4);
            v.more = count - v.tiles.len() as i64;
            v.link = v.who.url.clone();
            v.link_text = "Visit The Store".into();
        }
        "listing" => {
            for id in ids("cards") {
                if let Some(mc) = members::card(c, p.member_id, id)?.filter(|x| x.kind == "sale" && x.status == "Active") {
                    v.tiles.push(listing_tile(p.member_id, &mc));
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
            v.link_text = "Visit The Store".into();
        }
        "vouch" => {
            v.headline = format!("vouched for {brand}");
            if let Some(vid) = p.payload["vouch"].as_i64() {
                // A vouch is for one store (0, the vault's, is the site's own name).
                let store: Option<String> = rusqlite::OptionalExtension::optional(c
                    .query_row(
                        "SELECT CASE WHEN TRIM(s.store_name)<>'' THEN TRIM(s.store_name) ELSE s.name || '''s store' END FROM vouches v JOIN members s ON s.id=v.store_id WHERE v.id=?1 AND v.store_id<>0",
                        [vid],
                        |r| r.get(0),
                    ))?;
                if let Some(s) = store {
                    v.headline = format!("vouched for {s}");
                }
                v.photos = c.prepare_cached("SELECT path, thumb FROM vouch_images WHERE vouch_id=?1 ORDER BY id LIMIT 4")?.query_map([vid], |r| Ok((0, r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
            }
        }
        // Price moves left the feed on 18 Nov 2026 (prices only where cards are shopped). Old ones are hidden by
        // social::migrate; this keeps any left from being drawn.
        "prices" => return Ok(None),
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
            v.caps = social::photo_captions(c, p.id)?;
            v.photo_threads = social::photos_have_their_own(c, p, v.photos.len())?;
            let card = &p.payload["card"];
            if let Some(title) = card["title"].as_str() {
                let s = |k: &str| card[k].as_str().unwrap_or("").to_string();
                v.attach = Some(Tile { title: title.to_string(), img: s("image"), url: s("url"), c: vault_search::cond_class(&s("cond")).to_string(), cond: s("cond"), sub: s("set") });
                v.headline = "shared a card".into();
            }
        }
    }
    Ok(Some(v))
}

/// Every store on the Stores page, in its order (the one that last put up or changed a card for sale first,
/// `store::store_cards`): the vault's and the trainers', each worked out the same way.
fn shops(c: &rusqlite::Connection, nav: &Chrome) -> rusqlite::Result<Vec<Shop>> {
    let mut out = vec![];
    for s in crate::store::store_cards(c, &nav.brand)? {
        out.push(if s.owner == 0 { vault_shop(c, &nav.links, &s)? } else { seller_shop(c, &s)? });
    }
    Ok(out)
}

/// A store's tile in the carousel: its first card for sale (`pic`, `title`; no picture: its cover photo). No price.
fn store_view(s: &StoreCard, pic: String, title: &str) -> StoreView {
    StoreView {
        id: s.owner,
        name: s.name.clone(),
        url: s.url.clone(),
        avatar: s.avatar.clone(),
        initial: s.initial.clone(),
        pic: if pic.is_empty() { s.cover.clone() } else { pic },
        sub: format!("{} On Sale{}", s.on_sale_text(), if title.is_empty() { String::new() } else { format!(" · {title}") }),
    }
}

/// The vault's store: its first card for sale, and every binder with cards for sale in the owner's order (as its store
/// page shows them).
fn vault_shop(c: &rusqlite::Connection, links: &[db::Collection], s: &StoreCard) -> rusqlite::Result<Shop> {
    let first: Option<i64> = rusqlite::OptionalExtension::optional(c.query_row("SELECT id FROM products WHERE available=1 AND image<>'' ORDER BY position, created_at DESC, id DESC LIMIT 1", [], |r| r.get(0)))?;
    let view = match first.map(|id| db::product_by_id(c, id)).transpose()?.flatten() {
        Some(p) => store_view(s, p.img(360), &p.short_title()),
        None => store_view(s, String::new(), ""),
    };
    // A binder has a set in the feed while one of its cards for sale has a picture.
    let peeks = db::binder_peeks(c, 1)?;
    let shelves = links
        .iter()
        .filter(|b| b.is_public() && peeks.get(&b.handle).is_some_and(|(_, pics)| !pics.is_empty()))
        .map(|b| Shelf { title: format!("From {} · {}", s.name, b.title), url: format!("/collections/{}", b.handle), total: b.count, cards: ShelfCards::Vault(b.handle.clone()) })
        .collect();
    Ok(Shop { view, shelves })
}

/// A trainer's store: its first card for sale, and its public binders with cards for sale now, in the store's order.
fn seller_shop(c: &rusqlite::Connection, s: &StoreCard) -> rusqlite::Result<Shop> {
    let listings = sellers::storefront_listings(c, s.owner)?;
    let view = match listings.first() {
        Some(k) => store_view(s, k.image.clone(), &k.title),
        None => store_view(s, String::new(), ""),
    };
    let mut shelves = vec![];
    for b in members::binders_of(c, s.owner)?.into_iter().filter(|b| b.visibility == "Public") {
        let tiles: Vec<Tile> = listings.iter().filter(|k| k.binder_id == Some(b.id) && k.status == "Active" && !k.image.is_empty()).map(|k| listing_tile(s.owner, k)).collect();
        if tiles.is_empty() {
            continue;
        }
        // Its page lists its cards on sale, reserved or out of stock: the number its tile in the store says.
        let total = listings.iter().filter(|k| k.binder_id == Some(b.id)).count() as i64;
        shelves.push(Shelf { title: format!("From {} · {}", s.name, b.title), url: format!("{}/binders/{}", s.url, b.slug), total, cards: ShelfCards::Seller(tiles) });
    }
    Ok(Shop { view, shelves })
}

/// The stores for a carousel: the Stores page's order, starting `turn` stores in (each later carousel starts further on).
fn store_views(shops: &[Shop], turn: usize) -> Vec<StoreView> {
    let mut out: Vec<StoreView> = shops.iter().map(|s| s.view.clone()).collect();
    if !out.is_empty() {
        let k = turn % out.len();
        out.rotate_left(k);
    }
    out
}

/// The binders in turn, store by store: the first binder of each store, then the second of each, and so on. So the sets
/// after the posts take turns between the stores, and every binder comes up once before any comes up again.
fn in_turn(shops: &[Shop]) -> Vec<&Shelf> {
    let most = shops.iter().map(|s| s.shelves.len()).max().unwrap_or(0);
    (0..most).flat_map(|k| shops.iter().filter_map(move |s| s.shelves.get(k))).collect()
}

/// `n` of `all` from `start` on, going round to the first ones after the last (never one twice).
fn ring<T: Clone>(all: &[T], start: usize, n: usize) -> Vec<T> {
    if all.is_empty() {
        return vec![];
    }
    all.iter().cycle().skip(start % all.len()).take(n.min(all.len())).cloned().collect()
}

/// A vault binder's set of cards the `round`-th time it comes up: the next cards each time, the first ones again after
/// the last. A big binder is read a page at a time, each page going round before the next.
fn vault_set(c: &rusqlite::Connection, handle: &str, round: usize) -> rusqlite::Result<Vec<Tile>> {
    let page = |n: usize| db::list(c, &db::Query { collection: Some(handle.to_string()), in_stock: true, page: n, ..Default::default() });
    let (first, found) = page(0)?;
    let pages = usize::try_from(found).unwrap_or(0).div_ceil(db::PAGE_SIZE).max(1);
    let start = round * SET;
    let (items, at) = match (start / db::PAGE_SIZE) % pages {
        _ if pages == 1 => (first, start),
        0 => (first, start % db::PAGE_SIZE),
        k => (page(k)?.0, start % db::PAGE_SIZE),
    };
    let tiles: Vec<Tile> = items.iter().filter(|p| p.has_image()).map(product_tile).collect();
    Ok(ring(&tiles, at, SET))
}

/// The n-th set of cards after the posts: the binders in turn (`in_turn`), then round again with the next cards of each.
fn binder_unit(c: &rusqlite::Connection, shelves: &[&Shelf], n: usize) -> rusqlite::Result<Option<BinderUnit>> {
    if shelves.is_empty() {
        return Ok(None);
    }
    let s = shelves[n % shelves.len()];
    let round = n / shelves.len();
    let tiles = match &s.cards {
        ShelfCards::Vault(handle) => vault_set(c, handle, round)?,
        ShelfCards::Seller(tiles) => ring(tiles, round * SET, SET),
    };
    Ok((!tiles.is_empty()).then(|| BinderUnit { title: s.title.clone(), url: s.url.clone(), tiles, total: s.total }))
}

/// Up to `want` sets from the n-th on, and the number the next page starts at. A binder with nothing to show is passed
/// over; when a whole turn of the binders finds nothing, there are none (and the feed ends instead of asking forever).
fn sets(c: &rusqlite::Connection, shelves: &[&Shelf], n: usize, want: usize) -> rusqlite::Result<(Vec<BinderUnit>, usize)> {
    let (mut out, mut k) = (vec![], n);
    while out.len() < want && k < n + want + shelves.len() {
        if let Some(u) = binder_unit(c, shelves, k)? {
            out.push(u);
        }
        k += 1;
    }
    Ok((out, k))
}

/// The stores, worked out at most once for a page of the feed, and only when the page shows them.
struct Shops<'a> {
    c: &'a rusqlite::Connection,
    nav: &'a Chrome,
    list: Option<Vec<Shop>>,
}

impl<'a> Shops<'a> {
    fn new(c: &'a rusqlite::Connection, nav: &'a Chrome) -> Self {
        Shops { c, nav, list: None }
    }
    fn all(&mut self) -> rusqlite::Result<&[Shop]> {
        if self.list.is_none() {
            self.list = Some(shops(self.c, self.nav)?);
        }
        Ok(self.list.as_deref().unwrap_or_default())
    }
}

/// Where a page of the feed starts: the top, after a post (time, id), or at the n-th set of cards from the binders.
/// (A set's address is still "v:n", from when the sets were the vault's only, so links already out keep working.)
enum Cursor {
    Top,
    After(i64, i64),
    Binders(usize),
}

fn cursor(s: &str) -> Cursor {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.as_slice() {
        ["p", t, id] => match (t.parse(), id.parse()) {
            (Ok(t), Ok(id)) => Cursor::After(t, id),
            _ => Cursor::Top,
        },
        ["v", n] => Cursor::Binders(n.parse::<usize>().unwrap_or(0).min(100_000)),
        _ => Cursor::Top,
    }
}

/// One page of the feed and where the next one starts. Nothing: the feed is over (the admins switched the sets off, or
/// no store has a binder to show).
fn build(c: &rusqlite::Connection, nav: &Chrome, viewer: i64, at: Cursor, shops: &mut Shops<'_>) -> rusqlite::Result<(Vec<Item>, String)> {
    let post_item = |p: PostView| Item { post: Some(p), stores: vec![], binder: None };
    let set_item = |u: BinderUnit| Item { post: None, stores: vec![], binder: Some(u) };
    let mut items: Vec<Item> = vec![];
    // (The admins' switch for the sets is called "vault": once, they were the vault's only.)
    let sets_on = !social::auto_off(c)?.iter().any(|k| k == "vault");
    let (before, top) = match at {
        Cursor::Binders(_) if !sets_on => return Ok((items, String::new())),
        Cursor::Binders(n) => {
            let all = shops.all()?;
            let (units, next) = sets(c, &in_turn(all), n, 2)?;
            if units.is_empty() {
                return Ok((items, String::new()));
            }
            items.extend(units.into_iter().map(set_item));
            // The stores carousel comes back every six sets or so, starting further along each time.
            if n / 6 != next / 6 {
                let stores = store_views(all, next / 6);
                if !stores.is_empty() {
                    items.insert(1.min(items.len()), Item { post: None, stores, binder: None });
                }
            }
            return Ok((items, format!("v:{next}")));
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
                let stores = store_views(shops.all()?, 0);
                if !stores.is_empty() {
                    items.push(Item { post: None, stores, binder: None });
                }
            }
        }
    }
    let next = match (full, last) {
        (true, Some((t, id))) => format!("p:{t}:{id}"),
        _ if !sets_on => String::new(),
        _ => {
            // The posts are done: the stores' binders take over, so the page never ends.
            let (units, next) = sets(c, &in_turn(shops.all()?), 0, 1)?;
            if units.is_empty() {
                String::new()
            } else {
                items.extend(units.into_iter().map(set_item));
                format!("v:{next}")
            }
        }
    };
    Ok((items, next))
}

/// The row of stories at the top is the showcase and nothing else: the cards the owner hand-picked for it, in the
/// owner's order. (Binders and the home page's other rows have their own places; they are not stories.)
fn stories(c: &rusqlite::Connection) -> rusqlite::Result<Vec<Story>> {
    let mut out: Vec<Story> = vec![];
    // One section is the showcase (the admin's Showcase tool edits the same one), and only while it is switched on.
    if !blocks::showcase(c)?.is_some_and(|b| b.visible) {
        return Ok(out);
    }
    for id in blocks::showcase_ids(c)? {
        let Some(p) = db::product_by_id(c, id)?.filter(|p| p.available && p.has_image()) else { continue };
        let url = format!("/products/{}", p.handle);
        if !out.iter().any(|s| s.url == url) {
            out.push(Story { title: p.short_title(), img: p.img(360), url });
        }
    }
    out.truncate(blocks::SHOWCASE_MAX);
    Ok(out)
}

/// The automatic posts are brought up to date at most every five minutes, when someone looks at the feed.
async fn sync_if_due(app: &App) {
    let now = crate::unix_now();
    let last = SYNCED.load(Ordering::Relaxed);
    if now - last < 300 || SYNCED.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_err() {
        return;
    }
    if let Err(e) = with_db(app, move |c| {
        for store in social::sync(c, now)? {
            tell_followers(c, store, now)?;
        }
        Ok(())
    })
    .await
    {
        tracing::warn!("feed sync: {}", e.1);
    }
}

/// A store just put up cards (`store` 0 is the vault's): whoever follows it hears, one line a day for each store (the
/// owner's plan, 18 Nov 2026: following a store was a dead end for the follower).
fn tell_followers(c: &rusqlite::Connection, store: i64, now: i64) -> rusqlite::Result<()> {
    let (name, url) = if store == 0 {
        (blocks::setting(c, "brand")?.filter(|b| !b.trim().is_empty()).unwrap_or_else(|| "Big Boys Vault".into()), format!("/store/{}", sellers::VAULT_SLUG))
    } else {
        match members::get(c, store)?.filter(|m| m.seller && !m.blocked && !m.store_hidden && m.profile_ok) {
            Some(m) => (m.store_title(), m.store_url()),
            None => return Ok(()),
        }
    };
    let day = (now + 8 * 3600) / 86_400;
    for f in sellers::follower_ids(c, store)?.into_iter().filter(|f| *f != store) {
        sellers::notify_from(c, f, store, "following", &format!("following:{store}:{day}"), &format!("{name} put up new cards."), &url)?;
    }
    Ok(())
}

/// A new post: whoever follows its author (the vault's store, for a post as the vault) hears, one unread line per
/// author kept up to date. `poster` is who wrote it (an admin, for the vault): they aren't told about their own post.
fn tell_post_followers(c: &rusqlite::Connection, author: i64, poster: i64, pid: i64, text: &str, brand: &str) -> rusqlite::Result<()> {
    let who_name = if author == 0 { brand.to_string() } else { members::get(c, author)?.map(|x| x.name).unwrap_or_default() };
    for f in sellers::follower_ids(c, author)?.into_iter().filter(|f| *f != poster && *f != author) {
        sellers::notify_from(c, f, author, "following", &format!("posts:{author}"), &format!("{who_name} posted: {}", said(text)), &format!("/posts/{pid}"))?;
    }
    Ok(())
}

/// Who hears about a reaction or a comment on a post: whoever wrote it, or for a post the vault itself wrote, its admins.
fn post_owners(c: &rusqlite::Connection, p: &social::Post) -> rusqlite::Result<Vec<i64>> {
    if p.member_id != 0 { Ok(vec![p.member_id]) } else { dm::admins(c) }
}

fn seo(nav: &Chrome) -> Seo {
    let site = nav.site.clone();
    Seo {
        title: format!("{} · Pokémon cards, graded slabs & collectibles in the Philippines", nav.brand),
        desc: crate::clip(&format!("{}: Pokémon cards and collectibles in one vault, priced in pesos. New arrivals, trainers' stores and vouches, every day.", nav.brand), 158),
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
        let mut shops = Shops::new(c, &nav);
        let (items, next, single) = match only {
            Some(id) => {
                let item = match social::post(c, viewer, id)?.filter(|p| !p.hidden) {
                    Some(p) => post_view(c, &p, &nav.brand, viewer)?,
                    None => None,
                };
                (item.into_iter().map(|p| Item { post: Some(p), stores: vec![], binder: None }).collect::<Vec<_>>(), String::new(), true)
            }
            None => {
                let (items, next) = build(c, &nav, viewer, Cursor::Top, &mut shops)?;
                (items, next, false)
            }
        };
        let stories = if single { vec![] } else { stories(c)? };
        // The welcome line can hold "{count}": the number of cards for sale on the site.
        let tagline = blocks::welcome(c)?.replace("{count}", &db::thousands(cards_for_sale(c)?));
        let stores = store_views(shops.all()?, 0).into_iter().take(6).collect();
        nav.seo = if single { Seo { title: format!("A Post · {}", nav.brand), ..Default::default() } } else { seo(&nav) };
        let end = !single && next.is_empty();
        Ok(FeedPage { nav, stories, items, next, tagline, stores, single, end })
    })
    .await?;
    render(t)
}

/// Every card for sale on the site: the vault's copies, and the sellers' cards The Vault page lists (`sellers::open_count`:
/// approved, not blocked, selling, not on vacation, not past its end date, a copy left). The home page's welcome line
/// says it where its owner typed "{count}".
pub(crate) fn cards_for_sale(c: &rusqlite::Connection) -> rusqlite::Result<i64> {
    Ok(admin::in_vault(c)?.0 + sellers::open_count(c)?)
}

/// A like or a comment changes only the kept copy of the feed (the one signed-out visitors get).
fn stale(app: &App) {
    app.cache.invalidate("feed");
}

/// GET /: the feed.
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
        build(c, &nav, viewer, cursor(&at), &mut Shops::new(c, &nav))
    })
    .await?;
    let end = next.is_empty();
    Ok(html(render(FeedPart { items, next, end })?))
}

/// GET /posts/{id}: one post on its own page (what a shared link opens).
pub async fn one(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>) -> Res {
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
        return Ok(fail(StatusCode::FORBIDDEN, "The vault's team has hidden your page, so you can't post for now."));
    }
    let (id, now) = (m.id, crate::unix_now());
    let (recent, room) = with_db(&app, move |c| Ok((social::posts_since(c, id, now - 3600)?, members::photo_bytes(c)? + dm::media_bytes(c)? + social::photo_bytes(c)?))).await?;
    if recent >= social::POSTS_PER_HOUR {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a lot of posts for one hour. Please take a short break."));
    }
    let (mut text, mut styled, mut files, mut card, mut as_vault) = (String::new(), String::new(), Vec::<Vec<u8>>::new(), messages::CardRef::default(), false);
    // Each new photo's caption, in the order the photos come (a JSON list of words).
    let mut captions: Vec<String> = vec![];
    let bad = |_| (StatusCode::BAD_REQUEST, "That didn't upload. Please try again.".to_string());
    while let Some(f) = mp.next_field().await.map_err(bad)? {
        match f.name().unwrap_or("") {
            "text" => text = f.text().await.map_err(bad)?,
            "rich" => styled = f.text().await.map_err(bad)?,
            "captions" => captions = serde_json::from_str(&f.text().await.map_err(bad)?).unwrap_or_default(),
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
    let (text, styled) = words(&text, &styled);
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
        social::caption_new_photos(c, pid, &[], &captions)?;
        if !styled.is_empty() {
            social::post_rich(c, pid, &styled)?;
        }
        let nav = crate::chrome(c)?;
        tell_post_followers(c, author, id, pid, &text, &nav.brand)?;
        let p = social::post(c, brand_viewer, pid)?;
        Ok(Ok(match p {
            Some(p) => post_view(c, &p, &nav.brand, brand_viewer)?,
            None => None,
        }))
    })
    .await?;
    app.forget();
    match out {
        Ok(Some(v)) => Ok(html(part(vec![Item { post: Some(v), stores: vec![], binder: None }])?)),
        Ok(None) => Ok(fail(StatusCode::INTERNAL_SERVER_ERROR, "That post didn't save. Please try again.")),
        Err(e) => Ok(fail(StatusCode::BAD_REQUEST, e)),
    }
}

/// The words of a post as they are kept: plain, and the same words in styled pieces when the editor sent any
/// (`rich`, runs as JSON; see `rich.rs`). Words sent without styles, or with styles that can't be read, are plain.
fn words(text: &str, styled: &str) -> (String, String) {
    match if styled.trim().is_empty() { None } else { rich::clean(styled, social::TEXT_MAX) } {
        Some(both) => both,
        None => (members::clean_text(text, social::TEXT_MAX), String::new()),
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
    let (mut text, mut styled, mut keep, mut files) = (String::new(), String::new(), Vec::<i64>::new(), Vec::<Vec<u8>>::new());
    // Captions: the photos that stay ({ "id": "words" }) and the new ones, in their order.
    let (mut kept_caps, mut captions): (std::collections::HashMap<String, String>, Vec<String>) = (Default::default(), vec![]);
    let bad = |_| (StatusCode::BAD_REQUEST, "That didn't upload. Please try again.".to_string());
    while let Some(f) = mp.next_field().await.map_err(bad)? {
        match f.name().unwrap_or("") {
            "text" => text = f.text().await.map_err(bad)?,
            "rich" => styled = f.text().await.map_err(bad)?,
            "keep" => keep = f.text().await.map_err(bad)?.split(',').filter_map(|x| x.trim().parse().ok()).collect(),
            "kept_captions" => kept_caps = serde_json::from_str(&f.text().await.map_err(bad)?).unwrap_or_default(),
            "captions" => captions = serde_json::from_str(&f.text().await.map_err(bad)?).unwrap_or_default(),
            "photo" => {
                let b = f.bytes().await.map_err(bad)?;
                if !b.is_empty() && files.len() < social::PHOTOS_MAX {
                    files.push(b.to_vec());
                }
            }
            _ => {}
        }
    }
    let (text, styled) = words(&text, &styled);
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
            let gone = social::post_edit(c, pid, &text, &keep, &photos, now)?;
            for (id, cap) in &kept_caps {
                if let Ok(id) = id.parse::<i64>() {
                    if keep.contains(&id) {
                        social::set_photo_caption(c, pid, id, cap)?;
                    }
                }
            }
            social::caption_new_photos(c, pid, &keep, &captions)?;
            gone
        } else {
            social::post_note(c, pid, &text, now)?;
            vec![]
        };
        if !styled.is_empty() {
            social::post_rich(c, pid, &styled)?;
        }
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
        Some(v) => Ok(html(part(vec![Item { post: Some(v), stores: vec![], binder: None }])?)),
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
        Ok(social::comment_for(c, id, me)?.map(|k| comment_json(c, &k, me, staff)))
    })
    .await?;
    Ok(match out {
        Some(k) => ok(json!({ "ok": true, "comment": k })),
        None => fail(StatusCode::FORBIDDEN, "That comment isn't yours."),
    })
}

#[derive(Deserialize)]
pub struct ReactReq {
    #[serde(default)]
    kind: String,
    /// One photo of the post, when the like is for the photo itself (`post_images.id`); 0 or left out for the post.
    #[serde(default)]
    image: i64,
}

/// Which photo of a post a list or a comment is about: `?image=` (`post_images.id`), or the post itself without it.
#[derive(Deserialize, Default)]
pub struct OnQs {
    #[serde(default)]
    image: i64,
}

/// Where a like or a comment is: the post's own page, opened on the photo when it is on one.
fn link_of(post: i64, image: i64) -> String {
    if image != 0 { format!("/posts/{post}?photo={image}") } else { format!("/posts/{post}") }
}

/// What it is on, in the words of a notice to whoever posted it.
fn thing(p: &social::Post, image: i64) -> &'static str {
    match (p.member_id == 0, image != 0) {
        (true, true) => "the vault's photo",
        (true, false) => "the vault's post",
        (false, true) => "your photo",
        (false, false) => "your post",
    }
}

/// POST /posts/{id}/react {kind, image?}: like, another reaction, or "" to take it back; on the post, or on one of
/// its photos when the photos have their own.
pub async fn react(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<ReactReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if !req.kind.is_empty() && !social::reaction_ok(&req.kind) {
        return Ok(fail(StatusCode::BAD_REQUEST, "That reaction isn't one of ours."));
    }
    let (me, name, now, kind, image) = (m.id, m.name.clone(), crate::unix_now(), req.kind.clone(), req.image);
    let out = with_db(&app, move |c| {
        let n = if image != 0 { social::photo_react(c, id, image, me, &kind, now)? } else { social::react(c, id, me, &kind, now)? };
        let Some(n) = n else { return Ok(None) };
        // Whoever wrote it hears (the vault's own posts: its admins): one line per post or photo, brought up to date
        // ("Ana and 2 others reacted to your post") instead of one per reaction.
        if !kind.is_empty() {
            if let Some(p) = social::post(c, 0, id)? {
                let whose = thing(&p, image);
                let words = if n > 1 { format!("{name} and {} other{} reacted to {whose}.", n - 1, if n == 2 { "" } else { "s" }) } else { format!("{name} reacted {} to {whose}.", social::emoji(&kind)) };
                let group = if image != 0 { format!("preact:{image}") } else { format!("react:{id}") };
                for who in post_owners(c, &p)?.into_iter().filter(|w| *w != me) {
                    sellers::notify_from(c, who, me, "social", &group, &words, &link_of(id, image))?;
                }
            }
        }
        let tops = if image != 0 { social::photo_tops(c, image)? } else { social::top_reactions(c, id)? };
        let tops: Vec<&'static str> = tops.iter().map(|k| social::emoji(k)).collect();
        Ok(Some((n, tops)))
    })
    .await?;
    stale(&app);
    Ok(match out {
        Some((n, tops)) => ok(json!({ "ok": true, "n": n, "mine": req.kind, "emoji": social::emoji(&req.kind), "tops": tops })),
        None => fail(StatusCode::NOT_FOUND, "That post isn't here any more."),
    })
}

fn comment_json(c: &rusqlite::Connection, k: &social::Comment, viewer: i64, staff: bool) -> Value {
    let tops: Vec<&'static str> = if k.like_n > 0 { social::comment_tops(c, k.id).unwrap_or_default().iter().map(|x| social::emoji(x)).collect() } else { vec![] };
    json!({
        "edited": k.edited_at > 0, "id": k.id, "parent": k.parent_id, "member": k.member_id, "name": k.name, "avatar": k.avatar,
        "initial": members::initial(&k.name), "role": k.role, "text": k.text, "at": k.created_at, "when": members::ago(k.created_at),
        "own": viewer == k.member_id, "can_remove": viewer == k.member_id || staff,
        "likes": k.like_n, "mine": k.mine, "emoji": social::emoji(&k.mine), "tops": tops, "image": k.image, "thumb": k.thumb,
    })
}

/// GET /posts/{id}/comments, or `?image=` for the comments on one of its photos.
pub async fn comments(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Query(q): Query<OnQs>) -> Res {
    let viewer = current(&app, &h).await?.map(|m| m.id).unwrap_or(0);
    let staff = admin::power(&app, &h) != admin::Power::Nothing;
    let image = q.image;
    let list = with_db(&app, move |c| Ok(social::comments_on(c, id, image, viewer, 200)?.iter().map(|k| comment_json(c, k, viewer, staff)).collect::<Vec<_>>())).await?;
    Ok(ok(json!({ "comments": list })))
}

#[derive(Deserialize)]
pub struct CaptionReq {
    #[serde(default)]
    caption: String,
}

/// POST /posts/{id}/photos/{image}/caption {caption}: the post's writer adds, changes or (with "") takes off one
/// photo's caption, from the photo window.
pub async fn caption(State(app): State<App>, h: HeaderMap, Path((id, image)): Path<(i64, i64)>, Json(req): Json<CaptionReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let (me, full) = (m.id, m.role == "admin");
    let out = with_db(&app, move |c| {
        let Some(p) = social::post(c, me, id)?.filter(|p| p.written() && may_edit(p, me, full)) else { return Ok(None) };
        if !social::set_photo_caption(c, p.id, image, &req.caption)? {
            return Ok(None);
        }
        let caps = social::photo_captions(c, p.id)?;
        let at = social::photos_ids(c, p.id)?.iter().position(|x| x.0 == image).unwrap_or(0);
        Ok(caps.get(at).cloned())
    })
    .await?;
    stale(&app);
    Ok(match out {
        Some(caption) => ok(json!({ "ok": true, "caption": caption })),
        None => fail(StatusCode::FORBIDDEN, "That photo isn't yours to caption."),
    })
}

/// How many posts a person's page shows at a time.
const PAGE_POSTS: i64 = 8;

/// One author's posts (0: the vault's) for their own page: the next few, and where the ones after them are (a full
/// address for the page's "more" loader, or "" at the end).
pub(crate) fn page_items(c: &rusqlite::Connection, viewer: i64, author: i64, before: Option<(i64, i64)>) -> rusqlite::Result<(Vec<Item>, String)> {
    let brand = blocks::setting(c, "brand")?.filter(|b| !b.trim().is_empty()).unwrap_or_else(|| "Big Boys Vault".into());
    let posts = social::posts_by(c, viewer, author, before, PAGE_POSTS + 1)?;
    let more = posts.len() as i64 > PAGE_POSTS;
    let (mut items, mut last) = (vec![], None);
    for p in posts.iter().take(PAGE_POSTS as usize) {
        last = Some((p.created_at, p.id));
        if let Some(v) = post_view(c, p, &brand, viewer)? {
            items.push(Item { post: Some(v), stores: vec![], binder: None });
        }
    }
    let next = match (more, last) {
        (true, Some((t, id))) => format!("/posts/by/{author}?c={t}:{id}"),
        _ => String::new(),
    };
    Ok((items, next))
}

/// The posts of a page, drawn.
pub(crate) fn page_html(items: Vec<Item>, next: String) -> Result<String, (StatusCode, String)> {
    render(FeedPart { items, next, end: false })
}

/// May everyone see this author's posts on their page? The vault's always; a trainer's while their page shows.
fn page_open(c: &rusqlite::Connection, author: i64) -> rusqlite::Result<bool> {
    Ok(author == 0 || members::get(c, author)?.is_some_and(|m| m.profile_ok && !m.blocked))
}

#[derive(Deserialize, Default)]
pub struct ByQs {
    #[serde(default)]
    c: String,
}

/// GET /posts/by/{author}?c=time:id: the next posts of a person's page (0: the vault's), as HTML.
pub async fn by(State(app): State<App>, h: HeaderMap, Path(author): Path<i64>, Query(q): Query<ByQs>) -> Res {
    let me = current(&app, &h).await?;
    let viewer = me.as_ref().map(|m| m.id).unwrap_or(0);
    let staff = admin::can_moderate(&app, &h);
    let before = q.c.split_once(':').and_then(|(t, i)| Some((t.parse().ok()?, i.parse().ok()?)));
    let (items, next) = with_db(&app, move |c| {
        if !(viewer == author || staff || page_open(c, author)?) {
            return Ok((vec![], String::new()));
        }
        page_items(c, viewer, author, before)
    })
    .await?;
    if items.is_empty() && before.is_none() {
        return Ok(html("<p class=\"box empty page-empty\">No Posts Yet.</p>".to_string()));
    }
    Ok(html(page_html(items, next)?))
}

/// GET /posts/{id}/photos/{image}: one photo of a written post as a small post of its own (who posted it, which photo
/// it is, its own likes and comments), for the photo window. Not found when the photo's likes and comments are its
/// post's (a post's only photo, and the photos of automatic posts).
pub async fn photo(State(app): State<App>, h: HeaderMap, Path((id, image)): Path<(i64, i64)>) -> Res {
    let viewer = current(&app, &h).await?.map(|m| m.id).unwrap_or(0);
    let view = with_db(&app, move |c| {
        let Some(p) = social::post(c, viewer, id)?.filter(|p| !p.hidden) else { return Ok(None) };
        let Some(own) = social::photo_own(c, id, image, viewer)? else { return Ok(None) };
        let nav = crate::chrome(c)?;
        photo_view(c, &p, &own, &nav.brand)
    })
    .await?;
    match view {
        Some(v) => Ok(html(part(vec![Item { post: Some(v), stores: vec![], binder: None }])?)),
        None => Ok(fail(StatusCode::NOT_FOUND, "That photo isn't here any more.")),
    }
}

/// One photo with its own likes and comments, drawn with the post's own layout (so every button works as on a post).
fn photo_view(c: &rusqlite::Connection, p: &social::Post, own: &social::PhotoOwn, brand: &str) -> rusqlite::Result<Option<PostView>> {
    let Some(author) = who(c, p.member_id, brand, false)? else { return Ok(None) };
    Ok(Some(PostView {
        id: p.id,
        kind: "photo".into(),
        who: author,
        headline: String::new(),
        when: members::ago(p.created_at),
        text: String::new(),
        rich: String::new(),
        tiles: vec![],
        more: 0,
        link: String::new(),
        link_text: String::new(),
        photos: vec![],
        caps: vec![],
        member: p.member_id,
        edited: false,
        attach: None,
        people: vec![],
        like_n: own.like_n,
        comment_n: own.comment_n,
        mine: own.mine.clone(),
        tops: social::photo_tops(c, own.id)?.iter().map(|k| social::emoji(k)).collect(),
        pinned: false,
        own: false,
        small: false,
        photo_threads: false,
        image: own.id,
        image_at: format!("Photo {} Of {}", own.at, own.of),
    }))
}

/// The start of what someone said, for a line in the bell: their words, or "a photo".
/// GET /posts/{id}/reactions: who reacted to a post (or, with `?image=`, to one of its photos), newest first (the
/// list behind its likes).
pub async fn reactions(State(app): State<App>, Path(id): Path<i64>, Query(q): Query<OnQs>) -> Res {
    let image = q.image;
    let list = with_db(&app, move |c| {
        if social::post(c, 0, id)?.filter(|p| !p.hidden).is_none() {
            return Ok(None);
        }
        if image != 0 {
            return Ok(if social::photo_has_its_own(c, id, image)? { Some(social::photo_reactions_of(c, image, 300)?) } else { None });
        }
        Ok(Some(social::reactions_of(c, id, 300)?))
    })
    .await?;
    let Some(list) = list else { return Ok(fail(StatusCode::NOT_FOUND, "That post isn't here any more.")) };
    let people: Vec<serde_json::Value> = list
        .into_iter()
        .map(|(mid, name, avatar, kind)| json!({ "id": mid, "initial": members::initial(&name), "name": name, "avatar": avatar, "emoji": social::emoji(&kind), "url": format!("/trainers/{mid}") }))
        .collect();
    Ok(ok(json!({ "people": people })))
}

#[derive(Deserialize, Default)]
pub struct ReportReq {
    #[serde(default)]
    reason: String,
}

/// POST /posts/{id}/report, /comments/{id}/report: once per person per post or comment; the vault's admins and
/// moderators hear (one line per thing reported, kept up to date).
async fn report(app: App, h: HeaderMap, kind: &'static str, id: i64, reason: String) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    let me = m.id;
    if over_limit(&app, format!("report|{me}"), 20) {
        return Ok(fail(StatusCode::TOO_MANY_REQUESTS, "That's a lot of reports for now. The team will look at them."));
    }
    let (name, now, reason) = (m.name.clone(), crate::unix_now(), members::clean_line(&reason, 200));
    let out = with_db(&app, move |c| {
        let (post, words) = match kind {
            "post" => match social::post(c, 0, id)? {
                Some(p) => (p.id, p.text.clone()),
                None => return Ok(None),
            },
            _ => match social::comment(c, id)? {
                Some(k) => (k.post_id, k.text.clone()),
                None => return Ok(None),
            },
        };
        if social::report(c, kind, id, me, &reason, now)? {
            let what = if kind == "post" { "a post" } else { "a comment" };
            let why = if reason.is_empty() { String::new() } else { format!(" ({reason})") };
            for s in sellers::staff_ids(c)?.into_iter().filter(|s| *s != me) {
                sellers::notify_from(c, s, me, "staff", &format!("report:{kind}:{id}"), &format!("{name} reported {what}{why}: {}", said(&words)), &format!("/posts/{post}"))?;
            }
        }
        Ok(Some(()))
    })
    .await?;
    Ok(match out {
        Some(()) => ok(json!({ "ok": true })),
        None => fail(StatusCode::NOT_FOUND, "That isn't here any more."),
    })
}

pub async fn report_post(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<ReportReq>) -> Res {
    report(app, h, "post", id, req.reason).await
}

pub async fn report_comment(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<ReportReq>) -> Res {
    report(app, h, "comment", id, req.reason).await
}

fn said(text: &str) -> String {
    if text.is_empty() {
        return "a photo".into();
    }
    let short: String = if text.chars().count() > 80 { format!("{}…", text.chars().take(79).collect::<String>()) } else { text.to_string() };
    format!("“{short}”")
}

/// POST /posts/{id}/comments (a form: text, parent, and maybe one photo, uploaded or taken with the card camera): a
/// comment or a reply, in words, a photo or both. Whoever wrote the post hears (the vault's own posts: its admins), and
/// whoever wrote the comment answered.
pub async fn comment(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, mut mp: Multipart) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    // `image`: the photo of the post this comment is on, when the post's photos have their own comments.
    let (mut text, mut parent, mut file, mut on) = (String::new(), None::<i64>, None::<Vec<u8>>, 0i64);
    let bad = |_| (StatusCode::BAD_REQUEST, "That didn't upload. Please try again.".to_string());
    while let Some(f) = mp.next_field().await.map_err(bad)? {
        match f.name().unwrap_or("") {
            "text" => text = f.text().await.map_err(bad)?,
            "parent" => parent = f.text().await.map_err(bad)?.trim().parse().ok(),
            "image" => on = f.text().await.map_err(bad)?.trim().parse().unwrap_or(0),
            "photo" => {
                let b = f.bytes().await.map_err(bad)?;
                if !b.is_empty() {
                    file = Some(b.to_vec());
                }
            }
            _ => {}
        }
    }
    let text = members::clean_text(&text, social::COMMENT_MAX);
    if text.is_empty() && file.is_none() {
        return Ok(fail(StatusCode::BAD_REQUEST, "Write something or add a photo first."));
    }
    // Photos, as on posts, unless the vault's team hid the trainer's page; words are for everyone signed in.
    if file.is_some() && !m.profile_ok {
        return Ok(fail(StatusCode::FORBIDDEN, "The vault's team has hidden your page, so you can't add photos for now. Words still work."));
    }
    let (me, name, now) = (m.id, m.name.clone(), crate::unix_now());
    let staff = admin::power(&app, &h) != admin::Power::Nothing;
    if with_db(&app, move |c| social::comments_since(c, me, now - 3600)).await? >= social::COMMENTS_PER_HOUR {
        return Ok(fail(StatusCode::BAD_REQUEST, "That's a lot of comments for one hour. Please take a short break."));
    }
    let mut photo: Option<(String, String, i64)> = None;
    if let Some(bytes) = file {
        let room = with_db(&app, |c| Ok(members::photo_bytes(c)? + dm::media_bytes(c)? + social::photo_bytes(c)?)).await?;
        if room >= photo_cap_mb() * 1024 * 1024 {
            return Ok(fail(StatusCode::INSUFFICIENT_STORAGE, "The vault's photo space is full right now. Words still work."));
        }
        let dir = PathBuf::from(&*app.uploads);
        let stem = format!("k{me}-{}{}", members::temp_password(), members::temp_password());
        match gated(move || store_photo(bytes, dir, stem)).await? {
            Ok((l, s, _w, _h, size)) => photo = Some((l, s, size)),
            Err(e) if e == TOO_BIG => return Ok(fail(StatusCode::BAD_REQUEST, "That photo is too big. Please use a smaller one.")),
            Err(_) => return Ok(fail(StatusCode::BAD_REQUEST, "That photo couldn't be read. Try a JPEG or PNG.")),
        }
    }
    let kept = photo.clone();
    let out = with_db(&app, move |c| {
        let pic = photo.as_ref().map(|(l, s, b)| (l.as_str(), s.as_str(), *b));
        let Some(cid) = social::comment_add_on(c, id, on, me, parent, &text, pic, now)? else { return Ok(Err("That post isn't here any more.")) };
        let what = said(&text);
        let link = link_of(id, on);
        let mut told = vec![me];
        if let Some(p) = social::post(c, 0, id)? {
            let whose = thing(&p, on);
            for who in post_owners(c, &p)? {
                if !told.contains(&who) {
                    sellers::notify_from(c, who, me, "social", "", &format!("{name} commented on {whose}: {what}"), &link)?;
                    told.push(who);
                }
            }
        }
        if let Some(k) = parent.and_then(|pid| social::comment(c, pid).ok().flatten()) {
            if !told.contains(&k.member_id) {
                sellers::notify_from(c, k.member_id, me, "social", "", &format!("{name} replied to your comment: {what}"), &link)?;
            }
        }
        let n: i64 = if on != 0 { c.query_row("SELECT comment_n FROM post_images WHERE id=?1", [on], |r| r.get(0))? } else { c.query_row("SELECT comment_n FROM posts WHERE id=?1", [id], |r| r.get(0))? };
        let k = social::comment_for(c, cid, me)?;
        Ok(Ok((k.map(|k| comment_json(c, &k, me, staff)), n)))
    })
    .await;
    let out = match out {
        Ok(o) => o,
        Err(e) => {
            // Nothing saved: the photo that went up first goes too.
            if let Some((l, s, _)) = &kept {
                admin::remove_upload(&app.uploads, l);
                admin::remove_upload(&app.uploads, s);
            }
            return Err(e);
        }
    };
    stale(&app);
    Ok(match out {
        Ok((Some(k), n)) => ok(json!({ "ok": true, "comment": k, "n": n })),
        Ok((None, _)) => fail(StatusCode::INTERNAL_SERVER_ERROR, "That comment didn't save. Please try again."),
        Err(e) => {
            // The post went away while the photo went up: the photo goes too.
            if let Some((l, s, _)) = kept {
                admin::remove_upload(&app.uploads, &l);
                admin::remove_upload(&app.uploads, &s);
            }
            fail(StatusCode::BAD_REQUEST, e)
        }
    })
}

/// POST /comments/{id}/react {kind}: like a comment, another reaction, or "" to take it back. Its writer hears, one line
/// per comment brought up to date.
pub async fn comment_react(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>, Json(req): Json<ReactReq>) -> Res {
    let m = match acting(&app, &h).await? {
        Ok(m) => m,
        Err(r) => return Ok(r),
    };
    if !req.kind.is_empty() && !social::reaction_ok(&req.kind) {
        return Ok(fail(StatusCode::BAD_REQUEST, "That reaction isn't one of ours."));
    }
    let (me, name, now, kind) = (m.id, m.name.clone(), crate::unix_now(), req.kind.clone());
    let out = with_db(&app, move |c| {
        let Some(n) = social::comment_react(c, id, me, &kind, now)? else { return Ok(None) };
        if !kind.is_empty() {
            if let Some(k) = social::comment(c, id)?.filter(|k| k.member_id != me) {
                let words = if n > 1 { format!("{name} and {} other{} reacted to your comment: {}", n - 1, if n == 2 { "" } else { "s" }, said(&k.text)) } else { format!("{name} reacted {} to your comment: {}", social::emoji(&kind), said(&k.text)) };
                sellers::notify_from(c, k.member_id, me, "social", &format!("creact:{id}"), &words, &link_of(k.post_id, k.image_id))?;
            }
        }
        let tops: Vec<&'static str> = social::comment_tops(c, id)?.iter().map(|x| social::emoji(x)).collect();
        Ok(Some((n, tops)))
    })
    .await?;
    stale(&app);
    Ok(match out {
        Some((n, tops)) => ok(json!({ "ok": true, "likes": n, "mine": req.kind, "emoji": social::emoji(&req.kind), "tops": tops })),
        None => fail(StatusCode::NOT_FOUND, "That comment isn't here any more."),
    })
}

/// POST /comments/{id}/delete: your own comment, or any comment for staff; the replies under it go with it.
/// Nobody is left wondering where it went (the owner, 19 Nov 2026): when you delete your own, whoever wrote the post
/// hears (the vault's own posts: its admins); when the vault's team takes one down, its writer hears.
pub async fn comment_remove(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>) -> Res {
    if !same_site(&h) {
        return Ok(fail(StatusCode::FORBIDDEN, "That request didn't come from this site."));
    }
    let (me, name) = current(&app, &h).await?.map(|m| (m.id, m.name)).unwrap_or((0, String::new()));
    let staff = admin::power(&app, &h) != admin::Power::Nothing;
    let out = with_db(&app, move |c| take_comment(c, id, me, &name, staff)).await?;
    stale(&app);
    Ok(match out {
        Some(Ok((n, files))) => {
            for f in files {
                admin::remove_upload(&app.uploads, &f);
            }
            ok(json!({ "ok": true, "n": n }))
        }
        Some(Err(())) => fail(StatusCode::FORBIDDEN, "That comment isn't yours."),
        None => ok(json!({ "ok": true, "n": 0 })),
    })
}

/// A comment taken away by `me` (0: the owner signed in with the admin password alone), with the replies under it,
/// and whoever should hear told. None: it was gone already. Err: it isn't theirs to delete. Else how many comments
/// the post has now, and the photo files to remove from disk.
fn take_comment(c: &rusqlite::Connection, id: i64, me: i64, name: &str, staff: bool) -> rusqlite::Result<Option<Result<(i64, Vec<String>), ()>>> {
    let Some(k) = social::comment(c, id)? else { return Ok(None) };
    let own = me != 0 && me == k.member_id;
    if !(staff || own) {
        return Ok(Some(Err(())));
    }
    let on = social::post(c, 0, k.post_id)?;
    let (post, files) = social::comment_remove(c, id)?.unwrap_or((k.post_id, vec![]));
    let (what, link) = (said(&k.text), link_of(post, k.image_id));
    if own {
        if let Some(p) = &on {
            let whose = thing(p, k.image_id);
            for who in post_owners(c, p)?.into_iter().filter(|w| *w != me) {
                sellers::notify_from(c, who, me, "social", "", &format!("{name} deleted their comment on {whose}: {what}"), &link)?;
            }
        }
    } else {
        sellers::notify_from(c, k.member_id, 0, "announcement", "", &format!("The vault's team took down your comment: {what}"), &link)?;
    }
    // How many comments are left where it was: on its photo, or on the post.
    let left = if k.image_id != 0 { c.query_row("SELECT comment_n FROM post_images WHERE id=?1", [k.image_id], |r| r.get(0)) } else { c.query_row("SELECT comment_n FROM posts WHERE id=?1", [post], |r| r.get(0)) };
    let n: i64 = rusqlite::OptionalExtension::optional(left)?.unwrap_or(0);
    Ok(Some(Ok((n, files))))
}

/// POST /posts/{id}/delete: the author deletes their own post; staff take any post out of the feed.
/// (An automatic post is hidden, not deleted: the site would only write it again.)
pub async fn remove(State(app): State<App>, h: HeaderMap, Path(id): Path<i64>) -> Res {
    if !same_site(&h) {
        return Ok(fail(StatusCode::FORBIDDEN, "That request didn't come from this site."));
    }
    let me = current(&app, &h).await?.map(|m| m.id).unwrap_or(0);
    let staff = admin::power(&app, &h) != admin::Power::Nothing;
    let out = with_db(&app, move |c| take_post(c, id, me, staff)).await?;
    let Some(files) = out else { return Ok(fail(StatusCode::FORBIDDEN, "That post isn't yours.")) };
    for f in files {
        admin::remove_upload(&app.uploads, &f);
    }
    app.forget();
    Ok(ok(json!({ "ok": true })))
}

/// A post taken out of the feed by `me` (0: the owner signed in with the admin password alone). None: it isn't theirs
/// to take. Else the photo files to remove from disk. A trainer's own words taken down by the vault's team tell their
/// writer, as a comment does (nobody is left wondering where it went); deleting your own tells nobody.
fn take_post(c: &rusqlite::Connection, id: i64, me: i64, staff: bool) -> rusqlite::Result<Option<Vec<String>>> {
    let Some(p) = social::post(c, 0, id)? else { return Ok(Some(vec![])) };
    let written = p.kind == "post" || p.kind == "share";
    if written && (staff || (me != 0 && me == p.member_id)) {
        // (Staff includes the full admins, who own what the vault itself posted.)
        let files = social::post_remove(c, id)?;
        if p.member_id != 0 && p.member_id != me {
            sellers::notify_from(c, p.member_id, 0, "announcement", "", &format!("The vault's team took down your post: {}", said(&p.text)), "/")?;
        }
        return Ok(Some(files));
    }
    if staff {
        // 2: hidden by a person, so the automatic refresh never brings it back.
        c.execute("UPDATE posts SET hidden=2 WHERE id=?1", [id])?;
        return Ok(Some(vec![]));
    }
    Ok(None)
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
    use marcuru_rs::members::Added;

    #[test]
    fn a_page_knows_where_it_starts() {
        assert!(matches!(cursor(""), Cursor::Top));
        assert!(matches!(cursor("p:1794400000:12"), Cursor::After(1_794_400_000, 12)));
        assert!(matches!(cursor("v:7"), Cursor::Binders(7)));
        assert!(matches!(cursor("p:x:y"), Cursor::Top));
        assert!(matches!(cursor("v:99999999999"), Cursor::Binders(100_000)), "a silly page number is capped");
    }

    fn db() -> rusqlite::Connection {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        db::register_set_code(&c).unwrap();
        c.execute_batch(db::SCHEMA).unwrap();
        c.execute_batch(blocks::SCHEMA).unwrap();
        c.execute_batch(members::SCHEMA).unwrap();
        members::migrate(&c).unwrap();
        db::migrate(&c).unwrap();
        social::migrate(&c).unwrap();
        marcuru_rs::history::migrate(&c).unwrap();
        c
    }

    /// The vault: Pikachu Set (two cards for sale, one sold), Graded Cards (one for sale), Trainers (sold out). Its newest
    /// card went up on 1 Jan 2026.
    fn vault(c: &rusqlite::Connection) {
        for (i, (h, t)) in [("pikachu-set", "Pikachu Set"), ("graded-cards", "Graded Cards"), ("trainers", "Trainers")].iter().enumerate() {
            c.execute("INSERT INTO collections(handle,title,position) VALUES(?1,?2,?3)", rusqlite::params![h, t, i as i64]).unwrap();
        }
        for (id, title, price, sale, binder) in [(1, "Pikachu V", 500.0, true, "pikachu-set"), (2, "Pikachu ex", 900.0, true, "pikachu-set"), (3, "Raichu", 50.0, false, "pikachu-set"), (4, "Iono", 300.0, false, "trainers"), (5, "Charizard PSA 10", 9000.0, true, "graded-cards")] {
            c.execute(
                "INSERT INTO products(id,handle,title,price,image,available,created_at) VALUES(?1,?2,?3,?4,?5,?6,'2026-01-01T00:00:00')",
                rusqlite::params![id, format!("c{id}"), title, price, format!("/uploads/c0-{id}_l.jpg"), sale],
            )
            .unwrap();
            c.execute("INSERT INTO collection_products(collection,product_id) VALUES(?1,?2)", rusqlite::params![binder, id]).unwrap();
        }
    }

    /// An open store with binders of cards: (binder, [(card, status)]). Its cards were last changed at `when`.
    fn store(c: &rusqlite::Connection, name: &str, binders: &[(&str, &str, &[(&str, &str)])], when: i64) -> (i64, Vec<i64>) {
        let s = members::create(c, name, "h").unwrap().unwrap();
        members::set_seller(c, s, true).unwrap();
        members::set_profile_ok(c, s, true).unwrap();
        let mut ids = vec![];
        for (title, visibility, cards) in binders {
            let b = members::create_member_binder(c, s, title).unwrap();
            c.execute("UPDATE member_binders SET visibility=?2 WHERE id=?1", rusqlite::params![b.id, visibility]).unwrap();
            for (card, status) in cards.iter() {
                let Added::Yes(id) = sellers::add_listing(c, s, &format!("tcgdex:{card}"), card, "1", "Set", &format!("/p/{card}.webp"), "").unwrap() else { panic!() };
                members::move_card_to_binder(c, s, id, Some(b.id)).unwrap();
                c.execute("UPDATE member_cards SET status=?2 WHERE id=?1", rusqlite::params![id, status]).unwrap();
                ids.push(id);
            }
        }
        c.execute("UPDATE member_cards SET updated_at=?2 WHERE member_id=?1", rusqlite::params![s, when]).unwrap();
        (s, ids)
    }

    /// Ash put up cards after the vault's newest; Brock before it. Ash has two public binders with cards for sale, a
    /// private one and one whose only card is reserved; Brock has one.
    fn town(c: &rusqlite::Connection) -> (i64, Vec<i64>, i64) {
        vault(c);
        let (ash, ids) = store(
            c,
            "Ash",
            &[("Fire", "Public", &[("charmander", "Active"), ("vulpix", "Active")]), ("Water", "Public", &[("squirtle", "Active")]), ("Secret", "Private", &[("mew", "Active")]), ("Held", "Public", &[("onix", "Reserved")])],
            2_000_000_000,
        );
        let (brock, _) = store(c, "Brock", &[("Rock", "Public", &[("geodude", "Active")])], 1_000);
        (ash, ids, brock)
    }

    fn feed_part(items: Vec<Item>, next: String) -> String {
        let end = next.is_empty();
        render(FeedPart { items, next, end }).unwrap()
    }

    #[test]
    fn followers_hear_when_someone_they_follow_posts() {
        let c = db();
        let (ash, _, brock) = town(&c);
        sellers::toggle_follow(&c, brock, ash).unwrap();
        let p1 = social::post_add(&c, ash, "post", "Look at this pull", &[], &json!({}), 10).unwrap();
        tell_post_followers(&c, ash, ash, p1, "Look at this pull", "Big Boys Vault").unwrap();
        let p2 = social::post_add(&c, ash, "post", "", &[], &json!({}), 11).unwrap();
        tell_post_followers(&c, ash, ash, p2, "", "Big Boys Vault").unwrap();
        let lines: Vec<(String, String)> = c.prepare("SELECT message, link FROM notifications WHERE member_id=?1 AND kind='following'").unwrap().query_map([brock], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect();
        assert_eq!(lines, vec![("Ash posted: a photo".to_string(), format!("/posts/{p2}"))], "one line per author, brought up to date");
        // Followers of the vault's store hear the vault's posts; the admin who wrote it doesn't.
        sellers::toggle_follow(&c, brock, 0).unwrap();
        let v = social::post_add(&c, 0, "post", "New box!", &[], &json!({}), 12).unwrap();
        tell_post_followers(&c, 0, brock, v, "New box!", "Big Boys Vault").unwrap();
        tell_post_followers(&c, 0, ash, v, "New box!", "Big Boys Vault").unwrap();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM notifications WHERE member_id=?1 AND message LIKE 'Big Boys Vault posted%'", [brock], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    /// A comment that goes doesn't go silently: deleting your own tells whoever wrote the post; the vault's team
    /// taking one down tells its writer. Nobody else can delete it.
    #[test]
    fn a_deleted_comment_tells_whoever_should_know() {
        let c = db();
        let (ash, _, brock) = town(&c);
        let misty = members::create(&c, "Misty", "h").unwrap().unwrap();
        let p = social::post_add(&c, ash, "post", "Look at this pull", &[], &json!({}), 10).unwrap();
        let said = |c: &rusqlite::Connection, who: i64| -> Vec<(String, String, i64)> {
            c.prepare("SELECT kind, message, actor_id FROM notifications WHERE member_id=?1 ORDER BY id").unwrap().query_map([who], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect()
        };
        // Brock deletes his own comment (and the reply under it): Ash, whose post it is, hears.
        let k = social::comment_add(&c, p, brock, None, "Trade you for it?", None, 11).unwrap().unwrap();
        social::comment_add(&c, p, misty, Some(k), "Me first", None, 12).unwrap().unwrap();
        assert_eq!(take_comment(&c, k, misty, "Misty", false).unwrap(), Some(Err(())), "not hers to delete");
        assert_eq!(take_comment(&c, k, brock, "Brock", false).unwrap(), Some(Ok((0, vec![]))));
        assert_eq!(said(&c, ash), vec![("social".to_string(), "Brock deleted their comment on your post: “Trade you for it?”".to_string(), brock)]);
        assert!(said(&c, brock).is_empty(), "he knows: he did it");
        // Ash deleting his own comment on his own post tells nobody.
        let own = social::comment_add(&c, p, ash, None, "Thanks all", None, 13).unwrap().unwrap();
        take_comment(&c, own, ash, "Ash", false).unwrap();
        assert_eq!(said(&c, ash).len(), 1);
        // The vault's team takes one down: its writer hears, in the vault's name.
        let rude = social::comment_add(&c, p, misty, None, "", None, 14).unwrap().unwrap();
        assert_eq!(take_comment(&c, rude, 0, "", true).unwrap(), Some(Ok((0, vec![]))));
        assert_eq!(said(&c, misty), vec![("announcement".to_string(), "The vault's team took down your comment: a photo".to_string(), 0)]);
        assert_eq!(take_comment(&c, rude, 0, "", true).unwrap(), None, "gone already");
        // A post goes the same way: its own writer deleting it tells nobody; the vault's team taking it down tells them.
        assert_eq!(take_post(&c, p, misty, false).unwrap(), None, "not hers to take");
        let mine = social::post_add(&c, brock, "post", "Selling my Onix", &[], &json!({}), 20).unwrap();
        assert_eq!(take_post(&c, mine, brock, false).unwrap(), Some(vec![]));
        assert!(said(&c, brock).is_empty());
        assert_eq!(take_post(&c, p, 0, true).unwrap(), Some(vec![]));
        assert_eq!(said(&c, ash).last().unwrap(), &("announcement".to_string(), "The vault's team took down your post: “Look at this pull”".to_string(), 0));
    }

    /// The owner, 19 Nov 2026: "give the option to bold, change font size in 3 levels of any text within a post". A
    /// post's words keep their styles through saving, are drawn with a few safe tags and nothing else, and are plain
    /// again once they are edited to plain words.
    #[test]
    fn a_posts_words_keep_their_bold_and_sizes() {
        let c = db();
        let (ash, _, _) = town(&c);
        let (text, styled) = words("ignored when styles come", r#"[{"t":"BIG pull\n","b":true,"s":"l"},{"t":"  <b>not a tag</b> & small print ","s":"s"}]"#);
        assert_eq!(text, "BIG pull\n<b>not a tag</b> & small print");
        let p = social::post_add(&c, ash, "post", &text, &[], &json!({}), 10).unwrap();
        social::post_rich(&c, p, &styled).unwrap();
        let draw = |c: &rusqlite::Connection, id: i64| {
            let post = social::post(c, 0, id).unwrap().unwrap();
            let v = post_view(c, &post, "Big Boys Vault", 0).unwrap().unwrap();
            part(vec![Item { post: Some(v), stores: vec![], binder: None }]).unwrap()
        };
        let html = draw(&c, p);
        assert!(html.contains("<p class=\"post-text\"><span class=\"rt-l\"><b>BIG pull\n</b></span><span class=\"rt-s\">&lt;b&gt;not a tag&lt;/b&gt; &amp; small print</span></p>"), "{html}");
        // Plain words (no styles sent, or "styles" that aren't) are kept as they always were.
        assert_eq!(words("  Just   words \n\n\n more ", ""), ("Just words\n\nmore".to_string(), String::new()));
        assert_eq!(words("Fallback words", "<b>html is not styles</b>"), ("Fallback words".to_string(), String::new()));
        // Edited to plain words: the old styles go with the old words, and styles about other words are never drawn.
        social::post_edit(&c, p, "New plain words", &[], &[], 20).unwrap();
        let html = draw(&c, p);
        assert!(html.contains("<p class=\"post-text\">New plain words</p>") && !html.contains("rt-l"), "{html}");
        social::post_rich(&c, p, &styled).unwrap();
        assert!(draw(&c, p).contains("<p class=\"post-text\">New plain words</p>"));
    }

    /// The owner, 19 Nov 2026: "we should be able to have unique comment,react, etc. per photo, not just show the entire
    /// post, this doesn't apply to card listings etc. just organic posts."
    #[test]
    fn each_photo_of_a_written_post_has_its_own_likes_and_comments() {
        let c = db();
        let (ash, _, brock) = town(&c);
        let pic = |n: i64| (format!("/uploads/p{n}_l.jpg"), format!("/uploads/p{n}_s.jpg"), 800, 600, 1000);
        let p = social::post_add(&c, ash, "post", "Three pulls", &[pic(1), pic(2), pic(3)], &json!({}), 10).unwrap();
        let ids: Vec<i64> = social::photos_ids(&c, p).unwrap().into_iter().map(|x| x.0).collect();
        let (a, b) = (ids[0], ids[1]);
        // The post says its photos have their own. A post with one photo doesn't: its photo is the post, as on Facebook.
        assert!(post_view(&c, &social::post(&c, 0, p).unwrap().unwrap(), "Big Boys Vault", 0).unwrap().unwrap().photo_threads);
        let single = social::post_add(&c, ash, "post", "One pull", &[pic(9)], &json!({}), 11).unwrap();
        let one = social::photos_ids(&c, single).unwrap()[0].0;
        assert!(!post_view(&c, &social::post(&c, 0, single).unwrap().unwrap(), "Big Boys Vault", 0).unwrap().unwrap().photo_threads);
        assert!(social::photo_react(&c, single, one, brock, "like", 12).unwrap().is_none() && social::photo_own(&c, single, one, 0).unwrap().is_none());
        assert!(social::photo_react(&c, single, a, brock, "like", 12).unwrap().is_none(), "a photo of another post");
        // Brock loves the second photo and comments on it. The post itself and the first photo are untouched.
        assert_eq!(social::photo_react(&c, p, b, brock, "love", 13).unwrap(), Some(1));
        let k = social::comment_add_on(&c, p, b, brock, None, "This one!", None, 14).unwrap().unwrap();
        let r = social::comment_add_on(&c, p, b, ash, Some(k), "Thanks", None, 15).unwrap().unwrap();
        let own = social::photo_own(&c, p, b, brock).unwrap().unwrap();
        assert_eq!((own.like_n, own.comment_n, own.mine.as_str(), own.at, own.of), (1, 2, "love", 2, 3));
        let first = social::photo_own(&c, p, a, brock).unwrap().unwrap();
        assert_eq!((first.like_n, first.comment_n, first.mine.as_str(), first.at), (0, 0, "", 1));
        let post = social::post(&c, brock, p).unwrap().unwrap();
        assert_eq!((post.like_n, post.comment_n, post.mine.as_str()), (0, 0, ""));
        assert!(social::comments(&c, p, 0, 50).unwrap().is_empty());
        assert_eq!(social::comments_on(&c, p, b, 0, 50).unwrap().iter().map(|x| (x.id, x.parent_id, x.image_id)).collect::<Vec<_>>(), vec![(k, None, b), (r, Some(k), b)]);
        assert_eq!(social::photo_reactions_of(&c, b, 10).unwrap().len(), 1);
        // A reply can't jump to another photo: there it is a comment of its own.
        let stray = social::comment_add_on(&c, p, a, brock, Some(k), "Wrong place", None, 16).unwrap().unwrap();
        assert_eq!(social::comment(&c, stray).unwrap().unwrap().parent_id, None);
        // A comment on the post itself is the post's.
        social::comment_add(&c, p, brock, None, "Great post", None, 17).unwrap().unwrap();
        assert_eq!(social::post(&c, 0, p).unwrap().unwrap().comment_n, 1);
        // What a notice says and where it leads: the post, or the post opened on that photo.
        assert_eq!((thing(&post, 0), thing(&post, b), link_of(p, 0), link_of(p, b)), ("your post", "your photo", format!("/posts/{p}"), format!("/posts/{p}?photo={b}")));
        // The photo as its own small post: which photo it is, its own counts, no menu, and it says which photo it is.
        let html = part(vec![Item { post: photo_view(&c, &post, &own, "Big Boys Vault").unwrap(), stores: vec![], binder: None }]).unwrap();
        assert!(html.contains(&format!("data-post=\"{p}\" data-image=\"{b}\"")) && html.contains("Photo 2 Of 3 · ") && html.contains("2 Comments") && !html.contains("data-post-menu") && html.contains("data-mine=\"love\""), "{html}");
        // And the post in the feed tells the page its photos have their own.
        let feed_post = part(vec![Item { post: post_view(&c, &post, "Big Boys Vault", 0).unwrap(), stores: vec![], binder: None }]).unwrap();
        assert!(feed_post.contains("class=\"post-media n3\" data-photo-threads") && !feed_post.contains("data-image="), "{feed_post}");
        // Deleting a comment on a photo (the reply under it goes too) leaves that photo's count right.
        assert_eq!(take_comment(&c, k, brock, "Brock", false).unwrap(), Some(Ok((0, vec![]))));
        assert_eq!(social::photo_own(&c, p, b, 0).unwrap().unwrap().comment_n, 0);
        let told: String = c.query_row("SELECT message || ' -> ' || link FROM notifications WHERE member_id=?1 ORDER BY id DESC LIMIT 1", [ash], |r| r.get(0)).unwrap();
        assert_eq!(told, format!("Brock deleted their comment on your photo: “This one!” -> /posts/{p}?photo={b}"));
        // Taking a photo off the post takes what was said about it, pictures included. The others keep theirs.
        social::comment_add_on(&c, p, b, brock, None, "Still love it", Some(("/uploads/k1_l.jpg", "/uploads/k1_s.jpg", 10)), 18).unwrap().unwrap();
        let gone = social::post_edit(&c, p, "Two pulls", &[a, ids[2]], &[], 19).unwrap();
        assert!(gone.contains(&"/uploads/k1_l.jpg".to_string()) && gone.contains(&"/uploads/p2_l.jpg".to_string()), "{gone:?}");
        assert!(social::photo_own(&c, p, b, 0).unwrap().is_none());
        assert_eq!(c.query_row("SELECT (SELECT COUNT(*) FROM photo_reactions WHERE image_id=?1) + (SELECT COUNT(*) FROM comments WHERE image_id=?1)", [b], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(social::photo_own(&c, p, a, 0).unwrap().unwrap().comment_n, 1, "what was said about the first photo is still there");
        // Down to one photo: one that has comments of its own keeps them.
        social::post_edit(&c, p, "One pull", &[a], &[], 20).unwrap();
        assert!(social::photo_own(&c, p, a, 0).unwrap().is_some());
        // The post goes: everything about its photos goes with it.
        social::photo_react(&c, p, a, brock, "like", 21).unwrap();
        social::post_remove(&c, p).unwrap();
        assert_eq!(c.query_row("SELECT (SELECT COUNT(*) FROM photo_reactions) + (SELECT COUNT(*) FROM comments WHERE post_id=?1)", [p], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }

    /// A person's posts on their own page (newest first, a few at a time, then "See More Posts"), and a caption on each
    /// photo, set when posting, when editing, and one at a time.
    #[test]
    fn a_persons_page_lists_their_posts_and_photos_take_captions() {
        let c = db();
        let (ash, _, brock) = town(&c);
        for n in 0..(PAGE_POSTS + 2) {
            social::post_add(&c, ash, "post", &format!("Pull {n}"), &[], &json!({}), 100 + n).unwrap();
        }
        social::post_add(&c, brock, "post", "Not Ash's", &[], &json!({}), 500).unwrap();
        let (items, next) = page_items(&c, 0, ash, None).unwrap();
        let texts: Vec<String> = items.iter().map(|i| i.post.as_ref().unwrap().text.clone()).collect();
        assert_eq!((texts.len() as i64, texts[0].as_str()), (PAGE_POSTS, "Pull 9"));
        assert!(next.starts_with(&format!("/posts/by/{ash}?c=")), "{next}");
        let html = page_html(items, next.clone()).unwrap();
        assert!(html.contains("See More Posts") && !html.contains("hx-trigger=\"revealed\""), "a page loads more by a button");
        let c2 = next.split("c=").nth(1).unwrap();
        let (t, id) = c2.split_once(':').unwrap();
        let (rest, end) = page_items(&c, 0, ash, Some((t.parse().unwrap(), id.parse().unwrap()))).unwrap();
        assert_eq!((rest.len(), end.as_str()), (2, ""));
        // Captions: given with the photos, changed on their own, kept or replaced on an edit.
        let pic = |n: i64| (format!("/uploads/p{n}_l.jpg"), format!("/uploads/p{n}_s.jpg"), 800, 600, 1000);
        let p = social::post_add(&c, ash, "post", "Two pulls", &[pic(1), pic(2)], &json!({}), 900).unwrap();
        social::caption_new_photos(&c, p, &[], &["The best one".into(), "  ".into()]).unwrap();
        assert_eq!(social::photo_captions(&c, p).unwrap(), vec!["The best one".to_string(), String::new()]);
        let ids: Vec<i64> = social::photos_ids(&c, p).unwrap().into_iter().map(|x| x.0).collect();
        assert!(social::set_photo_caption(&c, p, ids[1], "Second <b>one</b>").unwrap());
        assert!(!social::set_photo_caption(&c, p + 1000, ids[1], "Not here").unwrap(), "only on its own post");
        let post = social::post(&c, 0, p).unwrap().unwrap();
        let html = part(vec![Item { post: post_view(&c, &post, "Big Boys Vault", 0).unwrap(), stores: vec![], binder: None }]).unwrap();
        assert!(html.contains("data-cap=\"The best one\"") && html.contains("data-cap=\"Second &#60;b&#62;one&#60;/b&#62;\"") || html.contains("data-cap=\"Second &lt;b&gt;one&lt;/b&gt;\""), "{html}");
        social::post_edit(&c, p, "Two pulls", &[ids[0]], &[pic(3)], 901).unwrap();
        social::caption_new_photos(&c, p, &[ids[0]], &["The new one".into()]).unwrap();
        assert_eq!(social::photo_captions(&c, p).unwrap(), vec!["The best one".to_string(), "The new one".to_string()]);
    }

    #[test]
    fn the_vaults_posts_reach_its_admins_and_followers_hear_once_a_day() {
        let c = db();
        let (ash, _, brock) = town(&c);
        let boss = members::create(&c, "Boss", "h").unwrap().unwrap();
        c.execute("UPDATE members SET role='admin' WHERE id=?1", [boss]).unwrap();
        // A like on a post the vault wrote tells its admins; on a trainer's post, the trainer.
        let vault_post = social::post_add(&c, 0, "post", "New box!", &[], &json!({}), 10).unwrap();
        let own = social::post_add(&c, ash, "post", "My pulls", &[], &json!({}), 11).unwrap();
        assert_eq!(post_owners(&c, &social::post(&c, 0, vault_post).unwrap().unwrap()).unwrap(), vec![boss]);
        assert_eq!(post_owners(&c, &social::post(&c, 0, own).unwrap().unwrap()).unwrap(), vec![ash]);
        // Brock follows Ash's store, and the vault's.
        assert!(sellers::toggle_follow(&c, brock, ash).unwrap());
        assert!(sellers::toggle_follow(&c, brock, 0).unwrap());
        let now = 2_000_000_000;
        tell_followers(&c, ash, now).unwrap();
        tell_followers(&c, ash, now + 60).unwrap();
        tell_followers(&c, 0, now).unwrap();
        let lines = |c: &rusqlite::Connection| -> Vec<(String, String, i64)> {
            c.prepare("SELECT message, link, actor_id FROM notifications WHERE member_id=?1 AND kind='following' ORDER BY id").unwrap().query_map([brock], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect()
        };
        let ash_url = members::get(&c, ash).unwrap().unwrap().store_url();
        assert_eq!(lines(&c), vec![("Ash put up new cards.".to_string(), ash_url, ash), ("Big Boys Vault put up new cards.".to_string(), "/store/vault".to_string(), 0)], "one line a day per store");
        // Read, and a new day: news again. A store that closed tells nobody.
        members::mark_notifications_read(&c, brock, None).unwrap();
        tell_followers(&c, ash, now + 86_400).unwrap();
        assert_eq!(lines(&c).len(), 3);
        members::set_seller(&c, ash, false).unwrap();
        tell_followers(&c, ash, now + 2 * 86_400).unwrap();
        assert_eq!(lines(&c).len(), 3);
    }

    #[test]
    fn the_sets_after_the_posts_take_turns_between_every_store() {
        let c = db();
        let (ash, ids, brock) = town(&c);
        let nav = crate::chrome(&c).unwrap();
        let slug = members::get(&c, ash).unwrap().unwrap().slug;
        let rock = format!("{}/binders/rock", members::get(&c, brock).unwrap().unwrap().store_url());
        let mut shops = Shops::new(&c, &nav);
        // The first binder of each store in the Stores page's order (Ash, the vault, Brock), then the second of each.
        let turn: Vec<(String, String)> = in_turn(shops.all().unwrap()).iter().map(|s| (s.title.clone(), s.url.clone())).collect();
        assert_eq!(
            turn,
            vec![
                ("From Ash · Fire".to_string(), format!("/store/{slug}/binders/fire")),
                ("From Big Boys Vault · Pikachu Set".into(), "/collections/pikachu-set".into()),
                ("From Brock · Rock".into(), rock),
                ("From Ash · Water".into(), format!("/store/{slug}/binders/water")),
                ("From Big Boys Vault · Graded Cards".into(), "/collections/graded-cards".into()),
            ],
            "not the private binder, not the one with nothing for sale, not the vault's sold-out one"
        );
        // A page of the feed after the posts: two sets, the seller's cards opening in the card window.
        let (items, next) = build(&c, &nav, 0, Cursor::Binders(0), &mut shops).unwrap();
        assert_eq!(next, "v:2");
        let titles: Vec<&str> = items.iter().filter_map(|i| i.binder.as_ref()).map(|u| u.title.as_str()).collect();
        assert_eq!(titles, vec!["From Ash · Fire", "From Big Boys Vault · Pikachu Set"]);
        let fire = items[0].binder.as_ref().unwrap();
        assert_eq!(fire.tiles.iter().map(|t| t.url.clone()).collect::<Vec<_>>(), vec![format!("/trainers/{ash}/cards/{}", ids[0]), format!("/trainers/{ash}/cards/{}", ids[1])]);
        assert_eq!(fire.total, 2);
        let pika = items[1].binder.as_ref().unwrap();
        assert_eq!(pika.tiles.iter().map(|t| t.url.as_str()).collect::<Vec<_>>(), vec!["/products/c2", "/products/c1"], "cards for sale only");
        let html = feed_part(items, next);
        assert!(!html.contains('₱'), "the sets of cards show no prices: only where cards are shopped");
        assert!(html.contains(&format!("<a href=\"/store/{slug}/binders/fire\">See All 2</a>")), "{html}");
        assert!(html.contains(&format!("<a class=\"vu\" href=\"/trainers/{ash}/cards/{}\"", ids[0])));
        assert!(html.contains("<h2 class=\"h2\">From Big Boys Vault · Pikachu Set</h2>") && html.contains("<a href=\"/collections/pikachu-set\">See All 3</a>"));
        assert!(html.contains("hx-get=\"/feed?c=v:2\""));
        // The next page goes on where this one stopped; after the last binder, round again.
        let (items, next) = build(&c, &nav, 0, cursor("v:4"), &mut shops).unwrap();
        let titles: Vec<&str> = items.iter().filter_map(|i| i.binder.as_ref()).map(|u| u.title.as_str()).collect();
        assert_eq!((titles, next.as_str()), (vec!["From Big Boys Vault · Graded Cards", "From Ash · Fire"], "v:6"));
        // The admins' switch ends the feed instead.
        social::set_auto(&c, "vault", false, 1).unwrap();
        let (items, next) = build(&c, &nav, 0, Cursor::Binders(0), &mut shops).unwrap();
        assert!(items.is_empty() && next.is_empty());
    }

    #[test]
    fn the_vault_is_a_store_like_any_other_in_the_carousel_and_the_list() {
        let c = db();
        let (ash, _, brock) = town(&c);
        let nav = crate::chrome(&c).unwrap();
        let mut shops = Shops::new(&c, &nav);
        let views = store_views(shops.all().unwrap(), 0);
        let url = |id: i64| members::get(&c, id).unwrap().unwrap().store_url();
        assert_eq!(views.iter().map(|s| (s.id, s.name.clone(), s.url.clone())).collect::<Vec<_>>(), vec![(ash, "Ash".to_string(), url(ash)), (0, "Big Boys Vault".into(), "/store/vault".into()), (brock, "Brock".into(), url(brock))], "the Stores page's order");
        let v = &views[1];
        assert_eq!((v.avatar.as_str(), v.pic.as_str()), ("/static/img/logo-sm-144.png?v=1", "/uploads/c0-5_s.jpg"));
        assert!(v.sub.starts_with("3 On Sale · "), "{}", v.sub);
        assert_eq!(store_views(shops.all().unwrap(), 1)[0].id, 0, "a later carousel starts further along");
        // In the feed, after the second post: the vault's store is one of the stores, its Message button hidden from the
        // full admins (they are the vault).
        for t in [100, 200] {
            social::post_add(&c, 0, "post", &format!("Hello {t}"), &[], &json!({}), t).unwrap();
        }
        let (items, next) = build(&c, &nav, 0, Cursor::Top, &mut shops).unwrap();
        assert!(items[0].post.is_some() && items[1].post.is_some() && items[2].stores.len() == 3, "the carousel after the second post");
        assert_eq!(items[3].binder.as_ref().map(|u| u.title.as_str()), Some("From Ash · Fire"), "the posts are done: the sets begin");
        assert_eq!(next, "v:1");
        let html = feed_part(items, next);
        assert!(!html.contains('₱'), "no prices in the feed: only where cards are shopped");
        assert!(html.contains("<section class=\"box\" aria-label=\"Stores\">") && html.contains("<h2 class=\"h2\">Stores</h2>") && !html.contains("Trainers' stores"));
        assert!(html.contains("<a class=\"caro-pic\" href=\"/store/vault\">") && html.contains("data-dm-to=\"0\" data-not-full>Message</button>"), "{html}");
        assert!(html.contains(&format!("data-dm-to=\"{ash}\">Message</button>")));
        // The carousel comes back between the sets later on.
        let back = (1..40).step_by(2).any(|n| build(&c, &nav, 0, Cursor::Binders(n), &mut shops).unwrap().0.iter().any(|i| !i.stores.is_empty()));
        assert!(back);
    }

    #[test]
    fn the_vaults_new_cards_read_like_a_stores_listing_post() {
        let c = db();
        vault(&c);
        social::sync(&c, 1_794_400_000).unwrap();
        let p = social::feed(&c, 0, None, 50).unwrap().into_iter().find(|p| p.kind == "arrivals").expect("the vault's post");
        let v = post_view(&c, &p, "Big Boys Vault", 0).unwrap().unwrap();
        assert_eq!((v.who.name.as_str(), v.who.url.as_str()), ("Big Boys Vault", "/store/vault"));
        assert_eq!(v.headline, "listed 3 cards for sale");
        assert_eq!((v.link.as_str(), v.link_text.as_str()), ("/store/vault", "Visit The Store"));
        // A post the vault wrote itself still opens the home page.
        let id = social::post_add(&c, 0, "post", "Hello", &[], &json!({}), 100).unwrap();
        let p = social::post(&c, 0, id).unwrap().unwrap();
        assert_eq!(post_view(&c, &p, "Big Boys Vault", 0).unwrap().unwrap().who.url, "/");
    }

    #[test]
    fn the_welcome_line_counts_every_card_for_sale_on_the_site() {
        let c = db();
        let (ash, _, brock) = town(&c);
        // The vault's three copies, Ash's four cards for sale (not the reserved one), Brock's one.
        assert_eq!(cards_for_sale(&c).unwrap(), 8);
        c.execute("UPDATE members SET blocked=1 WHERE id=?1", [brock]).unwrap();
        assert_eq!(cards_for_sale(&c).unwrap(), 7, "a blocked store's cards aren't for sale");
        c.execute("UPDATE members SET status='Vacation' WHERE id=?1", [ash]).unwrap();
        assert_eq!(cards_for_sale(&c).unwrap(), 3, "nor a store on vacation");
        c.execute("UPDATE members SET status='Active' WHERE id=?1", [ash]).unwrap();
        let ended = c.query_row("SELECT id FROM member_cards WHERE member_id=?1 AND kind='sale' AND status='Active' LIMIT 1", [ash], |r| r.get::<_, i64>(0)).unwrap();
        c.execute("UPDATE member_cards SET expires_at=?2 WHERE id=?1", rusqlite::params![ended, members::now() - 60]).unwrap();
        assert_eq!(cards_for_sale(&c).unwrap(), 6, "nor a card past its end date");
        c.execute("UPDATE member_cards SET expires_at=0 WHERE id=?1", [ended]).unwrap();
        members::set_profile_ok(&c, ash, false).unwrap();
        assert_eq!(cards_for_sale(&c).unwrap(), 3, "nor a store that isn't approved");
    }

    #[test]
    fn with_nothing_to_show_the_feed_ends_instead_of_asking_forever() {
        let c = db();
        let nav = crate::chrome(&c).unwrap();
        let (items, next) = build(&c, &nav, 0, Cursor::Top, &mut Shops::new(&c, &nav)).unwrap();
        assert!(items.is_empty() && next.is_empty());
        // A store whose cards are in no binder has no sets either.
        let s = members::create(&c, "Misty", "h").unwrap().unwrap();
        members::set_seller(&c, s, true).unwrap();
        members::set_profile_ok(&c, s, true).unwrap();
        let Added::Yes(_) = sellers::add_listing(&c, s, "tcgdex:staryu", "Staryu", "1", "Set", "/p/staryu.webp", "").unwrap() else { panic!() };
        let mut shops = Shops::new(&c, &nav);
        assert_eq!(store_views(shops.all().unwrap(), 0).len(), 1, "it is still one of the stores");
        let (items, next) = build(&c, &nav, 0, Cursor::Binders(3), &mut shops).unwrap();
        assert!(items.is_empty() && next.is_empty());
    }

    #[test]
    fn a_binder_shows_its_next_cards_each_time_round() {
        assert_eq!(ring(&(1..=10).collect::<Vec<_>>(), 8, 8), vec![9, 10, 1, 2, 3, 4, 5, 6]);
        assert_eq!(ring(&[1, 2], 8, 8), vec![1, 2], "a small binder shows all it has");
        assert!(ring::<i32>(&[], 3, 8).is_empty());
        // A big vault binder goes a page at a time: 60 cards, the newest first.
        let c = db();
        c.execute("INSERT INTO collections(handle,title,position) VALUES('big','Big',0)", []).unwrap();
        for id in 1..=60 {
            c.execute("INSERT INTO products(id,handle,title,price,image,available,created_at) VALUES(?1,?2,?2,1,?3,1,'2026-01-01T00:00:00')", rusqlite::params![id, format!("c{id}"), format!("/uploads/c0-{id}_l.jpg")]).unwrap();
            c.execute("INSERT INTO collection_products(collection,product_id) VALUES('big',?1)", [id]).unwrap();
        }
        let set = |round: usize| vault_set(&c, "big", round).unwrap().iter().map(|t| t.url.trim_start_matches("/products/c").parse::<i64>().unwrap()).collect::<Vec<_>>();
        assert_eq!(set(0), (53..=60).rev().collect::<Vec<_>>());
        assert_eq!(set(1), (45..=52).rev().collect::<Vec<_>>());
        assert_eq!(set(6), (5..=12).rev().collect::<Vec<_>>(), "the second page");
        assert_eq!(set(7), vec![4, 3, 2, 1, 12, 11, 10, 9], "its last cards, then round");
        assert_eq!(set(12), set(0), "and back to the start");
    }
}
