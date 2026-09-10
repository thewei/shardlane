# site/
> L2 | Parent: /CLAUDE.md

index.html: the single site page, deep-space aurora glassmorphism (design
final: designs/site-deai/ Option D, chosen by the user on 2026-09-09):
sticky glass floating nav (compact "scrolled" state past 24px), large hero
(pill badge + gradient accent headline), glass product-window mockup
(three backend tabs + Agent status + two panes), the multi-backend matrix
(Herdr / tmux / UU Remote / Luvus, each with its official logo,
positioning badge, capability list, and outbound link), the native
multi-workspace instance picker, the decoupled architecture pipeline
diagram, bento features, and a four-step install; download links point at
the Shardlane-macos-aarch64.zip GitHub Releases asset. Scroll-reveal
stagger comes from AOS 2.3.4 (cdnjs — the only external request) plus a
few inline lines for the nav state; a no-JS / no-CDN / reduced-motion
fallback keeps every element visible (html.no-js guard + CSS overrides).
styles.css: all styles; background #05060f deep space; brand accents
--accent #6d8bff / --accent2 #5af1df, backend colors
--herdr/--tmux/--uu/--luvus inherit each official brand. Design language:
fixed aurora blobs (three .aurora blur(90px) floating circles, disabled
under reduced-motion); glass = rgba white 6% + backdrop-filter
blur(16-24px) + a 1px white 12% line, used for nav/cards/product
window/pipeline; hero gradient accent headline; no JS, no external
requests; line count <= 800.
assets/icon.png: 512px copy of the app icon, from
assets/app-icon/shardlane-512.png, never hand-edited.
assets/mark.svg: copy of the brand Option-D mark, from
assets/app-icon/shardlane-d-dark.svg; nav and footer logo plus favicon.
assets/herdr-logo.png: official Herdr logo, from
herdr.dev/assets/logo.png (512px), used on the backend matrix cards.
assets/tmux-logo.svg: official tmux logo, from tmux.app/favicon.svg
(black >_ wordmark), used on the backend matrix cards.
assets/uu-logo.png: official NetEase UU Remote logo (white horizontal
wordmark, cropped to 256x60), from the uuyc.163.com static asset
uuyc.res.netease.com logo_light_134a4fbb.png; used on the backend matrix
cards and the mockup sidebar.
assets/luvus-logo.png: official Luvus logo, from
github.com/RizRiyz/luvus assets/logo.png (1000px), used on the backend
matrix cards.
CLAUDE.md: this file.

[PROTOCOL]: Update this header on change, then check CLAUDE.md.
