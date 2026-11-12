//! The site's two looks while the redesign is built: today's ("classic") and the new Facebook-style one.
//!
//! Every request carries one flag, "the new look": on when the visitor asked for it (the `bbv_look` cookie, set from
//! the style guide or My profile) or the owner switched everyone over (the `site_look` setting), **and** the page asked
//! for has been converted (READY). A page not converted yet always shows today's look, so nothing is half-styled.
//! Templates read the flag with `crate::look::is_new()`; `cached()` keeps the two looks apart.
//!
//! The flag lives in a task-local set by `layer` (around the whole request) and is copied into the database
//! thread by `with_db`, because some pages are rendered there.
use crate::{admin, with_db, App, Chrome, Res};
use askama::Template;
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use marcuru_rs::{blocks, db};
use serde::Deserialize;
use serde_json::json;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

tokio::task_local! { static TASK: bool; }
thread_local! { static THREAD: Cell<bool> = const { Cell::new(false) }; }

/// The new look for everyone (the setting `site_look` = "new"). Read at boot, changed by the owner.
pub static FOR_ALL: AtomicBool = AtomicBool::new(false);
pub const COOKIE: &str = "bbv_look";
pub const GUIDE: &str = "/admin/look";

/// Pages already converted. "/x" covers "/x" and everything under it; "=/x" covers only "/x" itself.
const READY: &[&str] = &[GUIDE, "=/", "/feed", "/posts", "/all", "/search", "/collections", "/products", "/trainers", "/stores", "/store", "/vouches", "=/ask", "=/chat", "=/login", "=/join", "=/signin", "=/me/messages", "=/admin/messages", "=/me/notifications", "=/admin", "=/admin/dashboard", "=/me/store", "=/me/listings",
    "=/admin/quick", "=/admin/sets", "=/admin/binders", "=/admin/orders", "=/admin/trainers", "=/admin/sellers", "=/admin/new", "/admin/edit", "=/me/binders", "=/me/orders", "=/me/settings"];

/// Is this request (or the page being rendered for it) in the new look?
pub fn is_new() -> bool {
    TASK.try_with(|v| *v).unwrap_or_else(|_| THREAD.with(Cell::get))
}

/// For `with_db`: the database thread renders some pages, so it is told the look too.
pub fn in_thread(on: bool) {
    THREAD.with(|t| t.set(on));
}

/// What the visitor chose on this device, if anything.
fn chosen(h: &HeaderMap) -> Option<bool> {
    let all = h.get(header::COOKIE)?.to_str().ok()?;
    all.split(';').map(str::trim).find_map(|kv| kv.strip_prefix("bbv_look=")).map(|v| v == "new")
}

pub fn wanted(h: &HeaderMap) -> bool {
    chosen(h).unwrap_or_else(|| FOR_ALL.load(Ordering::Relaxed))
}

pub fn ready(path: &str) -> bool {
    READY.iter().any(|p| match p.strip_prefix('=') {
        Some(only) => path == only,
        None => path == *p || path.strip_prefix(p).is_some_and(|rest| rest.starts_with('/')),
    })
}

/// Wraps every request: works out the look once, and everything inside can ask for it.
pub async fn layer(req: Request, next: Next) -> Response {
    let path = req.uri().path();
    // The style guide is always the new look: it is where the look is shown and switched on.
    let on = path == GUIDE || (wanted(req.headers()) && ready(path));
    TASK.scope(on, next.run(req)).await
}

/// Read once when the site starts.
pub fn load(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    FOR_ALL.store(blocks::setting(c, "site_look")?.as_deref() == Some("new"), Ordering::Relaxed);
    Ok(())
}

#[derive(Template)]
#[template(path = "look_guide.html")]
struct Guide {
    nav: Chrome,
    items: Vec<db::Product>,
    mine: bool,
    all: bool,
    full: bool,
    /// Each kind of automatic post: (kind, what it is, on?).
    autos: Vec<(String, String, bool)>,
}

/// GET /admin/look: every piece of the new look on one page, for sign-off, with the switches.
pub async fn guide(State(app): State<App>, h: HeaderMap) -> Res {
    let power = admin::power(&app, &h);
    if power == admin::Power::Nothing {
        return Ok((StatusCode::NOT_FOUND, "Not found").into_response());
    }
    let (mut nav, items, autos) = with_db(&app, |c| {
        let nav = crate::chrome(c)?;
        let q = db::Query { collection: None, q: String::new(), sort: String::new(), in_stock: true, page: 0 };
        let (items, _) = db::list(c, &q)?;
        let off = marcuru_rs::social::auto_off(c)?;
        let autos = marcuru_rs::social::AUTO.iter().map(|(k, label)| (k.to_string(), label.to_string(), !off.iter().any(|o| o == k))).collect::<Vec<_>>();
        Ok((nav, items.into_iter().take(8).collect::<Vec<_>>(), autos))
    })
    .await?;
    nav.seo.title = format!("The new look · {}", nav.brand);
    let t = Guide { nav, items, mine: chosen(&h) == Some(true), all: FOR_ALL.load(Ordering::Relaxed), full: matches!(power, admin::Power::Admin(_)), autos };
    Ok(crate::community::html(crate::community::render(t)?))
}

#[derive(Deserialize)]
pub struct Pick {
    look: String,
}

/// POST /me/look {look: "new" | "classic"}: this device's choice (staff only while the new look is being built).
pub async fn pick(State(app): State<App>, h: HeaderMap, Json(req): Json<Pick>) -> Res {
    if !crate::community::same_site(&h) {
        return Ok(crate::community::fail(StatusCode::FORBIDDEN, "That request didn't come from this site."));
    }
    if admin::power(&app, &h) == admin::Power::Nothing && !FOR_ALL.load(Ordering::Relaxed) {
        return Ok(crate::community::fail(StatusCode::FORBIDDEN, "The new look isn't open to everyone yet."));
    }
    let v = if req.look == "new" { "new" } else { "classic" };
    let https = h.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) == Some("https");
    let cookie = format!("{COOKIE}={v}; Path=/; Max-Age=31536000; SameSite=Lax{}", if https { "; Secure" } else { "" });
    Ok(([(header::SET_COOKIE, cookie)], crate::community::ok(json!({ "ok": true, "look": v }))).into_response())
}

#[derive(Deserialize)]
pub struct ForAll {
    on: bool,
}

/// POST /admin/look/all {on}: the new look for every visitor (full admins). Pages not converted yet stay as they are.
pub async fn for_all(State(app): State<App>, h: HeaderMap, Json(req): Json<ForAll>) -> Res {
    if !crate::community::same_site(&h) || !matches!(admin::power(&app, &h), admin::Power::Admin(_)) {
        return Ok(crate::community::fail(StatusCode::FORBIDDEN, "Only a full admin can switch the look for everyone."));
    }
    let on = req.on;
    with_db(&app, move |c| blocks::set_setting(c, "site_look", if on { "new" } else { "classic" })).await?;
    FOR_ALL.store(on, Ordering::Relaxed);
    app.forget();
    Ok(crate::community::ok(json!({ "ok": true, "all": on })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_converted_pages_change() {
        assert!(ready("/admin/look"));
        assert!(!ready("/admin/looks"));
        assert!(ready("/"), "the feed is home");
        assert!(ready("/posts/12"));
        assert!(ready("/all") && ready("/products/pikachu-1") && ready("/collections/graded-cards"));
        assert!(ready("/trainers") && ready("/trainers/4/cards/9") && ready("/store/hayme") && ready("/stores") && ready("/vouches"));
        assert!(ready("/ask") && !ready("/ask/more"));
        assert!(ready("/chat") && !ready("/chat/messages"));
        assert!(ready("/login") && ready("/join") && ready("/signin") && ready("/me/messages") && ready("/me/notifications"));
        assert!(ready("/admin") && ready("/admin/dashboard") && ready("/me/store") && ready("/me/listings"));
        // The Manager: every tool in the admin's and the seller's rail.
        assert!(ready("/admin/quick") && ready("/admin/sets") && ready("/admin/binders") && ready("/admin/orders") && ready("/admin/trainers") && ready("/admin/sellers"));
        assert!(ready("/admin/new") && ready("/admin/edit/12") && ready("/me/binders") && ready("/me/orders") && ready("/me/settings"));
        // The home page editor is today's home page itself, so it stays in today's look. So does what only answers with data.
        assert!(!ready("/me") && !ready("/storefront") && !ready("/admin/page") && !ready("/admin/page/list") && !ready("/admin/save"));
        assert!(!ready("/admin/quick-edit") && !ready("/admin/sets/search") && !ready("/admin/binders/pikachu-set/covers") && !ready("/admin/editor"));
    }

    #[test]
    fn the_cookie_is_this_devices_choice() {
        let mut h = HeaderMap::new();
        assert_eq!(chosen(&h), None);
        h.insert(header::COOKIE, "a=1; bbv_look=new; b=2".parse().unwrap());
        assert_eq!(chosen(&h), Some(true));
        h.insert(header::COOKIE, "bbv_look=classic".parse().unwrap());
        assert_eq!(chosen(&h), Some(false));
        assert!(!wanted(&h), "a device that chose today's look keeps it, even once everyone is switched");
    }

    #[tokio::test]
    async fn the_flag_follows_the_request() {
        assert!(!is_new());
        TASK.scope(true, async { assert!(is_new()) }).await;
        in_thread(true);
        assert!(is_new(), "the database thread is told too");
        in_thread(false);
    }
}
