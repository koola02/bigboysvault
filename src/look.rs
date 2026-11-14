//! The style guide: every piece of the site's look on one page (/admin/look), with the switches for the posts the
//! feed writes by itself. The site has one look; the older one, and the switch between the two, are gone.
use crate::{admin, with_db, App, Chrome, Res};
use askama::Template;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use marcuru_rs::db;

#[derive(Template)]
#[template(path = "look_guide.html")]
struct Guide {
    nav: Chrome,
    items: Vec<db::Product>,
    full: bool,
    /// Each kind of automatic post: (kind, what it is, on?).
    autos: Vec<(String, String, bool)>,
}

/// GET /admin/look: every piece of the look on one page, with the automatic posts' switches. Staff only.
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
    nav.seo.title = format!("The look · {}", nav.brand);
    let t = Guide { nav, items, full: matches!(power, admin::Power::Admin(_)), autos };
    Ok(crate::community::html(crate::community::render(t)?))
}
