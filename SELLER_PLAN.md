# Seller Account Implementation Plan

## Phase 1: Data Model Expansion & DB Migrations (`src/members.rs`)
1. Expand `members` table to add fields: `cover_photo`, `location`, `contact_info`, `contact_public` (bool), `status` (Active, Away, Vacation).
2. Expand `member_binders` to add fields: `description`, `visibility` (Public, Private, Unlisted).
3. Expand `member_cards` to add fields: `status` (Active, Reserved, Sold, Hidden, Expired), `expires_at` (i64), `is_custom` (bool).
4. Create new tables:
   - `store_follows`: `id`, `follower_id`, `seller_id`, `created_at`
   - `reviews`: `id`, `reviewer_id`, `seller_id`, `transaction_id`, `rating`, `text`, `created_at`
   - `inquiries`: `id`, `sender_id`, `seller_id`, `card_id`, `message`, `created_at`
   - `offers`: `id`, `sender_id`, `seller_id`, `card_id`, `amount`, `status` (Pending, Accepted, Declined, Countered), `created_at`
   - `reservations`: `id`, `buyer_id`, `seller_id`, `card_id`, `status`, `expires_at`, `created_at`
   - `transactions`: `id`, `buyer_id`, `seller_id`, `card_id`, `amount`, `status`, `created_at`
   - `listing_views`: `id`, `card_id`, `viewer_ip_hash`, `created_at`
   - `listing_saves`: `id`, `member_id`, `card_id`, `created_at`
   - `notifications`: `id`, `member_id`, `kind`, `message`, `link`, `is_read`, `created_at`
   - `notification_preferences`: `member_id`, `event_type`, `site`, `email`, `push`
   - `push_subscriptions`: `id`, `member_id`, `endpoint`, `p256dh`, `auth`
5. Create helper methods in `members.rs` for querying these new tables (e.g., getting dashboard stats, computing average ratings, fetching notifications).

## Phase 2: Seller Profile & Storefront (`src/community.rs`, `templates/trainer_body.html`, `templates/me.html`)
1. Add routes/logic for saving store settings (cover photo, status, contact info, location).
2. Update `/trainer/{slug}` to act as the public storefront: show cover photo, active status banner, ratings summary, and contact info (if public).
3. Build the backend for `Follow` store and viewing Reviews.

## Phase 3: Binder & Inventory Management (`static/me.js`, `src/community.rs`)
1. Implement single and bulk edit endpoints (`/me/cards/bulk`) for status, price, condition, binder.
2. Build UI in `me.js` for bulk selection and actions.
3. Enhance binder settings: edit visibility and description.
4. Implement a "Scan" stub UI in `/me` that accepts an image, mocks an API delay, and returns a top candidate to confirm.
5. Manual upload: Expand `/me/cards/upload` to handle full custom card fields.

## Phase 4: Seller Dashboard (`templates/me.html`, `src/community.rs`, `static/me.js`)
1. Create a new dashboard view in `me.html` showing stat tiles (Total Listings, Active Listings, Items Sold, Binder Count).
2. Create a `/api/seller/stats` endpoint returning historical views/sales.
3. Use a lightweight canvas/SVG or CSS bar chart in `me.js` for the 7/30 days listing views chart.
4. Display recent activity and pending actions (unanswered inquiries, pending offers).

## Phase 5: Notifications Center
1. Implement the site notification bell in `base.html` / `me.html`.
2. Add endpoints: `GET /api/notifications`, `POST /api/notifications/read`.
3. Add a background hook or async task for dispatching emails (stubbed/pluggable via env vars like `SMTP_URL`).
4. Add push subscription endpoints (`/api/notifications/push/subscribe`) with a clear TODO/feature flag.

## Validation & Testing
1. Run DB migrations against a clean local database.
2. Add Rust unit tests in `src/members.rs` and `src/community.rs` for stats, reviews constraints, and inventory lifecycle.
3. Build and ensure `cargo test --lib` passes continuously.