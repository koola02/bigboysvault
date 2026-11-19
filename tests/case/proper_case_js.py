"""Proper case for the labels written in scripts (static/*.js and the <script> blocks of the templates).

Finds every string in the script ('…', "…" and `…` with its ${values}), and rewrites only:
  - strings holding HTML: the words between the tags, by the same rules as the templates (proper_case.py);
  - plain strings of two words or more that start with a capital letter and are not code: a label gets Every Word
    Capitalised, a sentence is left alone.
Strings that are also stored values ("Out of stock") are never touched. Run without --write to see the changes.
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(__file__))
from proper_case import EXPR, cap_all, fix_attrs, fix_template, is_sentence, load_overrides  # noqa: E402

# Words the server stores and compares: shown through a helper, never rewritten where they are written.
DATA = {"Out of stock", "Link only", "Bank transfer", "Cash on meetup", "Near Mint", "Lightly Played", "Moderately Played", "Heavily Played"}
REGEX_BEFORE = set("(,=:[!&|?{};+-*%<>~^")
TAG = r"</?(?:a|b|i|u|p|div|span|button|label|input|select|option|form|small|strong|em|h[1-6]|ul|ol|li|img|svg|header|footer|section|article|nav|table|tr|td|th|dl|dt|dd|textarea|br|hr|details|summary|dialog|figure|video|audio|canvas|picture|main|aside|template|code)\b[^<>]*>?"
# Scripts whose strings are not words on a page: a shader, tool names for browsers' agents, the notifications worker.
SKIP = {"holo.js", "webmcp.js", "sw.js"}


def lex(src):
    """Yields ('str', start, end, quote) for plain strings (content between the quotes) and
    ('tpl', [(qstart, qend), ...]) for a template literal: its text parts in order (values lie between them)."""
    out = []
    i, n = 0, len(src)
    last = ""  # last meaningful character of code, to tell a regex from a division

    def template(i):
        # i is just after the opening backtick. Returns (parts, index after the closing backtick).
        parts, start = [], i
        while i < n:
            ch = src[i]
            if ch == "\\":
                i += 2
                continue
            if ch == "`":
                parts.append((start, i))
                return parts, i + 1
            if ch == "$" and i + 1 < n and src[i + 1] == "{":
                parts.append((start, i))
                i = code(i + 2, True)
                start = i
                continue
            i += 1
        parts.append((start, n))
        return parts, n

    def code(i, until_brace):
        nonlocal last
        depth = 0
        while i < n:
            ch = src[i]
            if ch in " \t\r\n":
                i += 1
                continue
            if ch == "/" and i + 1 < n and src[i + 1] == "/":
                j = src.find("\n", i)
                i = n if j < 0 else j
                continue
            if ch == "/" and i + 1 < n and src[i + 1] == "*":
                j = src.find("*/", i + 2)
                i = n if j < 0 else j + 2
                continue
            if ch in "'\"":
                j = i + 1
                while j < n and src[j] != ch:
                    if src[j] == "\\":
                        j += 1
                    if src[j] == "\n":
                        break
                    j += 1
                out.append(("str", i + 1, j, ch))
                i = j + 1
                last = "s"
                continue
            if ch == "`":
                parts, i = template(i + 1)
                out.append(("tpl", parts))
                last = "s"
                continue
            if ch == "/" and (last == "" or last in REGEX_BEFORE or last == "k"):
                # A regular expression: skip to its end (classes may hold a slash).
                j, in_class = i + 1, False
                while j < n and src[j] != "\n":
                    if src[j] == "\\":
                        j += 2
                        continue
                    if src[j] == "[":
                        in_class = True
                    elif src[j] == "]":
                        in_class = False
                    elif src[j] == "/" and not in_class:
                        break
                    j += 1
                i = j + 1
                last = "s"
                continue
            if until_brace:
                if ch == "{":
                    depth += 1
                elif ch == "}":
                    if depth == 0:
                        return i + 1
                    depth -= 1
            if ch.isalpha() or ch in "_$":
                j = i
                while j < n and (src[j].isalnum() or src[j] in "_$"):
                    j += 1
                word = src[i:j]
                last = "k" if word in ("return", "typeof", "case", "in", "of", "do", "else", "void", "delete", "throw", "new", "yield", "await") else "w"
                i = j
                continue
            last = ch if not ch.isdigit() else "w"
            i += 1
        return n

    code(0, False)
    return out


def looks_like_words(t):
    """Two or more words a person reads, starting with a capital: not a selector, a list of classes, a path or code."""
    t = t.replace(EXPR, "x").strip()
    if len(t) < 4 or " " not in t:
        return False
    if not re.match(r"^[^A-Za-z0-9]{0,3}[A-Z]", t):
        return False
    if re.search(r"[<>={}\[\]|\\^$;]|://|\b(function|return|const|let|var)\b|^\S+:\S", t):
        return False
    return True


def fix_js(src, name, overrides):
    edits, changes = [], []
    for node in lex(src):
        if node[0] == "str":
            _, a, b, _q = node
            parts = [(a, b)]
        else:
            parts = node[1]
        texts = [src[a:b] for a, b in parts]
        whole = EXPR.join(texts)
        if not re.search(r"[A-Za-z]", whole):
            continue
        plain = re.sub(r"\s+", " ", whole.replace(EXPR, "{}")).strip()
        if plain in DATA or overrides.get((name, plain)) == "K":
            continue
        if re.search(TAG, whole):
            # HTML: the same pass as a template, with each ${value} standing as a template value.
            synth = "".join(t + ("{{%d}}" % k if k < len(texts) - 1 else "") for k, t in enumerate(texts))
            new, ch, _ = fix_template(synth, name, overrides)
            new, ch2 = fix_attrs(new, name, overrides)
            if new != synth:
                pieces = re.split(r"\{\{\d+\}\}", new)
                if len(pieces) != len(texts):
                    raise SystemExit(f"{name}: lost count in {synth[:80]!r}")
                for (a, b), old, nw in zip(parts, texts, pieces):
                    if old != nw:
                        edits.append((a, b, nw))
                changes += [(name, src.count("\n", 0, parts[0][0]) + 1, "html", c[3], c[4], c[5]) for c in ch + ch2]
            continue
        if not looks_like_words(whole):
            continue
        force = overrides.get((name, plain))
        if is_sentence(whole, force):
            continue
        glue = False
        for (a, b), old in zip(parts, texts):
            if not old:
                glue = True
                continue
            nw = cap_all(old, glue and not old[0].isspace())
            glue = True
            if nw != old:
                edits.append((a, b, nw))
                changes.append((name, src.count("\n", 0, a) + 1, "text", old, nw, "L"))
    for a, b, nw in sorted(edits, reverse=True):
        src = src[:a] + nw + src[b:]
    return src, changes


SCRIPT = re.compile(r"(<script\b[^>]*>)(.*?)(</script>)", re.S)

if __name__ == "__main__":
    write = "--write" in sys.argv
    ov = load_overrides(next((a.split("=", 1)[1] for a in sys.argv if a.startswith("--overrides=")), ""))
    report = next((a.split("=", 1)[1] for a in sys.argv if a.startswith("--report=")), "")
    all_changes = []
    for f in sorted(os.listdir("static")):
        if not f.endswith(".js") or f.endswith(".min.js") or f.startswith(("htmx", "vendor")) or f in SKIP:
            continue
        p = os.path.join("static", f)
        src = open(p, encoding="utf-8", newline="").read()
        new, ch = fix_js(src, f, ov)
        all_changes += ch
        if write and new != src:
            open(p, "w", encoding="utf-8", newline="").write(new)
    for f in sorted(os.listdir("templates")):
        if not f.endswith(".html"):
            continue
        p = os.path.join("templates", f)
        src = open(p, encoding="utf-8", newline="").read()
        found = []

        def one(m):
            if "src=" in m.group(1) or "application/ld+json" in m.group(1) or "{{" in m.group(2) or "{%" in m.group(2):
                return m.group(0)
            new, ch = fix_js(m.group(2), f + "<script>", ov)
            found.extend(ch)
            return m.group(1) + new + m.group(3)

        new = SCRIPT.sub(one, src)
        all_changes += found
        if write and new != src:
            open(p, "w", encoding="utf-8", newline="").write(new)
    if report:
        with open(report + ".changes.tsv", "w", encoding="utf-8") as o:
            for c in all_changes:
                o.write("\t".join(str(x).replace("\n", "⏎").replace("\t", " ") for x in c) + "\n")
    print(f"{len(all_changes)} changes in {len(set(c[0] for c in all_changes))} scripts ({'written' if write else 'dry run'})")
