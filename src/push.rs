//! Web Push: how a call or a message reaches a phone or computer while the site isn't open.
//!
//! A browser that allows it gives the site a "subscription": an address at its maker's push service (Google, Apple,
//! Mozilla, Microsoft) and two keys. The server encrypts a short message for that browser alone (RFC 8291, aes128gcm)
//! and posts it to the address, signed with the site's own key (RFC 8292, VAPID). The push service wakes the phone and
//! the site's service worker (static/sw.js) shows the notification. Nobody but that browser can read the message.
//!
//! The site's key pair is made once and kept in the settings table, so there is nothing to set up. VAPID_PUBLIC_KEY and
//! VAPID_PRIVATE_KEY (base64url) override it when both are set.
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use hkdf::Hkdf;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};
use rand_core::{OsRng, RngCore};
use rusqlite::Connection;
use serde_json::{json, Value};
use sha2::Sha256;
use std::sync::OnceLock;
use std::time::Duration;

/// The site's VAPID key pair.
pub struct Vapid {
    secret: SecretKey,
    /// The public key as browsers want it: the uncompressed point, base64url.
    pub public: String,
}

impl Vapid {
    fn from_secret(secret: SecretKey) -> Self {
        let public = B64.encode(secret.public_key().to_encoded_point(false).as_bytes());
        Self { secret, public }
    }
}

static KEYS: OnceLock<Vapid> = OnceLock::new();

/// Loads the site's key pair (made and saved on the first start), once per run. Called at start-up.
pub fn init(c: &Connection) -> rusqlite::Result<()> {
    if KEYS.get().is_some() {
        return Ok(());
    }
    let from_env = std::env::var("VAPID_PRIVATE_KEY").ok().and_then(|k| B64.decode(k.trim().trim_end_matches('=')).ok()).and_then(|b| SecretKey::from_slice(&b).ok());
    let v = match from_env {
        Some(k) => Vapid::from_secret(k),
        None => {
            let saved = marcuru_rs::blocks::setting(c, "vapid_private")?.and_then(|k| B64.decode(k.trim()).ok()).and_then(|b| SecretKey::from_slice(&b).ok());
            match saved {
                Some(k) => Vapid::from_secret(k),
                None => {
                    let k = SecretKey::random(&mut OsRng);
                    marcuru_rs::blocks::set_setting(c, "vapid_private", &B64.encode(k.to_bytes()))?;
                    Vapid::from_secret(k)
                }
            }
        }
    };
    let _ = KEYS.set(v);
    Ok(())
}

/// The public key pages subscribe with ("" before start-up loaded it).
pub fn public_key() -> &'static str {
    KEYS.get().map(|k| k.public.as_str()).unwrap_or("")
}

// ---------- which push services the server will post to ----------

/// The push services of the browsers people use. The server only ever posts to these, never to an address someone
/// typed in, so a made-up subscription can't make the server call another computer. The address is read the way the
/// HTTP client reads it (a WHATWG URL), so tricks like a backslash before the real host don't get through, and it must
/// already be in its plain, canonical form.
pub fn push_url(endpoint: &str) -> Option<reqwest::Url> {
    if endpoint.len() > 1000 || endpoint.chars().any(|c| c == '\\' || c.is_whitespace() || c.is_control()) {
        return None;
    }
    let url = reqwest::Url::parse(endpoint).ok()?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return None;
    }
    // domain() is None for an IP address.
    let host = url.domain()?;
    // Chrome, Android, Edge on Android, Samsung Internet, Opera, Brave: Google's FCM (Chrome Beta, Dev and Canary at
    // jmt17.google.com). Firefox: Mozilla's autopush. Safari on Mac, iPhone and iPad: Apple. Edge on Windows: WNS.
    const EXACT: [&str; 4] = ["fcm.googleapis.com", "android.googleapis.com", "jmt17.google.com", "web.push.apple.com"];
    const SUFFIX: [&str; 3] = [".push.services.mozilla.com", ".push.apple.com", ".notify.windows.com"];
    let known = EXACT.contains(&host) || SUFFIX.iter().any(|s| host.ends_with(s) && host.len() > s.len());
    (known && url.as_str() == endpoint).then_some(url)
}

pub fn allowed_endpoint(endpoint: &str) -> bool {
    push_url(endpoint).is_some()
}

/// A subscription's keys, checked: the browser's public key (a P-256 point) and its 16-byte secret.
pub fn valid_keys(p256dh: &str, auth: &str) -> bool {
    let (Ok(k), Ok(a)) = (B64.decode(p256dh.trim().trim_end_matches('=')), B64.decode(auth.trim().trim_end_matches('='))) else { return false };
    k.len() == 65 && PublicKey::from_sec1_bytes(&k).is_ok() && a.len() == 16
}

// ---------- RFC 8291: the message, readable by that one browser only ----------

/// How long one record may be. One record carries the whole message (they are well under 4 KB).
const RECORD_SIZE: u32 = 4096;
/// The most a push service has to accept, header included.
const MAX_BODY: usize = 4096;

/// Encrypts `plain` for a browser (`ua_public`: its P-256 key, `auth`: its 16-byte secret) with a one-off key pair and
/// salt. Returns the request body: the aes128gcm header (salt, record size, the one-off public key), then the record.
pub fn encrypt_with(ua_public: &[u8], auth: &[u8], plain: &[u8], one_off: &SecretKey, salt: &[u8; 16]) -> Result<Vec<u8>, &'static str> {
    let ua = PublicKey::from_sec1_bytes(ua_public).map_err(|_| "bad browser key")?;
    let as_public = one_off.public_key().to_encoded_point(false);
    let shared = p256::ecdh::diffie_hellman(one_off.to_nonzero_scalar(), ua.as_affine());
    // IKM = HKDF(salt = auth, ikm = ECDH, info = "WebPush: info\0" || ua_public || as_public, 32 bytes).
    let mut info = b"WebPush: info\0".to_vec();
    info.extend_from_slice(ua_public);
    info.extend_from_slice(as_public.as_bytes());
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes()).expand(&info, &mut ikm).map_err(|_| "hkdf")?;
    // The content key and nonce, from the salt.
    let prk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
    prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).map_err(|_| "hkdf")?;
    prk.expand(b"Content-Encoding: nonce\0", &mut nonce).map_err(|_| "hkdf")?;
    // One record, the last: the message then the 0x02 delimiter.
    let mut record = plain.to_vec();
    record.push(2);
    // Push services only promise to carry 4096 bytes in all: the 86-byte header, the message, its delimiter and the
    // 16-byte seal. That leaves 3993 bytes for the message (RFC 8291 section 4).
    if 86 + record.len() + 16 > MAX_BODY {
        return Err("message too long");
    }
    let sealed = Aes128Gcm::new_from_slice(&cek).map_err(|_| "key")?.encrypt(Nonce::from_slice(&nonce), record.as_slice()).map_err(|_| "seal")?;
    let mut body = Vec::with_capacity(86 + sealed.len());
    body.extend_from_slice(salt);
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(65);
    body.extend_from_slice(as_public.as_bytes());
    body.extend_from_slice(&sealed);
    Ok(body)
}

/// As `encrypt_with`, with a fresh one-off key and salt (always, in real use).
pub fn encrypt(ua_public: &[u8], auth: &[u8], plain: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    encrypt_with(ua_public, auth, plain, &SecretKey::random(&mut OsRng), &salt)
}

// ---------- RFC 8292: the site signs each request ----------

/// The push service's own address, which the signature is for.
fn audience(url: &reqwest::Url) -> String {
    url.origin().ascii_serialization()
}

/// A signed token for one push service, good until `exp` (at most a day ahead).
pub fn jwt(key: &SecretKey, aud: &str, exp: i64, sub: &str) -> String {
    let head = B64.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
    let claims = B64.encode(json!({ "aud": aud, "exp": exp, "sub": sub }).to_string());
    let signing = format!("{head}.{claims}");
    let sig: Signature = SigningKey::from(key).sign(signing.as_bytes());
    format!("{signing}.{}", B64.encode(sig.to_bytes()))
}

// ---------- sending ----------

/// How a push should travel.
#[derive(Clone)]
pub struct Opts {
    /// Seconds the push service may hold it for a phone that is off or asleep. Never 0: Apple refuses it, and the
    /// others would drop a message for a phone that is out of reach for a moment.
    pub ttl: u32,
    /// "high" wakes a sleeping phone at once (calls, messages); "normal" may wait for the phone's next wake-up.
    pub urgency: &'static str,
    /// Pushes with the same topic replace each other while still waiting at the push service, so a phone that was out
    /// of reach gets only the latest (a call's "missed" instead of its rings). Letters and digits, 32 at most.
    /// What the topic is about (a call or chat). It is never sent as it is: each browser gets its own unreadable
    /// version of it, so a push service can't tell which pushes belong to the same conversation.
    pub topic: Option<String>,
    /// One more try when the push service is busy. Off for a call's rings: the ring loop sends the next one itself,
    /// and a late retry must never land after the call's last word.
    pub retry: bool,
}

impl Opts {
    pub fn new(ttl: u32, urgency: &'static str, topic: Option<String>) -> Self {
        let topic = topic.map(|t| t.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(64).collect::<String>()).filter(|t| !t.is_empty());
        Self { ttl: ttl.max(1), urgency, topic, retry: true }
    }
    pub fn once(mut self) -> Self {
        self.retry = false;
        self
    }
}

/// The Topic a push service sees: 32 characters made from the site's key, the browser's address and what it is about.
/// The same for one browser and one conversation (so pushes still replace each other), meaningless to anyone else,
/// and different for the two people in a conversation.
fn opaque_topic(key: &SecretKey, endpoint: &str, about: &str) -> String {
    let mut msg = endpoint.as_bytes().to_vec();
    msg.push(0);
    msg.extend_from_slice(about.as_bytes());
    let (mac, _) = Hkdf::<Sha256>::extract(Some(key.to_bytes().as_slice()), &msg);
    B64.encode(&mac[..24])
}

/// What became of one push.
#[derive(Debug, PartialEq)]
pub enum Sent {
    Ok,
    /// The browser unsubscribed or the address expired: forget it.
    Gone,
    /// The push service refused it (400, 401, 403): usually a key that no longer matches. A few in a row and the
    /// subscription is forgotten, so a device that can't be reached stops counting as reachable.
    Refused(String),
    Failed(String),
}

/// One subscription: (row id, endpoint, browser key, browser secret).
pub type Sub = (i64, String, String, String);

pub fn subscriptions(c: &Connection, members: &[i64]) -> rusqlite::Result<Vec<Sub>> {
    if members.is_empty() {
        return Ok(vec![]);
    }
    let list = members.iter().map(|m| m.to_string()).collect::<Vec<_>>().join(",");
    c.prepare(&format!("SELECT id, endpoint, p256dh, auth FROM push_subscriptions WHERE member_id IN ({list}) ORDER BY id"))?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect()
}

/// Whether anyone among these members can be reached by push.
pub fn reachable(c: &Connection, members: &[i64]) -> rusqlite::Result<bool> {
    Ok(!subscriptions(c, members)?.is_empty())
}

/// The address that signs the site's pushes (the site's own address, never a person's).
fn subject() -> String {
    std::env::var("PUBLIC_URL").ok().filter(|u| u.starts_with("https://")).map(|u| u.trim_end_matches('/').to_string()).unwrap_or_else(|| "https://www.bigboysvault.com".into())
}

/// The HTTP client for push services: it never follows a redirect (a push service doesn't send one; anything that
/// does is not one).
pub fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(10)).user_agent("bigboysvault push").build().expect("push client")
    })
}

/// Posts one encrypted message to one browser.
pub async fn send_one(endpoint: &str, p256dh: &str, auth: &str, msg: &Value, o: &Opts) -> Sent {
    let Some(keys) = KEYS.get() else { return Sent::Failed("no key yet".into()) };
    let Some(url) = push_url(endpoint) else { return Sent::Gone };
    let (Ok(ua), Ok(secret)) = (B64.decode(p256dh.trim().trim_end_matches('=')), B64.decode(auth.trim().trim_end_matches('='))) else { return Sent::Gone };
    let body = match encrypt(&ua, &secret, msg.to_string().as_bytes()) {
        Ok(b) => b,
        Err("bad browser key") => return Sent::Gone,
        Err(e) => return Sent::Failed(e.into()),
    };
    let token = token_for(&keys.secret, &audience(&url));
    let topic = o.topic.as_deref().map(|t| opaque_topic(&keys.secret, endpoint, t));
    let tries = if o.retry { 2 } else { 1 };
    for attempt in 0..tries {
        let mut req = client()
            .post(url.clone())
            .header("TTL", o.ttl.to_string())
            .header("Urgency", o.urgency)
            .header("Content-Encoding", "aes128gcm")
            .header("Content-Type", "application/octet-stream")
            .header("Authorization", format!("vapid t={token}, k={}", keys.public))
            .body(body.clone());
        if let Some(t) = &topic {
            req = req.header("Topic", t.as_str());
        }
        let last = attempt + 1 == tries;
        let wait = match req.send().await {
            Ok(r) if r.status().is_success() => return Sent::Ok,
            // 404 and 410: the subscription has ended (the browser unsubscribed, the app was removed, it expired).
            Ok(r) if r.status().as_u16() == 404 || r.status().as_u16() == 410 => return Sent::Gone,
            Ok(r) if matches!(r.status().as_u16(), 400 | 401 | 403) => {
                return Sent::Refused(format!("{} {}", r.status(), r.text().await.unwrap_or_default().chars().take(200).collect::<String>()));
            }
            Ok(r) if r.status().as_u16() == 429 || r.status().is_server_error() => {
                if last {
                    return Sent::Failed(format!("{}", r.status()));
                }
                r.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(2).clamp(1, 8)
            }
            Ok(r) => return Sent::Failed(format!("{} {}", r.status(), r.text().await.unwrap_or_default().chars().take(200).collect::<String>())),
            Err(e) if last => return Sent::Failed(e.to_string()),
            Err(_) => 2,
        };
        tokio::time::sleep(Duration::from_secs(wait)).await;
    }
    Sent::Failed("gave up".into())
}

/// The signed token for a push service, made again only every few hours (Apple refuses more than one an hour).
fn token_for(key: &SecretKey, aud: &str) -> String {
    static TOKENS: OnceLock<std::sync::Mutex<std::collections::HashMap<String, (i64, String)>>> = OnceLock::new();
    let now = crate::unix_now();
    let map = TOKENS.get_or_init(Default::default);
    let mut m = match map.lock() {
        Ok(m) => m,
        Err(p) => p.into_inner(),
    };
    if let Some((exp, t)) = m.get(aud) {
        if *exp - now > 3600 {
            return t.clone();
        }
    }
    let exp = now + 12 * 3600;
    let t = jwt(key, aud, exp, &subject());
    m.insert(aud.to_string(), (exp, t.clone()));
    t
}

/// How many refusals in a row before a subscription is forgotten.
const REFUSALS: i64 = 3;

/// Sends a message to every browser these members turned alerts on in, all at once. Ended subscriptions are
/// forgotten, and so are ones refused a few times in a row. Returns how many browsers took it.
pub async fn to_members(app: &crate::App, members: Vec<i64>, msg: Value, o: Opts) -> usize {
    let Ok(subs) = crate::with_db(app, move |c| subscriptions(c, &members)).await else { return 0 };
    if subs.is_empty() {
        return 0;
    }
    let msg = std::sync::Arc::new(msg);
    let mut set = tokio::task::JoinSet::new();
    for (id, endpoint, key, auth) in subs {
        let (msg, o) = (msg.clone(), o.clone());
        set.spawn(async move {
            let r = send_one(&endpoint, &key, &auth, &msg, &o).await;
            (id, endpoint, r)
        });
    }
    let (mut took, mut fine, mut gone, mut refused) = (0, vec![], vec![], vec![]);
    while let Some(done) = set.join_next().await {
        let Ok((id, endpoint, r)) = done else { continue };
        let host = endpoint.split('/').nth(2).unwrap_or("?").to_string();
        match r {
            Sent::Ok => {
                took += 1;
                fine.push(id);
            }
            Sent::Gone => gone.push(id),
            Sent::Refused(why) => {
                tracing::warn!("push to {host} refused: {why}");
                refused.push(id);
            }
            Sent::Failed(why) => tracing::warn!("push to {host} failed: {why}"),
        }
    }
    let _ = crate::with_db(app, move |c| {
        for id in &fine {
            c.execute("UPDATE push_subscriptions SET fails=0 WHERE id=?1 AND fails>0", [id])?;
        }
        for id in &refused {
            c.execute("UPDATE push_subscriptions SET fails=fails+1 WHERE id=?1", [id])?;
            c.execute("DELETE FROM push_subscriptions WHERE id=?1 AND fails>=?2", rusqlite::params![id, REFUSALS])?;
        }
        for id in &gone {
            c.execute("DELETE FROM push_subscriptions WHERE id=?1", [id])?;
        }
        Ok(())
    })
    .await;
    took
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_browsers_push_services_are_allowed() {
        for ok in [
            "https://fcm.googleapis.com/fcm/send/abc:def",
            "https://updates.push.services.mozilla.com/wpush/v2/gAAA",
            "https://jmt17.google.com/fcm/send/abc",
            "https://web.push.apple.com/QK7vY",
            "https://api.push.apple.com/3/device/x",
            "https://wns2-par02p.notify.windows.com/w/?token=BQYAAA",
        ] {
            assert!(allowed_endpoint(ok), "{ok}");
        }
        for bad in [
            "https://attacker.example\\.push.apple.com/x",
            "https://attacker.example\\@fcm.googleapis.com/x",
            "https://fcm.googleapis.com\\@attacker.example/x",
            "https://FCM.googleapis.com/fcm/send/abc",
            "https://fcm.googleapis.com./fcm/send/abc",
            "https://fcm.googleapis.com/fcm/send/a b",
            "https://fcm.googleapis.com:443/fcm/send/abc",
            "https://[::1]/x",
            "https://10.0.0.1/x",
            "https://%66cm.googleapis.com/x",
            "https://xn--fcm-googleapis.com/x",
            "https://fcm.googleapis.com",
            "http://fcm.googleapis.com/fcm/send/abc",
            "https://fcm.googleapis.com.evil.example/x",
            "https://evil.example/fcm.googleapis.com",
            "https://user@fcm.googleapis.com/x",
            "https://fcm.googleapis.com:8443/x",
            "https://127.0.0.1/x",
            "https://localhost/x",
            "https://push.apple.com/x",
            "https://notify.windows.com/x",
            "https://evilpush.apple.com.example/x",
            "",
        ] {
            assert!(!allowed_endpoint(bad), "{bad}");
        }
    }

    /// RFC 8291 Appendix A: the worked example, byte for byte (keys, salt and message as printed there).
    #[test]
    fn the_encryption_matches_the_standards_worked_example() {
        let d = |s: &str| B64.decode(s).unwrap();
        let as_private = SecretKey::from_slice(&d("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw")).unwrap();
        assert_eq!(
            B64.encode(as_private.public_key().to_encoded_point(false).as_bytes()),
            "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8"
        );
        let ua_public = d("BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4");
        let auth = d("BTBZMqHH6r4Tts7J_aSIgg");
        let salt: [u8; 16] = d("DGv6ra1nlYgDCS1FRnbzlw").try_into().unwrap();
        let plain = d("V2hlbiBJIGdyb3cgdXAsIEkgd2FudCB0byBiZSBhIHdhdGVybWVsb24");
        assert_eq!(plain, b"When I grow up, I want to be a watermelon");
        let body = encrypt_with(&ua_public, &auth, &plain, &as_private, &salt).unwrap();
        assert_eq!(body.len(), 144, "86-byte header and a 58-byte record (the RFC's 145 is erratum 5230)");
        assert_eq!(
            B64.encode(&body),
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
        );
        // A real message gets a fresh one-off key and salt each time.
        assert_ne!(encrypt(&ua_public, &auth, &plain).unwrap(), encrypt(&ua_public, &auth, &plain).unwrap());
        assert_eq!(encrypt(&ua_public, &auth, &vec![b'x'; 3993]).unwrap().len(), 4096, "the longest message fills 4096 bytes");
        assert!(encrypt(&ua_public, &auth, &vec![b'x'; 3994]).is_err(), "one byte more is refused");
    }

    #[test]
    fn a_push_service_sees_a_topic_it_cannot_read() {
        let k = SecretKey::random(&mut OsRng);
        let a = opaque_topic(&k, "https://fcm.googleapis.com/fcm/send/one", "call42");
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_eq!(a, opaque_topic(&k, "https://fcm.googleapis.com/fcm/send/one", "call42"), "the same for one browser and one call");
        assert_ne!(a, opaque_topic(&k, "https://fcm.googleapis.com/fcm/send/two", "call42"), "the other person's browser gets another");
        assert_ne!(a, opaque_topic(&k, "https://fcm.googleapis.com/fcm/send/one", "m42"), "a chat and a call differ");
        assert!(!a.contains("42"));
    }

    #[test]
    fn a_topic_is_short_and_plain() {
        assert_eq!(Opts::new(0, "high", Some("call42".into())).topic.as_deref(), Some("call42"));
        assert_eq!(Opts::new(0, "high", None).ttl, 1, "never 0");
        assert_eq!(Opts::new(5, "high", Some("a b/c!".into())).topic.as_deref(), Some("abc"));
        assert!(Opts::new(5, "high", None).retry && !Opts::new(5, "high", None).once().retry);
        assert_eq!(Opts::new(5, "high", Some("  ".into())).topic, None);
    }

    #[test]
    fn a_browser_key_is_checked() {
        let k = SecretKey::random(&mut OsRng);
        let public = B64.encode(k.public_key().to_encoded_point(false).as_bytes());
        assert!(valid_keys(&public, &B64.encode([7u8; 16])));
        assert!(valid_keys(&format!("{public}="), &format!("{}==", B64.encode([7u8; 16]))), "padding is fine");
        assert!(!valid_keys(&public, &B64.encode([7u8; 15])), "secret too short");
        assert!(!valid_keys(&B64.encode([4u8; 65]), &B64.encode([7u8; 16])), "not a point on the curve");
        assert!(!valid_keys("not base64!", &B64.encode([7u8; 16])));
    }

    #[test]
    fn the_signature_is_the_sites_and_says_who_it_is_for() {
        let k = SecretKey::random(&mut OsRng);
        let t = jwt(&k, "https://fcm.googleapis.com", 1_800_000_000, "https://www.bigboysvault.com");
        let parts: Vec<&str> = t.split('.').collect();
        assert_eq!(parts.len(), 3);
        let head: Value = serde_json::from_slice(&B64.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(head, json!({ "typ": "JWT", "alg": "ES256" }));
        let claims: Value = serde_json::from_slice(&B64.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims, json!({ "aud": "https://fcm.googleapis.com", "exp": 1_800_000_000, "sub": "https://www.bigboysvault.com" }));
        let sig = B64.decode(parts[2]).unwrap();
        assert_eq!(sig.len(), 64, "raw r and s");
        use p256::ecdsa::signature::Verifier;
        let vk = p256::ecdsa::VerifyingKey::from(&SigningKey::from(&k));
        assert!(vk.verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &Signature::from_slice(&sig).unwrap()).is_ok());
        assert_eq!(audience(&push_url("https://fcm.googleapis.com/fcm/send/x?y").unwrap()), "https://fcm.googleapis.com");
    }

    #[test]
    fn the_key_pair_is_made_once_and_kept() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)").unwrap();
        // init() fills a process-wide slot; the saving it does is checked here directly.
        let k = SecretKey::random(&mut OsRng);
        marcuru_rs::blocks::set_setting(&c, "vapid_private", &B64.encode(k.to_bytes())).unwrap();
        let back = marcuru_rs::blocks::setting(&c, "vapid_private").unwrap().unwrap();
        let again = Vapid::from_secret(SecretKey::from_slice(&B64.decode(back).unwrap()).unwrap());
        assert_eq!(again.public, Vapid::from_secret(k).public);
        assert_eq!(B64.decode(&again.public).unwrap().len(), 65);
    }
}
