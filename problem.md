# Known problem: cold start is not fullscreen (black bar on top)

## Symptom

- Splash/boot screen shows correctly in fullscreen.
- When the app finishes loading, it appears with a **black status-bar-sized bar on top** — fullscreen "not being fullscreen".
- The bar only disappears after **minimizing and re-entering the app** or after **rotating the screen**.

## Affected apps

- `miated`, `fitny`, `mood-meter` (all installed PWAs, Android/Chrome).

## Context

- Manifests use `display: "fullscreen"` with `display_override: ["fullscreen", "standalone"]`.
- Cold start *after the first post-install launch* boots in fullscreen, but Chrome sizes the
  renderer for the old viewport and doesn't propagate a resize when the immersive state settles.

## Attempted fixes (in `index.html` of each app)

1. Root height pinned to `visualViewport.height` (`syncViewport` watchdog) — did not fix it.
2. Programmatic "immersive nudge": `requestFullscreen()` → `screen.orientation.lock(current)` →
   `exitFullscreen()` — works, but only takes effect **after the user touches the app**
   (browsers reject fullscreen without a user gesture), despite automatic retries at load.

## Current status

- Workaround works on first tap, not instantly at boot.
- A true fix likely requires Chromium to fire a resize/inset recalculation after the splash
  screen; nothing in page code can force it without a user gesture.
