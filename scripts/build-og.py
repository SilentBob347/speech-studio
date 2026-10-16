# Renders docs/og-<lang>.jpg (1200x630, the link preview of each page): the studio's mark and name, the page's
# headline in its language, and the hero screenshot of that language cropped on the right.
import base64, json, pathlib, subprocess, sys
from playwright.sync_api import sync_playwright

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
strings = json.loads(subprocess.run(["node", "-e", "import('./docs/site/strings.mjs').then(m => console.log(JSON.stringify(m.STRINGS)))"],
                                    cwd=ROOT, capture_output=True, text=True, encoding="utf-8", check=True).stdout)
mark = (DOCS / "favicon.svg").read_text(encoding="utf-8")
PAGE = """<!doctype html><html><head><meta charset="utf-8"><style>
body {{ margin: 0; width: 1200px; height: 630px; background: #0b0b0e; color: #ececef; overflow: hidden; position: relative;
  font-family: "Segoe UI", "Microsoft YaHei", sans-serif; }}
.left {{ position: absolute; left: 64px; top: 64px; width: 470px; }}
.brand {{ display: flex; align-items: center; gap: 18px; font-size: 32px; font-weight: 700; }}
.brand svg {{ width: 72px; height: 72px; border-radius: 18px; }}
h1 {{ margin: 92px 0 0; font-size: {size}px; line-height: 1.12; letter-spacing: -.02em; font-weight: 800; }}
.line {{ position: absolute; left: 64px; bottom: 84px; width: 180px; height: 6px; border-radius: 3px; background: linear-gradient(90deg, #a5b8ff, #4cc2ff); }}
.shot {{ position: absolute; left: 600px; top: 120px; width: 900px; border-radius: 14px 0 0 0; border: 1px solid rgba(255,255,255,.12);
  box-shadow: 0 20px 60px rgba(76,194,255,.15); }}
</style></head><body>
<div class="left"><div class="brand">{mark}<span>Speech Studio</span></div><h1>{title}</h1></div>
<div class="line"></div><img class="shot" src="data:image/webp;base64,{shot}">
</body></html>"""
with sync_playwright() as p:
    browser = p.chromium.launch()
    page = browser.new_page(viewport={"width": 1200, "height": 630})
    for lang, s in strings.items():
        shot = base64.b64encode((DOCS / "screenshots" / f"{lang}-01-speech.webp").read_bytes()).decode()
        title = s["heroTitle"]
        size = 54 if len(title) > 40 else 60
        page.set_content(PAGE.format(mark=mark, title=title, shot=shot, size=size))
        page.wait_for_timeout(300)
        page.screenshot(path=str(DOCS / f"og-{lang}.jpg"), type="jpeg", quality=88)
        print(f"[OK] docs/og-{lang}.jpg", flush=True)
    browser.close()
