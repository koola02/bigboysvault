# Task: a suggested price for every card in every condition (admins and sellers)

## Why
A trainer asked for what the Collectr app does: pick a card's condition, and the suggested price changes with it.

Today:
- The admin side suggests the **Near Mint market price whatever the condition**. A Lightly Played or Damaged card gets the same suggestion as a pristine one, e.g. "Pikachu, ₱10" every time.
- Sellers (trainers) already get a condition-adjusted value, but they can't see how it's worked out, and the rule lives in a different place from the admin side's.

Goal: **anyone selling a card (the owner, admins, and sellers/trainers) gets a clear, sensible suggested price for any card, in any condition, from one shared rule**.

## What exists today (read these first)
- **Sellers' rule:**
  - `src/members.rs`, `condition_factor(cond)`: LP-NM 0.9, Lightly Played 0.8, MP-LP 0.7, Moderately Played 0.6, HP-MP 0.5, Heavily Played 0.4, Damaged-HP 0.32, Damaged 0.25, everything else 1.0.
  - The same numbers are copied by hand into the SQL macro `worth_sql!` just below it. They must always agree.
  - `MemberCard::worth()` is the trainer's own figure (`value_own`) if they set one, otherwise the market value × `condition_factor`.
- **Market prices:**
  - `src/market.rs` looks up a card's market prices per finish (Normal, Holofoil, Reverse Holo, 1st Edition…) in USD and EUR, and converts them to pesos.
  - `GET /admin/price?id=<catalogue id>` (src/admin.rs `price`) returns `matches[0].finishes = [{key, label, usd, php}]`, plus `source` and `updated`. These are Near Mint (ungraded) market prices.
- **Condition names used everywhere:**
  - `'Near Mint', 'LP-NM', 'Lightly Played', 'MP-LP', 'Moderately Played', 'HP-MP', 'Heavily Played', 'Damaged-HP', 'Damaged', 'Sealed', 'PSA 10', 'PSA 9', 'CGC 10', 'CGC 9.5', 'BGS 9.5'`.
  - The condition box also allows "Type my own…".
  - These are duplicated in `static/quick.js` and `static/admin-float.js` (`CONDS`).
- **Where a price is chosen today:**
  - **Admin Quick add and Add by set:** `static/quick.js`. In the review sheet (`copyRow`), each copy has a finish, a price and a condition. Choosing a finish fills the market price.
  - **Admin card edit window:** `static/admin-float.js` (`copies()`): each copy has a finish, a condition and a price. Plus the full edit form `templates/admin.html` and `static/admin.js`.
  - **Admin My cards quick edits:** `static/my-cards.js`, where price and condition can be changed right on the grid (`data-price`, `data-cond`). Saved through `POST /admin/quick-edit` in src/admin.rs.
  - **Sellers:**
    - `/me` (templates/me.html, static/me.js, src/community.rs `add_card` and `card_action`).
    - A trainer adds a card from the catalogue (`/me/search`) or their own photo, sets its condition, and can type their own value (`value_own`).

## The rule (one place, used by everyone)
1. Create `src/pricing.rs` (registered in `src/lib.rs`) as the **single source of truth**:
   - `pub fn factor(condition: &str) -> Option<f64>`: the share of the Near Mint price for that condition. `None` means "no automatic suggestion" (see 3).
   - `pub fn suggest(near_mint_php: f64, condition: &str) -> Option<f64>`: the suggested price in pesos, rounded (see 5).
   - `pub fn read_condition(text: &str) -> Condition`: understands every way a condition is written on the site:
     - the names above;
     - older imports like "near-mint / reverse-holofoil / holo-rare";
     - "NM", "LP", "MP", "HP", "DMG", "Mint", "Pristine", any casing.
     - "Mint" and "Pristine" count as Near Mint (Pokémon cards' top ungraded condition).
   - Move `members::condition_factor` to call this module, and generate the SQL in `worth_sql!` from the same table (build the CASE text from the table, or test that the two agree). There must be ONE list of numbers.
2. Default shares of the Near Mint market price (the usual rule of thumb among Pokémon card sellers):

   | Condition | Share |
   |---|---|
   | Near Mint (also Mint, Pristine, NM) | 100% |
   | LP-NM | 90% |
   | Lightly Played (LP) | 80% |
   | MP-LP | 70% |
   | Moderately Played (MP) | 60% |
   | HP-MP | 50% |
   | Heavily Played (HP) | 40% |
   | Damaged-HP | 32% |
   | Damaged (DMG) | 25% |
3. No automatic number for:
   - **Graded** cards (PSA / CGC / BGS / ACE / TAG with a grade). Graded prices don't follow a simple share of the raw price; a PSA 10 can be several times the raw price, and a PSA 6 less than raw.
   - **Sealed** product.
   - **A condition the site can't read.**

   For these, show the Near Mint raw market price as a reference ("Ungraded Near Mint: ₱X; set the graded price yourself") and never fill the price automatically.
4. **The owner can change the shares:**
   - Add a small "Condition prices" panel to the admin (Admin › Dashboard, or the settings the home page editor uses; follow how `blocks::setting` stores site settings).
   - Show the table, an editable % per condition, and a "Back to the usual shares" button.
   - Save as a setting (JSON). `pricing` reads the saved shares, falling back to the defaults.
   - Clear the cache with `app.forget()` when saved.
   - Only the owner and full admins can change it. Moderators and sellers can't.
5. **Rounding:**
   - whole pesos;
   - under ₱100, nearest ₱1;
   - ₱100 to ₱1,000, nearest ₱5;
   - over ₱1,000, nearest ₱10;
   - never below ₱1.
6. The finish matters. The suggestion always starts from the Near Mint market price **of the chosen finish**: Holofoil, Reverse Holo, 1st Edition Holo and so on each have their own market price. Change the finish and the suggestion follows.

## One endpoint for both sides
`GET /api/price-suggest?id=<catalogue id>&finish=<finish key>&cond=<condition>` returns:
```json
{ "near_mint": 10, "finish": "holofoil", "finish_label": "Holofoil", "condition": "Lightly Played",
  "share": 0.8, "suggested": 8, "graded": false, "source": "TCGplayer", "updated": "2026-11-09",
  "finishes": [{ "key": "...", "label": "...", "near_mint": 10, "suggested": 8 }] }
```
- `suggested` is null when graded, sealed, unknown, or there's no market price. `finishes` lists every finish with its suggestion for that condition, so a picker can show them all.
- **Who can call it:** signed-in admins (owner token or admin/moderator trainer) and signed-in trainers.
  - Visitors get 404.
  - Rate-limit it with `app.limits` like the other endpoints (e.g. 600 per 15 minutes per account).
- Reuse the existing market lookup and its cache (`app.prices`). Don't call the price services more often than today.
- Also expose the share table to the browser for instant updates without a round trip:
  - `GET /api/condition-shares` returns `{ "Near Mint": 1.0, "LP-NM": 0.9, ... }`, or embed it as a `data-` attribute or JSON script in the pages that need it.
  - The browser recalculates as soon as the condition box changes.

## What people see (admins and sellers alike)
Under every price box where a card is being priced:
- **One clear line:** "Suggested **₱8** · Lightly Played · 80% of Near Mint ₱10 (Holofoil, TCGplayer, 9 Nov)".
- **A "Use ₱8" button** that fills the price.
- **When the condition or finish changes, the suggestion updates instantly.** Fill the price box automatically ONLY IF:
  - the person hasn't typed their own price for that copy, or
  - the current price is still exactly the previous suggestion.

  Never overwrite a price someone typed. Remember per copy whether the price was typed or suggested, e.g. the existing `c.market` flag in quick.js and admin-float.js, and `price_src`.
- **Graded or sealed:** "Graded card: set your own price. Ungraded Near Mint is ₱10." Never auto-filled.
- **No market price found:** "No market price found for this card. Set your own price."
- **Words a 10-year-old understands**, in the site's dark and gold style. Easy to tap on phones (at least 44px targets). No sideways scrolling on a 360px-wide phone.

### Admin screens to update
1. **Quick add and Add by set review sheet** (`static/quick.js` `copyRow`):
   - Each copy row gets the suggestion line and "Use" button.
   - The finish dropdown labels show the condition-adjusted price for the row's condition, e.g. "Holofoil · ₱8".
   - New copies start with the suggested price for their condition, not the Near Mint price.
2. **Admin card edit window** (`static/admin-float.js` `copies()`): the same for each copy row.
3. **Admin full edit form** (`templates/admin.html` / `static/admin.js`): the suggestion under the price.
4. **Admin My cards grid quick edits** (`static/my-cards.js`, `POST /admin/quick-edit`):
   - When the condition is changed right on the grid and that card's price was market-based (`price_src` = market or suggested), show the new suggestion in a small "Price → ₱8?" chip that applies it on tap. Never change it silently.

### Seller screens to update
5. **`/me`, adding a catalogue card:** after picking the card, finish and condition, show the suggestion line and fill the value with it, unless the trainer typed their own.
6. **`/me`, editing one of their cards:** changing the condition updates the suggested value line. Their own typed value (`value_own`) always wins, and the line says so: "You set ₱12. Suggested for Lightly Played: ₱8".
7. **The trainer's total worth** (`worth_sql!` and `MemberCard::worth`) keeps using the same shares, now from `pricing.rs`, so totals match what the screens suggest.
8. **Own-photo cards** (`own:` ids) have no market price: show "No market price for your own photo. Set your own value."

## Data
- No destructive database changes. If a new value is needed for `price_src`, use a new text value `suggested` in the existing column (it's a text column).
- Products saved from a suggestion store `price_src = 'suggested'` and the date in `price_updated`, so later price refreshes know it was a condition-adjusted market price and can refresh it the same way.
- Settings: one new setting key for the share table, added the way existing settings are stored.

## Tests (must all pass before deploying)
**Rust unit tests** in `src/pricing.rs`:
- **Each condition name** gives the right share, including "near-mint / reverse-holofoil / holo-rare", "NM", "lp", "Mint", "Pristine" and "Damaged-HP".
- **Graded** ("PSA 10", "CGC 9.5", "BGS 9.5", "PSA 9 GEM MT") and **Sealed** give no suggestion.
- **Rounding** at ₱9.6, ₱99, ₱101, ₱998 and ₱1,234.
- **Saved shares** replace the defaults, and a broken saved setting falls back to the defaults.
- **The SQL `worth_sql!` shares equal `pricing.rs`'s shares**: a test that fails if they ever differ.

**Endpoint tests** (or a small script against a local server):
- A visitor gets 404, a trainer and an admin get JSON.
- A graded condition gives `suggested: null`.

**Browser checks** at 390×844 (phone) and 1440×900 (desktop):
- **Admin Quick add:** add a card, set Lightly Played; the suggestion shows 80% of Near Mint and the price fills. Type ₱50, change the condition to Damaged; ₱50 stays and the line shows the new suggestion.
- **Seller /me:** add a catalogue card as Moderately Played; the suggestion is 60% and the value fills.
- **Graded:** choose PSA 10; no auto-fill, and the reference line shows.

## Done means
- Admins and sellers see a condition-based suggested price for every catalogue card, in every condition and finish, with graded and sealed handled honestly.
- One table of shares is used everywhere (screens, totals, SQL), and the owner can edit it.
- Typed prices are never overwritten.
- It works on phones and desktops.
- Live database backed up first, all tests passing, deployed, and the release confirmed with `fly releases -a bigboysvault`.
