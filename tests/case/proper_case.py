"""Proper case for the site's labels (the owner, 19 Nov 2026: "fix all capitalizations on the entire site. they
should be consistent. use proper case").

The rule: words that are not a sentence (a button, a tab, a heading, a field's name, a count's name, a chip, a
tooltip) have Every Word Capitalised. A sentence (it ends in . ! ? or …, or runs long) is written as a sentence: its
first letter is a capital and the rest is left alone.

This reads the templates as tags, template blocks and text, works out for every piece of text which element it
belongs to, decides label or sentence for that element's words as a whole, and rewrites only the letters that need
it. Run without --write to see what it would change.
"""
import os
import re
import sys

TOKEN = re.compile(r"(<![A-Za-z][^<>]*>|\{#.*?#\}|\{%.*?%\}|\{\{.*?\}\}|<!--.*?-->|<script\b.*?</script>|<style\b.*?</style>|</?[A-Za-z][^<>]*?(?:\{\{.*?\}\}[^<>]*?|\{%.*?%\}[^<>]*?)*>)", re.S)
VOID = {"br", "img", "input", "hr", "meta", "link", "source", "path", "circle", "rect", "line", "polyline", "polygon", "ellipse", "use", "stop", "col", "wbr"}
# Inside these the words belong to whatever holds them (a bold word in a sentence is part of the sentence).
THROUGH = {"b", "strong", "em", "i", "u", "code", "mark", "sup", "sub"}
# Blocks whose text is not shown as words (a layout name, things for <head>).
SILENT_BLOCKS = {"layout", "head"}
# Attributes whose value is read by people.
SEEN_ATTRS = ("placeholder", "title", "aria-label", "alt", "data-ph", "data-label", "data-title", "data-empty", "data-confirm", "data-yes", "data-no", "data-ok", "data-hint")
# Words that stay as they are written.
KEEP = {"ex", "px", "vs", "e.g.", "i.e.", "x", "v", "n", "kg", "g", "ml", "cm", "mm", "am", "pm"}
OPEN_PUNCT = "([{\"'“‘«‹+✓✔⛔⏳🔒★☆·•–—-›»"
END = re.compile(r"[.!?…][\"”’')\]]*\s*$")
MANY = re.compile(r"[.!?] +[A-Z“\"]")
EXPR = ""  # a template value in the middle of words
BOX = ""   # another element's words in the middle of these


def is_sentence(run, force=None):
    """Words as a whole: a sentence, or a label?"""
    words = run.replace(EXPR, " ").replace(BOX, " ")
    text = re.sub(r"\s+", " ", words).strip()
    if force is not None:
        return force
    n = len([w for w in text.split(" ") if re.search(r"[A-Za-z]", w)])
    if END.search(text) or MANY.search(text):
        return True
    if n >= 7 or ("," in text and n >= 5):
        return True
    return False


def cap_word(w):
    """One word with its first letter made a capital, when it is a plain lowercase word."""
    if not w or w.lower() in KEEP:
        return w
    if w.startswith("&") and ";" in w:  # an HTML entity
        return w
    if "://" in w or "@" in w or re.match(r"^[\w.-]+\.(com|ph|net|org|io|dev|app)\b", w) or w.startswith("/"):
        return w
    i = 0
    while i < len(w) and w[i] in OPEN_PUNCT:
        i += 1
    if i < len(w) and w[i].isalpha() and w[i].islower():
        # A word like "iPhone" or "eBay" is spelled that way on purpose.
        if i + 1 < len(w) and w[i + 1].isupper():
            return w
        return w[:i] + w[i].upper() + w[i + 1:]
    return w


def cap_all(piece, glued):
    """Every word of a piece capitalised. `glued`: its first word carries on from what came before (a plural "s")."""
    parts = re.split(r"(\s+)", piece)
    out, first = [], True
    for p in parts:
        if not p or p.isspace():
            out.append(p)
            continue
        out.append(p if (first and glued) else cap_word(p))
        first = False
    return "".join(out)


def cap_first(piece):
    """Only the first letter of the first word (a sentence's start)."""
    m = re.match(r"(\s*)(\S+)(.*)$", piece, re.S)
    if not m:
        return piece
    lead, word, rest = m.groups()
    if word.lower() in KEEP:
        return piece
    return lead + cap_word(word) + rest


class Run:
    def __init__(self, key, where):
        self.key, self.where = key, where
        self.items = []  # ("text", index into tokens, glued) | ("expr",) | ("box",)

    def text(self, toks):
        out = []
        for it in self.items:
            if it[0] == "ctl":
                continue
            out.append(toks[it[1]] if it[0] == "text" else (EXPR if it[0] == "expr" else BOX))
        return "".join(out)

    def lines(self):
        """The run as the lines a person can meet: when every word sits inside {% if %} branches, each branch is
        a line of its own; words around an if (a plural "s") make it all one line."""
        depth, outside = 0, False
        for it in self.items:
            if it[0] == "ctl":
                depth += 1 if it[1] == "open" else (-1 if it[1] == "close" else 0)
            elif depth <= 0 and (it[0] != "text" or True):
                outside = True
        if outside or not any(it[0] == "ctl" for it in self.items):
            one = Run(self.key, self.where)
            one.items = [it for it in self.items if it[0] != "ctl"]
            return [one]
        out, cur, depth = [], None, 0
        for it in self.items:
            if it[0] == "ctl":
                if it[1] == "open":
                    depth += 1
                    if depth == 1:
                        cur = Run(self.key, self.where); out.append(cur)
                elif it[1] == "close":
                    depth -= 1
                elif depth == 1:
                    cur = Run(self.key, self.where); out.append(cur)
                continue
            if cur is not None:
                cur.items.append(it)
        return [r for r in out if r.items]


def fix_template(src, name, overrides):
    toks, kinds, pos = [], [], 0
    for m in TOKEN.finditer(src):
        if m.start() > pos:
            toks.append(src[pos:m.start()]); kinds.append("text")
        tok = m.group(0)
        if tok.startswith("{{"):
            kinds.append("expr")
        elif tok.startswith("{%"):
            kinds.append("ctl")
        elif tok.startswith("<") and not tok.startswith("<!") and not tok.lower().startswith(("<script", "<style")):
            kinds.append("tag")
        else:
            kinds.append("skip")
        toks.append(tok)
        pos = m.end()
    if pos < len(src):
        toks.append(src[pos:]); kinds.append("text")

    stack = []        # (name, class, id) of open elements; id is unique per element
    blocks = []       # names of open {% block %}
    runs, order = {}, []
    uid = 0
    line = 1
    # Whether the letters just before this point run straight on (no space): for a plural "s" after {% if %}.
    glue, ifs = False, []

    def container():
        for i in range(len(stack) - 1, -1, -1):
            if stack[i][0] not in THROUGH:
                return stack[i]
        return ("", "", f"top:{blocks[-1] if blocks else ''}")

    for i, (tok, kind) in enumerate(zip(toks, kinds)):
        if kind == "tag":
            m = re.match(r"<(/?)([A-Za-z0-9]+)", tok)
            nm = m.group(2).lower()
            if m.group(1):
                for j in range(len(stack) - 1, -1, -1):
                    if stack[j][0] == nm:
                        del stack[j:]
                        break
                if nm not in THROUGH:
                    glue = False
            else:
                # The element that holds this one sees a box where it stands.
                if nm not in THROUGH and nm not in ("br",):
                    c = container()
                    if c[2] not in runs:
                        runs[c[2]] = Run(c[2], (line, [f"{n}{'.' + k.split()[0] if k.strip() else ''}" for n, k, _ in stack[-3:]]))
                    runs[c[2]].items.append(("box",))
                    glue = False
                if nm == "br":
                    glue = False
                if nm not in VOID and not tok.rstrip().endswith("/>"):
                    cls = re.search(r'class="([^"]*)"', tok)
                    uid += 1
                    stack.append((nm, cls.group(1) if cls else "", f"e{uid}"))
        elif kind == "ctl":
            body = tok[2:-2].strip().lstrip("-").rstrip("-").strip()
            word = body.split(" ")[0] if body else ""
            if word == "block":
                blocks.append(body.split(" ")[1] if " " in body else "")
            elif word == "endblock" and blocks:
                blocks.pop()
            elif word in ("if", "match", "for"):
                ifs.append(glue)
            elif word in ("else", "when", "elif"):
                glue = ifs[-1] if ifs else False
            elif word in ("endif", "endmatch", "endfor"):
                if ifs:
                    ifs.pop()
            if word in ("if", "match", "else", "when", "elif", "endif", "endmatch") and not (blocks and blocks[-1] in SILENT_BLOCKS):
                c = container()
                if c[2] not in runs:
                    runs[c[2]] = Run(c[2], (line, [f"{n}{'.' + k.split()[0] if k.strip() else ''}" for n, k, _ in stack[-3:]]))
                runs[c[2]].items.append(("ctl", "open" if word in ("if", "match") else ("close" if word in ("endif", "endmatch") else "next")))
        elif kind == "expr":
            if not (blocks and blocks[-1] in SILENT_BLOCKS) and not any(s[0] in ("svg", "script", "style", "textarea", "pre") for s in stack):
                c = container()
                runs.setdefault(c[2], Run(c[2], (line, [f"{n}{'.' + k.split()[0] if k.strip() else ''}" for n, k, _ in stack[-3:]])))
                if c[2] not in order:
                    order.append(c[2])
                runs[c[2]].items.append(("expr",))
            glue = True
        elif kind == "text":
            if tok.strip() and not (blocks and blocks[-1] in SILENT_BLOCKS) and not any(s[0] in ("svg", "script", "style", "textarea", "pre") for s in stack):
                c = container()
                if c[2] not in runs:
                    runs[c[2]] = Run(c[2], (line, [f"{n}{'.' + k.split()[0] if k.strip() else ''}" for n, k, _ in stack[-3:]]))
                    order.append(c[2])
                elif c[2] not in order:
                    order.append(c[2])
                runs[c[2]].items.append(("text", i, glue and not tok[0].isspace()))
                if c[2] not in order:
                    order.append(c[2])
            if tok:
                glue = tok[-1].isalnum() or tok[-1] in "'’"
        line += tok.count("\n")

    changes, review = [], []
    todo = []
    for key in order:
        todo += runs[key].lines()
    for run in todo:
        if not any(it[0] == "text" and re.search(r"[A-Za-z]", toks[it[1]]) for it in run.items):
            continue
        whole = run.text(toks)
        shown = re.sub(r"\s+", " ", whole.replace(EXPR, "{}").replace(BOX, "▢")).strip()
        force = overrides.get((name, shown))
        if force == "K":
            review.append((name, run.where[0], "K", 0, " > ".join(run.where[1]), shown))
            continue
        sentence = is_sentence(whole, force)
        first = True
        for it in run.items:
            if it[0] != "text":
                # A value or a box at the start: what follows is not the first word of the line.
                first = False
                continue
            _, idx, glued = it
            old = toks[idx]
            if not re.search(r"[A-Za-z]", old):
                # Marks only (a bracket, a dot): nothing to rewrite, and a bracket does not use up the first word.
                if old.strip() and not all(ch in OPEN_PUNCT or ch.isspace() for ch in old):
                    first = False
                continue
            new = cap_all(old, glued) if not sentence else (cap_first(old) if first and not glued else old)
            first = False
            if new != old:
                toks[idx] = new
                changes.append((name, run.where[0], " > ".join(run.where[1]), re.sub(r"\s+", " ", old).strip(), re.sub(r"\s+", " ", new).strip(), "S" if sentence else "L"))
        n = len([w for w in shown.replace("{}", " ").replace("▢", " ").split(" ") if re.search(r"[A-Za-z]", w)])
        review.append((name, run.where[0], "S" if sentence else "L", n, " > ".join(run.where[1]), shown))
    return "".join(toks), changes, review


ATTR = re.compile(r'\b(' + "|".join(SEEN_ATTRS) + r')="([^"]*)"')


def fix_attrs(src, name, overrides):
    """The same for words kept in attributes (a hint in an empty box, a tooltip, a name for screen readers)."""
    changes = []

    def one(m):
        attr, val = m.group(1), m.group(2)
        if not re.search(r"[A-Za-z]", val) or val.startswith(("http", "/", "#", ".")):
            return m.group(0)
        # Template values inside stay as they are; the words around them are judged as one line.
        parts = re.split(r"(\{\{.*?\}\}|\{%.*?%\})", val)
        whole = "".join(EXPR if p.startswith(("{{", "{%")) else p for p in parts)
        shown = re.sub(r"\s+", " ", whole.replace(EXPR, "{}")).strip()
        if overrides.get((name, shown)) == "K":
            return m.group(0)
        sentence = is_sentence(whole, overrides.get((name, shown)))
        out, first, glue = [], True, False
        for p in parts:
            if p.startswith(("{{", "{%")):
                out.append(p)
                glue = True
                first = False
                continue
            if not p:
                continue
            glued = glue and not p[0].isspace()
            out.append(cap_all(p, glued) if not sentence else (cap_first(p) if first and not glued else p))
            glue = p[-1].isalnum() or p[-1] in "'’"
            first = False
        new = "".join(out)
        if new != val:
            changes.append((name, 0, f"@{attr}", val, new, "S" if sentence else "L"))
        return f'{attr}="{new}"'

    return ATTR.sub(one, src), changes


def load_overrides(path):
    """Lines of  file<TAB>S|L<TAB>words  : this line is a sentence (S) or a label (L), whatever the rule says."""
    out = {}
    if path and os.path.exists(path):
        for ln in open(path, encoding="utf-8"):
            ln = ln.rstrip("\n")
            if not ln or ln.startswith("#"):
                continue
            f, how, words = ln.split("\t", 2)
            out[(f, words)] = "K" if how == "K" else how == "S"
    return out


if __name__ == "__main__":
    root = sys.argv[1]
    write = "--write" in sys.argv
    ov = load_overrides(next((a.split("=", 1)[1] for a in sys.argv if a.startswith("--overrides=")), ""))
    report = next((a.split("=", 1)[1] for a in sys.argv if a.startswith("--report=")), "")
    all_changes, all_review = [], []
    for f in sorted(os.listdir(root)):
        if not f.endswith(".html"):
            continue
        p = os.path.join(root, f)
        src = open(p, encoding="utf-8", newline="").read()
        new, ch, rv = fix_template(src, f, ov)
        new, ch2 = fix_attrs(new, f, ov)
        all_changes += ch + ch2
        all_review += rv
        if write and new != src:
            open(p, "w", encoding="utf-8", newline="").write(new)
    if report:
        with open(report + ".changes.tsv", "w", encoding="utf-8") as o:
            for c in all_changes:
                o.write("\t".join(str(x) for x in c) + "\n")
        with open(report + ".runs.tsv", "w", encoding="utf-8") as o:
            for r in all_review:
                o.write("\t".join(str(x) for x in r) + "\n")
    print(f"{len(all_changes)} changes in {len(set(c[0] for c in all_changes))} files ({'written' if write else 'dry run'}); {len(all_review)} lines of words")
