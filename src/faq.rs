//! The FAQs: the questions and the site's own answers (the owner approved them, 20 Nov 2026). One list for the FAQs
//! page (templates/faq.html), Ask The Vault's AI (its brief carries them all) and Ask The Vault's own answers (`find`
//! picks the question asked). Change the words here; the page and Ask The Vault follow. An answer is a little HTML:
//! paragraphs, <b> and lists, written here, never typed by anyone on the site.

pub struct Q {
    pub q: &'static str,
    /// The answer as the page shows it (trusted HTML: <p>, <b>, <ul>, <li>).
    pub a: &'static str,
}

pub struct Section {
    pub title: &'static str,
    pub qs: &'static [Q],
}

pub static EVERYONE: &[Section] = &[
    Section { title: "Getting Started", qs: &[
        Q { q: "How Do I Make An Account?", a: "<p>Tap Register, pick a trainer name and a password. No email or approval needed. Your page is live straight away.</p>" },
        Q { q: "I Forgot My Password. What Do I Do?", a: "<p>Message an admin and they'll make you a new one. Once you're logged in, you can change it in Account Settings with Change Password.</p>" },
        Q { q: "Where Is My Page?", a: "<p>Tap your name at the top of the Menu. To change your picture, cover photo, name or password, tap the gear on your page to open Account Settings.</p>" },
    ] },
    Section { title: "Your Personal Collection", qs: &[
        Q { q: "What Is My Personal Collection?", a: "<p>The binders on your page that show the cards you own. They're for showing off, not for sale, so they have no prices or conditions.</p>" },
        Q { q: "How Do I Make A Binder?", a: "<p>On your page, tap Add Binder, or go to Account Settings › Manage Binders. Give it a name, and add a description if you like. You can have up to 50 binders.</p>" },
        Q { q: "How Do I Add Cards To My Binder?", a: "<p>In Manage Binders, tap Add Cards on a binder. Find your card with Search or By Set, or tap Not In The List to use your own photo. You can also add a card straight from the Scanner.</p>" },
        Q { q: "Who Can See My Binders?", a: "<p>Everyone can see your binders that have cards in them. Empty binders only show to you.</p>" },
    ] },
    Section { title: "Scanner", qs: &[
        Q { q: "What Is The Scanner?", a: "<p>Take a photo of a card and the Scanner tells you what it is: its set, number, versions and what it sells for. Tap the card to add it to one of your binders. You'll find it in the Menu.</p>" },
        Q { q: "How Many Scans Do I Get?", a: "<p>3 a day without a store and 8 a day with a store. Your scans start again at midnight. You need to be logged in to scan.</p>" },
        Q { q: "How Do I Get More Scans?", a: "<p>Send an admin a message. The vault team can raise your daily limit. When you run out, the Scanner shows a Message An Admin button.</p>" },
        Q { q: "The Scan Got My Card Wrong. What Now?", a: "<p>Check the name and number on your card and pick the right one from Other Versions. For the best read, hold the card flat, close and in good light, so the small number at the bottom is sharp. Scan one card at a time.</p>" },
    ] },
    Section { title: "Ask The Vault", qs: &[
        Q { q: "What Is Ask The Vault?", a: "<p>An AI helper. Ask it about card prices, what's in stock in the stores, or ideas for what to collect. It's free.</p>" },
        Q { q: "Is There A Limit On Ask The Vault?", a: "<p>You get 30 AI answers every 15 minutes. After that it keeps answering with simpler built-in answers until your time resets. The whole site also has a daily total of 900 AI answers; if that runs out, everyone gets the simpler answers until midnight.</p>" },
    ] },
    Section { title: "Buying", qs: &[
        Q { q: "How Do I Buy A Card?", a: "<p>There are two ways:</p><ul><li><b>On the site:</b> tap + on a card to put it in your cart, then open the cart and tap Send Order. The store has 3 days to accept it (the Big Boys Vault store has 7).</li><li><b>Outside the site:</b> take a screenshot of the cards you want and send it to the seller, by message here or wherever you talk to them.</li></ul>" },
        Q { q: "How Do I Pay?", a: "<p>Payment is between you and the seller. It can happen on the site or outside it, by GCash, Maya, bank or however you both agree. The store's payment details are on its store page. If you ordered on the site, tap I've Paid on your order once you've paid.</p>" },
        Q { q: "How Do Meetups And Shipping Work?", a: "<p>You and the seller agree on them directly, in your chat here or outside the site.</p>" },
        Q { q: "Is Big Boys Vault Responsible For My Purchase?", a: "<p>No. Big Boys Vault has no liability for any transaction, whether it takes place on the website or outside it. Check a store's vouches before you buy, and only pay people you trust.</p>" },
        Q { q: "Can I Make An Offer?", a: "<p>Yes, on cards from stores: tap Make An Offer on the card. The store can accept, decline or send you a counter offer. You can have one open offer per card. Cards in the Big Boys Vault store don't take offers.</p>" },
        Q { q: "Can I Cancel An Order?", a: "<p>Yes, while the store hasn't finished it yet. Open My Orders and tap Cancel.</p>" },
        Q { q: "How Do I Leave A Vouch?", a: "<p>Open the store's page and go to Vouches. Say how it went, and add photos if you like. Vouches help other trainers know who to trust.</p>" },
        Q { q: "How Do I Save A Store Or A Card For Later?", a: "<p>Tap Bookmark. Everything you bookmark is in the Menu under Bookmarks. To hear when a store adds new cards, tap Follow on the store.</p>" },
    ] },
    Section { title: "Posts And Messages", qs: &[
        Q { q: "How Do I Post?", a: "<p>Write in the box on the Home page or on your own page. You can add photos, use bold and three text sizes, and add a caption to each photo. Your post shows on your page and in everyone's feed.</p>" },
        Q { q: "How Do I Message Someone?", a: "<p>Tap Message on their page, their store or a card. Chats can have photos and voice notes, and you can call from a chat.</p>" },
    ] },
    Section { title: "Notifications", qs: &[
        Q { q: "How Do I Get Alerts On My Phone?", a: "<p>Open Notifications and turn alerts on, then allow them when your phone asks. On an iPhone, first tap Share, then “Add to Home Screen”, open Big Boys Vault from your Home Screen, and turn alerts on there.</p>" },
        Q { q: "How Do I Choose What I'm Told About?", a: "<p>In Notifications, each kind of alert has its own switch.</p>" },
    ] },
];

pub static STORES: &[Section] = &[
    Section { title: "Opening A Store", qs: &[
        Q { q: "How Do I Open A Store?", a: "<p>On your page, tap Create A Store. Tell us a bit about what you sell, add photos if you like, and send. An admin answers in your messages. Once you're approved, the button becomes Store Manager.</p>" },
        Q { q: "Is My Store Separate From My Page?", a: "<p>Yes. Your store has its own name, picture, cover, links and posts. Your page and personal binders stay yours. A post written on your store shows on the store, and a post written on your page shows on your page.</p>" },
    ] },
    Section { title: "Adding Cards", qs: &[
        Q { q: "How Do I Add Cards To My Store?", a: "<p>Go to Store Manager › Add Cards. Use Search, Scan A Card, By Set, or Not In The List for your own photo. For each card, pick its condition and price, and choose the online picture or your own photo.</p>" },
        Q { q: "How Many Cards Can My Store Hold?", a: "<p>Up to 500.</p>" },
        Q { q: "How Many Scans Do I Get As A Store?", a: "<p>8 a day, shared between the Scanner and Add Cards. They start again at midnight. Need more? Message an admin.</p>" },
        Q { q: "How Do Binders Work In My Store?", a: "<p>Binders group your cards, by set or by Pokémon for example. Go to Store Manager › Binders, or tap Edit Binders on your store. You can sort them, add a cover and description, and choose who sees them.</p>" },
    ] },
    Section { title: "Orders", qs: &[
        Q { q: "Someone Wants To Buy. What Do I Do?", a: "<ul><li><b>An order on the site:</b> open Store Manager › Orders and Accept or Decline within 3 days. When the money arrives, tap Mark Paid.</li><li><b>A message or a screenshot:</b> sort out the price, payment and shipping or meetup with the buyer directly, here or outside the site.</li></ul>" },
        Q { q: "I Sold A Card Outside The Site. What Do I Do?", a: "<p>Update your store straight away so buyers only see what you still have. In Store Manager › Listings, lower its stock or tap In Stock to switch it to Out Of Stock. You can also tap Edit on the card in your store and choose Out Of Stock or Hidden.</p>" },
        Q { q: "Is Big Boys Vault Responsible For My Sales?", a: "<p>No. Big Boys Vault has no liability for any transaction, whether it takes place on the website or outside it.</p>" },
        Q { q: "How Do I Answer An Offer?", a: "<p>In Orders, Accept, Decline, or Counter with your price. An accepted offer becomes an order at that price.</p>" },
    ] },
    Section { title: "Your Store", qs: &[
        Q { q: "I'm Busy Or Away. Can I Pause My Store?", a: "<p>Yes, in Store Settings. Away keeps your store open but tells buyers replies may be slow. Vacation stops new orders and offers until you're back. You can also hide your store completely.</p>" },
        Q { q: "How Do Views Work?", a: "<p>A view counts when someone else opens your card, once a day per person. Your own looks don't count, and neither do search engines or link previews.</p>" },
    ] },
];

/// An answer as plain words: tags gone, each list item on its own line after "- ".
pub fn plain(a: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    let mut name = String::new();
    for ch in a.chars() {
        match ch {
            '<' => {
                tag = true;
                name.clear();
            }
            '>' => {
                tag = false;
                let n = name.trim_start_matches('/').split_whitespace().next().unwrap_or("").to_ascii_lowercase();
                if n == "li" && !name.starts_with('/') {
                    out.push_str("\n- ");
                } else if n == "p" && name.starts_with('/') {
                    out.push(' ');
                }
            }
            c if tag => name.push(c),
            c => out.push(c),
        }
    }
    out.split('\n').map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

/// Every question and answer as plain text, for Ask The Vault's brief: "Everyone" first, then "Stores".
pub fn as_text() -> String {
    let mut out = String::new();
    for (tab, secs) in [("Everyone", EVERYONE), ("Stores (for trainers with a store)", STORES)] {
        out.push_str(&format!("[{tab}]\n"));
        for s in secs {
            for q in s.qs {
                out.push_str(&format!("Q: {}\nA: {}\n", q.q, plain(q.a)));
            }
        }
    }
    out
}

/// Words that say nothing about which question it is.
const SKIP: &[&str] = &[
    "how", "do", "does", "did", "i", "my", "me", "a", "an", "the", "is", "are", "can", "could", "what", "whats", "where", "who", "when", "why", "which",
    "to", "of", "for", "in", "on", "it", "its", "and", "or", "with", "get", "many", "much", "im", "there", "this", "that", "be", "will", "any", "some",
    "you", "your", "we", "our", "as", "at", "by", "from", "up", "out", "now", "so", "if", "not", "no", "yes", "ok", "please", "pa", "po", "ba", "ang",
    "ng", "sa", "ko", "ako", "paano", "saan", "ano", "mag", "na", "lang", "should", "would", "about", "here", "site", "big", "boys", "vault", "one",
    "way", "ways", "work", "works", "use",
];

fn key(w: &str) -> String {
    let w: String = w.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    if w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") { w[..w.len() - 1].to_string() } else { w }
}

fn keys(text: &str) -> Vec<String> {
    let mut v: Vec<String> = text.split(|c: char| !c.is_alphanumeric() && c != '\'').map(key).filter(|w| !w.is_empty() && !SKIP.contains(&w.as_str())).collect();
    v.sort();
    v.dedup();
    v
}

/// Everyday words people use for a question, beside the question's own (Tagalog and English).
const ALSO: &[(&str, &[&str])] = &[
    ("How Do I Make An Account?", &["register", "signup", "sign", "join", "account", "magregister", "mag-register"]),
    ("I Forgot My Password. What Do I Do?", &["reset", "forgot", "login", "log", "password"]),
    ("How Do I Open A Store?", &["sell", "seller", "selling", "magbenta", "benta", "apply"]),
    ("How Do I Save A Store Or A Card For Later?", &["follow", "bookmark", "save"]),
    ("How Do I Get Alerts On My Phone?", &["iphone", "android", "notification", "push", "alert"]),
    ("How Do I Message Someone?", &["chat", "dm", "pm", "contact", "seller", "message"]),
    ("How Do Meetups And Shipping Work?", &["ship", "shipping", "delivery", "deliver", "meet", "meetup", "courier", "lalamove"]),
    ("How Do I Pay?", &["payment", "gcash", "maya", "bank", "bayad", "magbayad"]),
    ("How Do I Buy A Card?", &["buy", "order", "cart", "bili", "bumili"]),
    ("How Many Scans Do I Get?", &["scan", "limit"]),
    ("How Do I Get More Scans?", &["more", "increase", "upgrade", "raise"]),
    ("How Do I Leave A Vouch?", &["vouch", "review", "feedback"]),
    ("Is Big Boys Vault Responsible For My Purchase?", &["responsible", "liability", "scam", "scammed", "refund"]),
    ("What Is Ask The Vault?", &["ai", "assistant"]),
    ("I'm Busy Or Away. Can I Pause My Store?", &["pause", "vacation", "away", "close", "hide"]),
];

/// A question's words for matching: its own and its everyday ones.
fn words_of(q: &Q) -> Vec<String> {
    let mut k = keys(q.q);
    if let Some((_, also)) = ALSO.iter().find(|(t, _)| *t == q.q) {
        k.extend(also.iter().map(|w| key(w)));
    }
    k.sort();
    k.dedup();
    k
}

/// The FAQs question someone is asking, if it is clearly one: (the question, "all" or "stores"). It must read like a
/// question, and the words it shares with one must weigh at least 2 (a word only one question has weighs 2, a word
/// several share weighs 1), or all of a one-word question's. The heaviest wins; on a tie, the one with fewest words
/// left over.
pub fn find(question: &str) -> Option<(&'static Q, &'static str)> {
    let low = question.to_lowercase();
    let asks = low.contains('?')
        || ["how", "what", "where", "who", "can", "is", "are", "do", "does", "why", "when", "paano", "ano", "saan", "pwede", "puwede", "pano"]
            .iter()
            .any(|w| low.split(|c: char| !c.is_alphanumeric()).any(|x| x == *w));
    if !asks {
        return None;
    }
    let said = keys(&low);
    if said.is_empty() {
        return None;
    }
    let all: Vec<(&'static Q, &'static str, Vec<String>)> = [("all", EVERYONE), ("stores", STORES)]
        .into_iter()
        .flat_map(|(tab, secs)| secs.iter().flat_map(move |s| s.qs.iter().map(move |q| (q, tab))))
        .map(|(q, tab)| (q, tab, words_of(q)))
        .collect();
    let used = |w: &String| all.iter().filter(|(_, _, k)| k.contains(w)).count();
    let mut best: Option<(usize, usize, &'static Q, &'static str)> = None;
    for (q, tab, k) in &all {
        let hits: Vec<&String> = k.iter().filter(|w| said.contains(w)).collect();
        let weight: usize = hits.iter().map(|w| if used(w) == 1 { 2 } else { 1 }).sum();
        let own = keys(q.q).len();
        if weight == 0 || (weight < 2 && !(own == 1 && hits.len() == 1)) {
            continue;
        }
        let left = own.saturating_sub(hits.len());
        if best.map_or(true, |(w, l, _, _)| weight > w || (weight == w && left < l)) {
            best = Some((weight, left, q, tab));
        }
    }
    best.map(|(_, _, q, tab)| (q, tab))
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn a_question_from_the_faqs_finds_its_answer() {
        assert_eq!(find("How many scans do I get?").map(|(q, _)| q.q), Some("How Many Scans Do I Get?"));
        assert_eq!(find("how do i open a store").map(|(q, t)| (q.q, t)), Some(("How Do I Open A Store?", "stores")));
        assert_eq!(find("I forgot my password, what do I do").map(|(q, _)| q.q), Some("I Forgot My Password. What Do I Do?"));
        assert_eq!(find("how to pay?").map(|(q, _)| q.q), Some("How Do I Pay?"));
        assert_eq!(find("paano mag register").map(|(q, _)| q.q), Some("How Do I Make An Account?"));
        assert_eq!(find("how do i sell cards").map(|(q, _)| q.q), Some("How Do I Open A Store?"));
        assert_eq!(find("how does shipping work").map(|(q, _)| q.q), Some("How Do Meetups And Shipping Work?"));
        assert_eq!(find("how do i turn on alerts on my iphone").map(|(q, _)| q.q), Some("How Do I Get Alerts On My Phone?"));
        assert_eq!(find("how do I message a seller").map(|(q, _)| q.q), Some("How Do I Message Someone?"));
        assert!(find("how much is charizard").is_none());
        assert!(find("do you have pikachu?").is_none());
        assert!(find("what binders do you have").is_none());
        assert!(find("Show me Near Mint Pikachu").is_none(), "a card search is not a question from the FAQs");
        assert!(find("What's the most expensive card?").is_none());
        assert!(find("Cheapest holo cards").is_none());
    }

    #[test]
    fn answers_read_as_plain_words() {
        assert_eq!(plain("<p>Up to 500.</p>"), "Up to 500.");
        assert_eq!(plain("<p>Two ways:</p><ul><li><b>On the site:</b> tap +.</li><li>Outside.</li></ul>"), "Two ways:\n- On the site: tap +.\n- Outside.");
        assert!(as_text().contains("Q: How Do I Open A Store?"));
    }
}
