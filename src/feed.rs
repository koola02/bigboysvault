//! The home feed: posts from marcuru_rs::social, drawn with live cards and prices, ten or so at a time.
//! It never ends: when the posts run out, the feed goes on with sets of cards from the stores' binders, the stores
//! taking turns (the vault's store is one of them, like any other), round and round. Between posts sit the stores
//! carousel and, at the very top, the showcase as a row of stories.
//!
//! GET /           the feed page (signed-out visitors get a kept copy; signed-in ones see their own reactions)
//! GET /feed?c=    the next items, as HTML (the page's last element asks for them when it scrolls into view)
//! POST /posts     a trainer's post (words, up to four photos, maybe a card)
//! POST /posts/{id}/react · GET|POST /posts/{id}/comments · POST /posts/{id}/delete|hide · POST /comments/{id}/delete
use crate::community::{acting, current, fail, gated, html, ok, photo_cap_mb, render, same_site, store_photo, TOO_BIG};
use crate::{admin, cached, messages, page, with_db, App, Chrome, Res, Seo};
use askama::Template;
use axum::{
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use marcuru_rs::{blocks, db, dm, members, sellers, social};
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

/// A store in the stores carousel and in the right column's list. `id` 0 is the vault's store.
#[derive(Clone)]
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

/// A seller's card for sale. Its link opens it in the card window.
fn listing_tile(seller: i64, k: &MemberCard) -> Tile {
    Tile { title: k.title.clone(), img: k.image.clone(), price: if k.has_price() { k.price_text() } else { "Ask".into() }, url: format!("/trainers/{seller}/cards/{}", k.id), c: k.cond_class().to_string(), cond: k.cond.clone(), sub: k.card_set.clone() }
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
        avatar: m.avatar.clone(),
        initial: m.initial(),
        url: if store { m.store_url() } else { format!("/trainers/{}", m.id) },
    }))
}

/// One post, drawn from what the site holds right now. None: nothing left to show (cards sold, trainer gone).
fn post_view(c: &rusqlite::Connection, p: &social::Post, brand: &str, viewer: i64) -> rusqlite::Result<Option<PostView>> {
    let Some(author) = who(c, p.member_id, brand, matches!(p.kind.as_str(), "listing" | "arrivals"))? else { return Ok(None) };
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
            v.link_text = "Visit the store".into();
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
            v.link_text = "Visit the store".into();
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

/// Every store on the Stores page, in its order (the one that last put up or changed a card for sale first,
/// `store::store_cards`): the vault's and the trainers', each worked out the same way.
fn shops(c: &rusqlite::Connection, nav: &Chrome) -> rusqlite::Result<Vec<Shop>> {
    let mut out = vec![];
    for s in crate::store::store_cards(c, &nav.brand)? {
        out.push(if s.owner == 0 { vault_shop(c, &nav.links, &s)? } else { seller_shop(c, &s)? });
    }
    Ok(out)
}

/// A store's tile in the carousel: its first card for sale (`pic`, `price`, `title`; no picture: its cover photo).
fn store_view(s: &StoreCard, pic: String, price: String, title: &str) -> StoreView {
    StoreView {
        id: s.owner,
        name: s.name.clone(),
        url: s.url.clone(),
        avatar: s.avatar.clone(),
        initial: s.initial.clone(),
        pic: if pic.is_empty() { s.cover.clone() } else { pic },
        price,
        sub: format!("{} on sale{}", s.on_sale_text(), if title.is_empty() { String::new() } else { format!(" · {title}") }),
    }
}

/// The vault's store: its first card for sale, and every binder with cards for sale in the owner's order (as its store
/// page shows them).
fn vault_shop(c: &rusqlite::Connection, links: &[db::Collection], s: &StoreCard) -> rusqlite::Result<Shop> {
    let first: Option<i64> = rusqlite::OptionalExtension::optional(c.query_row("SELECT id FROM products WHERE available=1 AND image<>'' ORDER BY position, created_at DESC, id DESC LIMIT 1", [], |r| r.get(0)))?;
    let view = match first.map(|id| db::product_by_id(c, id)).transpose()?.flatten() {
        Some(p) => store_view(s, p.img(360), if p.price > 0.0 { p.price_text() } else { String::new() }, &p.short_title()),
        None => store_view(s, String::new(), String::new(), ""),
    };
    // A binder has a set in the feed while one of its cards for sale has a picture.
    let peeks = db::binder_peeks(c, 1)?;
    let shelves = links
        .iter()
        .filter(|b| peeks.get(&b.handle).is_some_and(|(_, pics)| !pics.is_empty()))
        .map(|b| Shelf { title: format!("From {} · {}", s.name, b.title), url: format!("/collections/{}", b.handle), total: b.count, cards: ShelfCards::Vault(b.handle.clone()) })
        .collect();
    Ok(Shop { view, shelves })
}

/// A trainer's store: its first card for sale, and its public binders with cards for sale now, in the store's order.
fn seller_shop(c: &rusqlite::Connection, s: &StoreCard) -> rusqlite::Result<Shop> {
    let listings = sellers::storefront_listings(c, s.owner)?;
    let view = match listings.first() {
        Some(k) => store_view(s, k.image.clone(), if k.has_price() { k.price_text() } else { String::new() }, &k.title),
        None => store_view(s, String::new(), String::new(), ""),
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
    let page = |n: usize| db::list(c, &db::Query { collection: Some(handle.to_string()), q: String::new(), sort: String::new(), in_stock: true, page: n });
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
        let moves = match c.query_row("SELECT id FROM posts WHERE kind='prices' AND hidden=0 ORDER BY created_at DESC LIMIT 1", [], |r| r.get::<_, i64>(0)) {
            Ok(id) => match social::post(c, 0, id)? {
                Some(p) => post_view(c, &p, &nav.brand, 0)?.map(|v| v.moves).unwrap_or_default(),
                None => vec![],
            },
            Err(_) => vec![],
        };
        let stores = store_views(shops.all()?, 0).into_iter().take(6).collect();
        nav.seo = if single { Seo { title: format!("A post · {}", nav.brand), ..Default::default() } } else { seo(&nav) };
        let end = !single && next.is_empty();
        Ok(FeedPage { nav, stories, items, next, tagline, moves: moves.into_iter().take(4).collect(), stores, single, end })
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
        Ok(Some(v)) => Ok(html(part(vec![Item { post: Some(v), stores: vec![], binder: None }])?)),
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
        assert!(html.contains(&format!("<a href=\"/store/{slug}/binders/fire\">See all 2</a>")), "{html}");
        assert!(html.contains(&format!("<a class=\"vu\" href=\"/trainers/{ash}/cards/{}\"", ids[0])));
        assert!(html.contains("<h2 class=\"h2\">From Big Boys Vault · Pikachu Set</h2>") && html.contains("<a href=\"/collections/pikachu-set\">See all 3</a>"));
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
        assert_eq!((v.avatar.as_str(), v.pic.as_str(), v.price.as_str()), ("/static/img/logo-sm-144.png?v=1", "/uploads/c0-5_s.jpg", "₱9,000"));
        assert!(v.sub.starts_with("3 on sale · "), "{}", v.sub);
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
        assert_eq!((v.link.as_str(), v.link_text.as_str()), ("/store/vault", "Visit the store"));
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
