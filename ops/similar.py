#!/usr/bin/env python3
"""Nightly "More like this" for the vault, worked out on the VM with a small language model.

Each card in the vault gets a short description built from its full catalogue details on TCGdex (name, Pokémon
type, stage, what it evolves from, rarity, artist, set, attacks), and the model turns each description into a
vector. Cards whose vectors point the same way are alike in what they are, not just in name: two Fire starters
by the same artist, a Trainer and the cards it searches for. A little weight also goes to prices being close.
The 24 best picks per card go to the site (POST /admin/similar), which shows them on card pages and in Ask the
Vault. Needs: the venv from ops/setup-extras.sh (sentence-transformers). Settings come from the environment
(SITE, ADMIN_TOKEN, DATA_DIR), set by ops/similar.sh."""
import json
import math
import os
import sys
import time
import urllib.error
import urllib.request

SITE = os.environ["SITE"].rstrip("/")
TOKEN = os.environ["ADMIN_TOKEN"]
DATA = os.environ.get("DATA_DIR", os.path.expanduser("~/bbv"))
CACHE = os.path.join(DATA, "tcgdex")
MODEL = os.environ.get("SIMILAR_MODEL", "sentence-transformers/all-MiniLM-L6-v2")
PICKS = 24


def get(url, admin=False):
    req = urllib.request.Request(url, headers={"Accept": "application/json", "User-Agent": "bbv-similar"})
    if admin:
        req.add_header("Cookie", f"marcuru_admin={TOKEN}")
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)


def details(api_id):
    """TCGdex's details for one card, kept on disk (cards don't change; a miss is kept too, as {})."""
    os.makedirs(CACHE, exist_ok=True)
    path = os.path.join(CACHE, api_id.replace(":", "_").replace("/", "_") + ".json")
    if os.path.exists(path):
        with open(path, encoding="utf-8") as f:
            d = json.load(f)
        return d if isinstance(d, dict) else {}
    # "tcgdex:base1-4", "tcgdex-ja:SV1a-001", "tcgdex-zh-tw:…", or a bare id like "cel25c-73A".
    src, sep, cid = api_id.partition(":")
    if not sep:
        src, cid = "tcgdex", api_id
    cid = cid.strip()
    if not cid:
        return {}
    lang = "ja" if src.endswith("-ja") else ("zh-tw" if src.endswith(("-zh", "-zh-tw")) else "en")
    try:
        d = get(f"https://api.tcgdex.net/v2/{lang}/cards/{urllib.request.quote(cid)}")
    except urllib.error.HTTPError as e:
        d = {} if e.code == 404 else None
    except Exception:
        d = None
    if d is None:
        return {}  # the service didn't answer: try again tomorrow, nothing kept
    if not isinstance(d, dict):
        d = {}  # not one card's details: kept as a miss
    with open(path, "w", encoding="utf-8") as f:
        json.dump(d, f)
    time.sleep(0.08)
    return d


def describe(card, d):
    """One plain sentence about the card for the model."""
    bits = [d.get("name") or card["title"].split(" - ")[0]]
    cat = d.get("category")
    if cat == "Pokemon":
        types = ", ".join(d.get("types") or [])
        stage = d.get("stage") or ""
        bits.append(f"{stage} {types} Pokémon".strip())
        if d.get("evolveFrom"):
            bits.append(f"evolves from {d['evolveFrom']}")
        attacks = [a.get("name", "") for a in (d.get("attacks") or []) if a.get("name")]
        if attacks:
            bits.append("attacks " + ", ".join(attacks[:3]))
        if d.get("suffix"):
            bits.append(d["suffix"])
    elif cat:
        bits.append(f"{d.get('trainerType') or d.get('energyType') or ''} {cat} card".strip())
    if d.get("rarity"):
        bits.append(f"{d['rarity']} rarity")
    if d.get("illustrator"):
        bits.append(f"illustrated by {d['illustrator']}")
    sset = (d.get("set") or {}).get("name") or card.get("set") or ""
    if sset:
        bits.append(f"from the {sset} set")
    if card.get("finish"):
        bits.append(card["finish"])
    return ". ".join(b for b in bits if b) + "."


def main():
    cards = get(f"{SITE}/admin/similar/cards", admin=True)["cards"]
    if len(cards) < 3:
        print("too few cards")
        return 0
    texts = []
    for c in cards:
        d = details(c["api_id"]) if c.get("api_id") else {}
        texts.append(describe(c, d))
    from sentence_transformers import SentenceTransformer  # loaded only now: it is the slow part
    import numpy as np

    model = SentenceTransformer(MODEL, device="cpu")
    emb = model.encode(texts, batch_size=64, normalize_embeddings=True, show_progress_bar=False)
    sims = emb @ emb.T
    price = np.array([math.log1p(max(c["price"], 0)) for c in cards])
    near = np.exp(-((price[:, None] - price[None, :]) ** 2) / (2 * 0.8 ** 2))
    score = 0.85 * sims + 0.15 * near
    name = [c["title"].split(" - ")[0].strip().lower() for c in cards]
    sset = [(c.get("set") or "").lower() for c in cards]
    out = []
    for i, c in enumerate(cards):
        order = np.argsort(-score[i])
        picks = []
        for j in order:
            if j == i or (name[j] == name[i] and sset[j] == sset[i]) or not cards[j]["available"]:
                continue  # itself, or the same card in another condition (already shown as "Other copies")
            picks.append(int(cards[j]["id"]))
            if len(picks) >= PICKS:
                break
        out.append({"id": int(c["id"]), "picks": picks})
    body = json.dumps({"cards": out}).encode()
    req = urllib.request.Request(f"{SITE}/admin/similar", data=body, method="POST", headers={"Content-Type": "application/json", "Cookie": f"marcuru_admin={TOKEN}"})
    with urllib.request.urlopen(req, timeout=120) as r:
        saved = json.load(r).get("saved", 0)
    known = sum(1 for c in cards if c.get("api_id") and os.path.exists(os.path.join(CACHE, c["api_id"].replace(":", "_").replace("/", "_") + ".json")))
    print(f"{saved} cards have look-alikes; {known} of {len(cards)} described from the catalogue")
    return 0


if __name__ == "__main__":
    sys.exit(main())
