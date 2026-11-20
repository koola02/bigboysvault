"""Big Boys Vault's Pokémon-style icons (the owner, 20 Nov 2026: menus and bars less boring, still easy to tell apart).
Original drawings, not copies of official art. Writes static/img/pk.svg (one <symbol> per icon, 48x48) and a preview."""
import pathlib, sys
R = pathlib.Path(r"C:\Users\jolhi\Personal Claude Code\marcuru-rs")
INK = "#15131c"; LIGHT = "#ece5d6"; RED = "#e3350d"; WHITE = "#f7f4ee"; CREAM = "#f3e7cf"; GOLD = "#f2c14e"; BLUE = "#3a6fd8"
SW = 2.4

def ball(cx, cy, r, top=RED):
    return (f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="{WHITE}"/>'
            f'<path d="M{cx-r} {cy}a{r} {r} 0 0 1 {2*r} 0z" fill="{top}"/>'
            f'<path d="M{cx-r} {cy}h{2*r}" fill="none"/>'
            f'<circle cx="{cx}" cy="{cy}" r="{r*0.38:.2f}" fill="{WHITE}"/>')

def sym(id_, body, title):
    return (f'<symbol id="{id_}" viewBox="0 0 48 48"><title>{title}</title>'
            f'<g stroke="{INK}" stroke-width="{SW}" stroke-linejoin="round" stroke-linecap="round">{body}</g></symbol>')

S = {}
# Home: a little house with a red roof and a ball window.
S["home"] = ("Home",
  f'<path d="M10 22h28v19a1.5 1.5 0 0 1-1.5 1.5h-25A1.5 1.5 0 0 1 10 41z" fill="{CREAM}"/>'
  f'<path d="M4 24L24 6l20 18z" fill="{RED}"/>'
  f'<path d="M27 42.5V32a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v10.5" fill="#9a5b2e"/>'
  + ball(18, 32, 5))
# Stores: a shop with a blue and white awning and a ball sign.
stripes = "".join(f'<path d="M{6+i*7.2:.1f} 14h7.2v7a3.6 3.6 0 0 1-7.2 0z" fill="{BLUE if i % 2 == 0 else WHITE}"/>' for i in range(5))
S["stores"] = ("Stores",
  f'<path d="M8 20h32v21a1.5 1.5 0 0 1-1.5 1.5h-29A1.5 1.5 0 0 1 8 41z" fill="{CREAM}"/>'
  + stripes +
  f'<rect x="20" y="29" width="8" height="13.5" rx="1" fill="#9fd3f0"/>'
  f'<rect x="11.5" y="29" width="6" height="6" rx="1" fill="#9fd3f0"/><rect x="30.5" y="29" width="6" height="6" rx="1" fill="#9fd3f0"/>'
  + ball(24, 8, 5))
# Ask: a red card-reading gadget with a blue lens and a sparkle on its screen.
S["ask"] = ("Ask",
  f'<rect x="9" y="5" width="30" height="38" rx="5" fill="{RED}"/>'
  f'<circle cx="16.5" cy="12.5" r="4.5" fill="#5ab0f2"/><circle cx="15.2" cy="11.2" r="1.3" fill="{WHITE}" stroke="none"/>'
  f'<circle cx="25" cy="11" r="1.6" fill="#ff6b6b"/><circle cx="30" cy="11" r="1.6" fill="{GOLD}"/><circle cx="35" cy="11" r="1.6" fill="#5fd38a"/>'
  f'<rect x="14" y="20" width="20" height="16" rx="2" fill="#bfeee0"/>'
  f'<path d="M24 22.5l1.6 4.2 4.2 1.6-4.2 1.6L24 34.1l-1.6-4.2-4.2-1.6 4.2-1.6z" fill="{GOLD}" stroke-width="1.6"/>')
# Menu: a trainer's bag.
S["bag"] = ("Menu",
  f'<path d="M18 13c0-6 12-6 12 0" fill="none" stroke="{LIGHT}" stroke-width="3"/>'
  f'<rect x="9" y="13" width="30" height="30" rx="8" fill="#f2a33a"/>'
  f'<path d="M9 21a8 8 0 0 1 8-8h14a8 8 0 0 1 8 8v4H9z" fill="#d9792b"/>'
  f'<rect x="16" y="30" width="16" height="9" rx="3" fill="#f7c76a"/>'
  + ball(24, 24, 4.2))
# Trainers: two friends, one with orange hair and a side ponytail, one with spiky dark hair and squinting eyes.
S["trainers"] = ("Trainers",
  # the spiky-haired friend, behind
  f'<path d="M22 44c0-6 4-10 10-10s10 4 10 10z" fill="#6f9a3e"/>'
  f'<circle cx="32" cy="23" r="8.5" fill="#d99a63"/>'
  f'<path d="M23 22l1-5-2-2 4 0 1-4 3 3 2-4 2 4 3-3 1 4 4 0-2 2 1 5c-3-3-6-4-9-4s-6 1-9 4z" fill="#4a2c1a"/>'
  f'<path d="M28 24.5h2.6M33.6 24.5h2.6" fill="none" stroke-width="2"/>'
  # the friend with orange hair, in front
  f'<path d="M5 44c0-6 4-9.5 10-9.5S25 38 25 44z" fill="{GOLD}"/>'
  f'<circle cx="15" cy="26" r="8" fill="#f8d2b0"/>'
  f'<path d="M7 26c0-6 3.5-9 8-9s8 3 8 7c-3 0-5-1-6-3-1 2-5 4-10 5z" fill="#f07a2a"/>'
  f'<path d="M21 18.5c2-3 6-4 7-2-1 2-3 3.5-6 3.5z" fill="#f07a2a"/>'
  f'<circle cx="12.3" cy="27.5" r="1.1" fill="{INK}" stroke="none"/><circle cx="17.7" cy="27.5" r="1.1" fill="{INK}" stroke="none"/>')
# FAQ: a puzzled yellow duck holding its head, with a question mark above.
S["faq"] = ("FAQs",
  f'<path d="M18 16c-1-4 0-7 2-8M22 15c0-4 1-7 3-8M26 16c1-4 3-6 5-6" fill="none" stroke="{LIGHT}" stroke-width="4.4"/>'
  f'<path d="M18 16c-1-4 0-7 2-8M22 15c0-4 1-7 3-8M26 16c1-4 3-6 5-6" fill="none" stroke-width="2"/>'
  f'<circle cx="22" cy="30" r="13.5" fill="#f6d34a"/>'
  f'<path d="M12.5 34c0-3.5 4-5.5 9.5-5.5s9.5 2 9.5 5.5-4 5-9.5 5-9.5-1.5-9.5-5z" fill="#f3e2b3"/>'
  f'<ellipse cx="8.5" cy="21" rx="4" ry="3.2" fill="#f6d34a" transform="rotate(-35 8.5 21)"/>'
  f'<ellipse cx="35.5" cy="21" rx="4" ry="3.2" fill="#f6d34a" transform="rotate(35 35.5 21)"/>'
  f'<circle cx="16.5" cy="24.5" r="2.7" fill="{WHITE}"/><circle cx="27.5" cy="24.5" r="2.7" fill="{WHITE}"/>'
  f'<circle cx="16.5" cy="24.5" r=".9" fill="{INK}" stroke="none"/><circle cx="27.5" cy="24.5" r=".9" fill="{INK}" stroke="none"/>'
  f'<path d="M35.5 7.5a4 4 0 1 1 5.5 3.8c-1.4.6-2 1.4-2 2.9" fill="none" stroke="{WHITE}" stroke-width="5"/>'
  f'<path d="M35.5 7.5a4 4 0 1 1 5.5 3.8c-1.4.6-2 1.4-2 2.9" fill="none" stroke="{BLUE}" stroke-width="2.6"/>'
  f'<circle cx="39" cy="18.6" r="1.7" fill="{BLUE}" stroke="none"/>')
# Vault Chat: a speech bubble with a ball in it.
S["chat"] = ("Vault Chat",
  f'<path d="M8 8h32a3 3 0 0 1 3 3v20a3 3 0 0 1-3 3H20l-9 8v-8H8a3 3 0 0 1-3-3V11a3 3 0 0 1 3-3z" fill="{WHITE}"/>'
  + ball(24, 21, 7.5))
# Vouches: a gold badge with a star and two ribbons.
S["vouches"] = ("Vouches",
  f'<path d="M17 28l-4 16 6-3 4 5 3-14z" fill="{RED}"/><path d="M31 28l4 16-6-3-4 5-3-14z" fill="{BLUE}"/>'
  f'<circle cx="24" cy="20" r="14" fill="{GOLD}"/><circle cx="24" cy="20" r="10" fill="#f7d77e"/>'
  f'<path d="M24 12.5l2.3 4.7 5.2.8-3.8 3.6.9 5.1-4.6-2.4-4.6 2.4.9-5.1-3.8-3.6 5.2-.8z" fill="{WHITE}" stroke-width="1.8"/>')
# Orders: a parcel with a ball sticker.
S["orders"] = ("Orders",
  f'<path d="M6 16l18-8 18 8v22l-18 8-18-8z" fill="#c58b4e"/>'
  f'<path d="M6 16l18 8 18-8M24 24v22" fill="none"/>'
  f'<path d="M15 12l18 8v7" fill="none" stroke="#f3e2b3" stroke-width="3.2"/>'
  + ball(15, 31, 4.5))
# Cart: a shopping cart with a ball inside.
S["cart"] = ("Cart",
  f'<path d="M3 8h6l5 22h24l4-15H11" fill="none" stroke="{LIGHT}" stroke-width="3"/>'
  f'<path d="M11 15h31l-4 15H14z" fill="{WHITE}"/>'
  + ball(26, 20, 6.5) +
  f'<circle cx="16" cy="39" r="3.4" fill="{LIGHT}"/><circle cx="35" cy="39" r="3.4" fill="{LIGHT}"/>')
# Scanner: a card inside scan corners, with a red line across it.
S["scanner"] = ("Scanner",
  f'<rect x="15" y="9" width="18" height="26" rx="2.5" fill="{GOLD}"/><rect x="18" y="12" width="12" height="9" rx="1" fill="#9fd3f0" stroke-width="1.8"/>'
  f'<path d="M18 25h12M18 29h8" fill="none" stroke-width="1.8"/>'
  f'<path d="M5 13V7a2 2 0 0 1 2-2h6M35 5h6a2 2 0 0 1 2 2v6M43 35v6a2 2 0 0 1-2 2h-6M13 43H7a2 2 0 0 1-2-2v-6" fill="none" stroke="{LIGHT}" stroke-width="3"/>'
  f'<path d="M8 22h32" fill="none" stroke="{RED}" stroke-width="3"/>')
# Bookmarks: a red ribbon with a ball on it.
S["bookmarks"] = ("Bookmarks",
  f'<path d="M12 4h24a1.5 1.5 0 0 1 1.5 1.5V44L24 35l-13.5 9V5.5A1.5 1.5 0 0 1 12 4z" fill="{RED}"/>'
  + ball(24, 18, 7, top=WHITE))
# Notifications: a "!" in a bubble, like being spotted.
S["notifications"] = ("Notifications",
  f'<path d="M24 4c11 0 19 7 19 17s-8 17-19 17c-2 0-4-.2-6-.8L9 42l2-8c-4-3-6-7.5-6-13C5 11 13 4 24 4z" fill="{WHITE}"/>'
  f'<path d="M24 11v14" fill="none" stroke="{RED}" stroke-width="5"/><circle cx="24" cy="31.5" r="2.8" fill="{RED}" stroke="none"/>')
# Admin: a purple master-style ball.
S["admin"] = ("Admin",
  f'<circle cx="24" cy="24" r="19" fill="{WHITE}"/>'
  f'<path d="M5 24a19 19 0 0 1 38 0z" fill="#7a3fc0"/>'
  f'<circle cx="13" cy="15" r="3.4" fill="#e86fb4"/><circle cx="35" cy="15" r="3.4" fill="#e86fb4"/>'
  f'<path d="M18 20v-8l6 6 6-6v8" fill="none" stroke="{WHITE}" stroke-width="2.6"/>'
  f'<path d="M5 24h38" fill="none"/><circle cx="24" cy="24" r="6" fill="{WHITE}"/>')
# Log Out: a door with an arrow going out.
S["logout"] = ("Log Out",
  f'<path d="M6 4h24v40H6z" fill="none" stroke="{LIGHT}" stroke-width="2.4"/><path d="M8 6h20v36H8z" fill="#9a5b2e"/><circle cx="23" cy="25" r="1.6" fill="{GOLD}"/>'
  f'<path d="M24 24h18M35 17l7 7-7 7" fill="none" stroke="{RED}" stroke-width="3.6"/>')
# Log In Or Register: a trainer card.
S["login"] = ("Log In Or Register",
  f'<rect x="4" y="10" width="40" height="28" rx="4" fill="{BLUE}"/>'
  f'<circle cx="15" cy="22" r="5" fill="#f8d2b0"/><path d="M8 33c1-4 4-6 7-6s6 2 7 6z" fill="{GOLD}"/>'
  f'<path d="M26 19h12M26 25h12M26 31h8" fill="none" stroke="{WHITE}" stroke-width="2.6"/>')
# Notifications in the header stay as they are; My Store uses the stores shop.

out = ['<svg xmlns="http://www.w3.org/2000/svg" style="display:none">',
       '<!-- Big Boys Vault menu and bar icons (static/img/pk.svg; drawn by tools/pk_icons.py: change them there and run it). Original drawings. -->']
for k, (title, body) in S.items():
    out.append(sym(k, body, title))
out.append("</svg>")
(R / "static/img/pk.svg").write_text("\n".join(out), encoding="utf-8")
print("pk.svg", len(S), "icons")

# A preview page: each icon at the sizes it is used, on the site's dark surfaces.
cells = "".join(
    f'<div class="c"><span class="m"><svg viewBox="0 0 48 48"><use href="pk.svg#{k}"/></svg></span>'
    f'<svg class="b" viewBox="0 0 48 48"><use href="pk.svg#{k}"/></svg><svg class="big" viewBox="0 0 48 48"><use href="pk.svg#{k}"/></svg><b>{t}</b></div>'
    for k, (t, _) in S.items())
pv = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else None
if pv:
    pv.mkdir(parents=True, exist_ok=True)
    (pv / "pk.svg").write_text("\n".join(out), encoding="utf-8")
    (pv / "index.html").write_text(f"""<!doctype html><meta charset="utf-8"><style>
body{{margin:0;padding:20px;background:#0b0c12;color:#eee;font:600 14px system-ui}}
.g{{display:grid;grid-template-columns:repeat(4,1fr);gap:14px}}
.c{{display:flex;align-items:center;gap:14px;padding:12px;border-radius:12px;background:#151824}}
.m{{display:grid;place-items:center;width:36px;height:36px;border-radius:50%;background:#262b3b}}
.m svg{{width:30px;height:30px}} .b{{width:28px;height:28px}} .big{{width:72px;height:72px}}
</style><div class="g">{cells}</div>""", encoding="utf-8")
    print("preview written")
