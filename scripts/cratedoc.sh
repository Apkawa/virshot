#!/usr/bin/env bash
# Read the rendered docs of a crate item as markdown with actionable links.
# Usage: ./scripts/cratedoc.sh <crate>::<item> [--all] [--full]
#   ./scripts/cratedoc.sh xcap                  # crate index page
#   ./scripts/cratedoc.sh xcap::Monitor         # struct/enum/trait/fn/macro/type/mod
#   ./scripts/cratedoc.sh tracing-subscriber::filter::EnvFilter
#   ./scripts/cratedoc.sh xcap::Monitor --all   # keep trait/blanket impl sections
#   ./scripts/cratedoc.sh xcap --full           # keep sidebar/TOC
# Renders `cargo doc -p <crate> --no-deps` (locked versions), strips the rustdoc
# sidebar, converts HTML to markdown with html2markdown (falls back to pandoc),
# then rewrites links into things that are actually usable:
#   item links  -> ./scripts/cratedoc.sh <crate>::<item>
#   Source links -> the real source file in .cache/cargo/registry/src/...:lines
#   in-page anchors (#method.new) -> plain label
set -euo pipefail
# Repo root: this script lives in ./scripts/.
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CARGO_HOME="$here/.cache/cargo"

item_spec="${1:?usage: ./scripts/cratedoc.sh <crate>::<item> [--all]}"
crate="${item_spec%%::*}"
item="${item_spec#*::}"
[[ "$item" == "$crate" ]] && item=""
flags="${2:-}"
keep_impl=false; keep_sidebar=false
for f in "$@"; do
  [[ "$f" == "--all" ]] && keep_impl=true
  [[ "$f" == "--full" ]] && keep_sidebar=true
done

mkdir -p "$here/.tmp"
cargo doc -p "$crate" --no-deps >/dev/null 2>&1 ||
  cargo doc -p "${crate//_/-}" --no-deps >/dev/null 2>&1 ||
  { echo "cargo doc failed for '$crate'" >&2; exit 1; }

# cargo takes the package name (`tracing-subscriber`); rustdoc writes the crate
# name with underscores (`target/doc/tracing_subscriber/`).
doc_root="$here/target/doc/${crate//-/_}"
[[ -d "$doc_root" ]] || doc_root="$here/target/doc/$crate"
# Registry source dir: crates.io writes the crate name as published (hyphens).
src_root="$(ls -d "$here/.cache/cargo/registry/src/"*/"${crate}-"*/ 2>/dev/null | head -1 || true)"
if [[ -z "$src_root" ]]; then
  src_root="$(ls -d "$here/.cache/cargo/registry/src/"*/"${crate//-/_}-"*/ 2>/dev/null | head -1 || true)"
fi

page="$(
  python3 -c '
import os, sys, urllib.parse
crate, item, root = sys.argv[1].replace("-", "_"), sys.argv[2], sys.argv[3]
if item.startswith(crate + "::"):
    item = item[len(crate) + 2:]
if not item or item == crate:
    idx = os.path.join(root, "index.html")
    print(idx if os.path.isfile(idx) else "")
    sys.exit(0)
kinds = {"struct","enum","trait","fn","type","macro","const","union","mod","primitive"}
exact, cands = "", []
for dirpath, _, files in os.walk(root):
    for f in sorted(files):
        if not f.endswith(".html"):
            continue
        path = os.path.join(dirpath, f)
        if "Redirecting to" in open(path, encoding="utf-8", errors="ignore").read(1200):
            continue
        name = urllib.parse.unquote(f[:-5])
        parts = os.path.relpath(path, root).split(os.sep)[:-1]
        base = name.split(".", 1)[1] if (name.split(".", 1)[0] in kinds and "." in name) else name
        if f == "index.html":
            full = "::".join(parts) if parts else crate
        else:
            full = "::".join(parts + [base])
        if full == item or full == crate + "::" + item:
            exact = path
            break
        if base == item.rsplit("::", 1)[-1]:
            cands.append(full)
        elif f == "index.html" and parts and parts[-1] == item.rsplit("::", 1)[-1]:
            cands.append(full)
if not exact:
    sys.stderr.write("item '%s' not found; candidates: %s\n" % (item, ", ".join(sorted(cands)[:12])))
print(exact)
' "$crate" "$item" "$doc_root"
)"

if [[ -z "$page" ]]; then exit 1; fi

python3 -c '
import re, sys
src = open(sys.argv[1], encoding="utf-8").read()
if not sys.argv[2] == "--keep":
    src = re.sub(r"<nav class=\"sidebar[^\"]*\">.*?</nav>", "", src, flags=re.S)
    src = re.sub(r"<div class=\"sidebar-crate\">.*?</div>\s*", "", src, flags=re.S)
    src = re.sub(r"<div class=\"sidebar-elems\">.*?</div>\s*", "", src, flags=re.S)
    src = re.sub(r"<div id=\"rustdoc-modnav\">.*?</div>", "", src, flags=re.S)
    src = re.sub(r"<div class=\"sidebar-resizer[^\"]*\"[^>]*>.*?</div>", "", src, flags=re.S)
    src = re.sub(r"<a[^>]*href=\"#main-content\"[^>]*>.*?</a>", "", src, flags=re.S)
    src = re.sub(r"<div class=\"warning\">.*?</div>", "", src, flags=re.S)
sys.stdout.write(src)
' "$page" "$( [[ "$keep_sidebar" == true ]] && echo --keep || echo --strip )" > "$here/.tmp/cratedoc.html"

md="$here/.tmp/cratedoc.md"
if command -v html2markdown >/dev/null 2>&1; then
  html2markdown --single-line-break --no-wrap-links "$here/.tmp/cratedoc.html" > "$md"
elif command -v pandoc >/dev/null 2>&1; then
  pandoc -r html -w plain --wrap=none "$here/.tmp/cratedoc.html" 2>/dev/null > "$md"
else
  python3 -c '
import html.parser, sys
class P(html.parser.HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.out = []; self.skip = 0
    def handle_starttag(self, tag, attrs):
        if tag in ("script", "style"): self.skip += 1
        if tag in ("div", "p", "h1", "h2", "h3", "h4", "h5", "li", "pre", "br", "section"):
            self.out.append("\n")
        if tag == "a" and not self.skip: self.out.append(" ")
    def handle_endtag(self, tag):
        if tag in ("script", "style"): self.skip -= 1
    def handle_data(self, d):
        if not self.skip and d.strip(): self.out.append(d.strip())
p = P(); p.feed(sys.stdin.read())
print("\n".join(x for x in p.out if x))
' < "$here/.tmp/cratedoc.html" > "$md"
fi

# Auto-generated trait/blanket impl sections are noise for API discovery.
if [[ "$keep_impl" != true ]]; then
  awk -v m='^Trait Implementations|^## Trait Implementations' \
    'm && $0 ~ m { exit } { print }' "$md" > "$md.trim"
else
  cp "$md" "$md.trim"
fi

# Rewrite links so they are actionable for an agent reading the output.
python3 -c '
import os, re, sys, urllib.parse
crate, doc_root, src_root = sys.argv[1].replace("-", "_"), sys.argv[2], sys.argv[3]
page_dir = os.path.dirname(os.path.relpath(sys.argv[4], sys.argv[5]))
kinds = {"struct","enum","trait","fn","type","macro","const","union","mod","primitive","impl","impl-"}

def item_from_href(href):
    href = href.split("#")[0]
    if not href.endswith(".html"):
        return None
    rel = os.path.normpath(os.path.join(page_dir, href))
    if not rel.startswith("target/doc/"):
        return None
    name = urllib.parse.unquote(os.path.basename(rel)[:-5])
    parts = rel.split(os.sep)[2:]
    owner = parts[0]
    body = parts[1:-1]
    if os.path.basename(rel) == "index.html":
        full = "::".join(body) if body else owner
    else:
        head = name.split(".", 1)[0]
        base = name.split(".", 1)[1] if head in kinds and "." in name else name
        full = "::".join(body + [base])
    if not full:
        return None
    if owner != crate:
        full = owner + "::" + full
    elif not full.startswith(crate + "::") and full != crate:
        full = crate + "::" + full
    return "./scripts/cratedoc.sh " + full

def rewrite(m):
    label, href, title = m.group(1), m.group(2), m.group(3)
    href = href.strip()
    if href.startswith(("http://", "https://")):
        return "[%s](%s)" % (label, href)
    if title:
        t = re.match(r"^(?:mod|struct|enum|trait|fn|type|const|macro|union|primitive)\s+(\S+)", title)
        if t:
            item = t.group(1)
            if "::" not in item:
                item = crate + "::" + item
            return "[%s](./scripts/cratedoc.sh %s)" % (label, item)
    if href.startswith("#"):
        return label
    if re.match(r"(\.\./)+src/", href):
        tail = re.sub(r"^(\.\./)+src/[^/]+/", "", href)
        file_part, _, lines = tail.partition(".html")
        if src_root:
            lines = lines.replace("#", ":") if lines else ""
            root_rel = os.path.relpath(src_root.rstrip("/"), sys.argv[5])
            return "[Source](%s/src/%s%s)" % (root_rel, file_part, lines)
        return "[Source](%s)" % href
    if href.endswith(".html"):
        target = item_from_href(href)
        if target:
            return "[%s](%s)" % (label, target)
    return m.group(0)

text = open(sys.argv[6]).read()
print(re.sub(r"\[([^\]]+)\]\(([^)\s]*)\s*(?:\"([^\"]*)\")?\)", rewrite, text))
' "$crate" "$doc_root" "$src_root" "$page" "$here" "$md.trim"
