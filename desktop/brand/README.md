# Ky desktop icon

Local desktop derivative of the existing Ky K mark. Not a new approved organization-wide identity.

## Provenance

- GitHub `webkubor/kyvault` main tree checked on 2026-11-12: no dedicated logo asset. `docs/index.html:53` uses a letter K; `docs/styles.css:2` supplies mint `#66f0b5` and charcoal `#080d10`.
- The local free image bed contained an older KEYRING shield/banner, not the current Ky mark; it was not reused.
- New hand-drawn SVG keeps K and the production colors, adding two key-cut steps to the lower diagonal. No external font, raster source, filters, network resource or generated-image dependency.
- The transparent symbol is `mark.svg` (light surfaces) / `mark-dark.svg` (dark surfaces). `icon.svg` adds a charcoal rounded tile for Dock and ordinary image elements. The bottom arm's key-cut detail is a secondary cue, not a claim of unique trademark ownership.

## Assets and validation

`icon-1024.png` is the large master render; `icon-512.png` supplies the desktop UI. Tauri PNG/ICO/ICNS files are in `../src-tauri/icons/`. `Ky.iconset` retains the macOS source resolutions.

Actual tools: `rsvg-convert`, Pillow, macOS `iconutil`, and the logo-brand-designer `build_review.py` manifest validator. All completed successfully. 32/512/1024 PNG alpha extrema are 0–255; corner pixels transparent; alpha bounds respectively `(1,1,31,31)`, `(24,24,488,488)`, `(48,48,976,976)`. Flat mint against its immediately adjacent charcoal is 13.67:1 (internal contrast measurement, not a WCAG certification).

`screenshots/rendered-review.png` is a renderer/Pillow contact sheet, not a running-app screenshot: 256px plus 48/32/24/16px icons, white and `#0d1117` backgrounds. `screenshots/marks.png` shows transparent symbols at 32/64/128px on those backgrounds. Both were visually inspected. The source canvas is 256 square; tile inset is 12, corner radius 52; glyph bounds are x=62–195, y=48–208. At tiny sizes recognition rests on K, not its two small cuts.

`review.html` was generated successfully from `manifest.json`; browser interactions, clipboard/download controls and actual macOS Dock rendering are not verified here. These require host acceptance after the application build. No color-vision simulation performed.

## Independent visual review

A separate image-reading agent reviewed the actual contact sheet, first blind and then with Ky's identity and existing mark explained. Resolved model: **unknown**; no gateway model receipt available. Review conversation: `d645cfce-c00f-49d2-b249-663540f55d7d`.

Blind question: what is A, is it generic, does it have a memory cue, is 24px legible, and are there rendering defects?

> “First blind read is a mint-green capital K on a near-black rounded square. The lower diagonal has a two-step/key-tooth notch … K silhouette remains clearly legible; the key-tooth detail becomes a small kink, not an immediately recognizable key. No visible clipping, broken contours, or obvious alignment/render defects.”

After product context and swap-name question:

> “Core glyph is a customized geometric K with one specific stepped detail—a viable, restrained derivative of the production mark … Recommend accept as desktop derivative if continuity is the goal; do not claim the notch provides strong independent brand recognition, especially at 24px where it mostly disappears.”

The reviewer also noted another K-named security product could plausibly use a similar concept. This is accepted as continuity-focused local application artwork, not a uniquely ownable new brand. No other candidates were presented; the unrelated legacy shield was excluded on provenance grounds.
