# Cat vs Rat (Rust + WASM)

Mortis66666/Cat-vs-Rat rewrite in rust. Cats (arrow keys) hunt rats; rats (W A S D) eat all tomatoes.
Includes an in-browser map editor with local saving and share links.

## Run

- Tests:   cargo test -p cat_vs_rat_core
- Desktop: cargo run -p cat-vs-rat (run from the repo root so Assets/ is found)
- Web:     ./build-web.sh && python3 -m http.server -d dist 8000
- Pages:   push to main, then Settings > Pages > Source: GitHub Actions

## Design choices (change in crates/core/src/game.rs)

- Fixed 60 Hz simulation; speeds are per second (cat 3.5, rat 7.0 cells/s).
- Spikes start off and toggle every 1 s (original code; its README says 2 s).
- Only living rats eat tomatoes; cats only kill living rats; dead rats still block rats.
- Last direction key pressed wins (falls back to the next held key on release).
- If attrition removes every cat, the round ends as a draw (tomatoes remain).

## Editor and browser features

Custom maps can be renamed, copied as JSON, and imported from clipboard with **Paste JSON**. The editor warns when a cat cannot reach any rat, a rat cannot reach any tomato, or a tomato cannot be reached by any rat. During a round, the HUD shows the next cat-attrition countdown. Movement is keyboard-only: arrow keys for cats and WASD for rats.

### URL parameters

- `?map=<name>` opens a saved map; `?map=1` selects the built-in map. `?map=` also accepts a share token or URL-encoded map JSON.
- `?debug` shows round time and remaining-object counts; `?debug=0` disables it.
- Existing share links using `#m=<token>` still work.

## How to play

### Cat

- Type: Player sprite
- Size: 1 x 1
- Controls: Keyboard arrow keys ↑ ← ↓ →
- Game goal: Kill all rats and protect tomatoes
- Attributes: Can't walk over tomatoes and other cats

### Rat

- Type: Player sprite
- Size: 1 x 1
- Controls: W A S D
- Game goal: Eat all tomatoes and don't get eaten by a cat
- Attributes: Can't walk past other rats
- Player sprites will have a special ability in the future

### Box

- Type: Map object
- Size: 1 x 1
- Attributes: Player sprites cannot walk past it

### Hole

- Type: Map object
- Size: 2 x 1
- Attributes: If a sprite goes into the hole, it comes out of the other hole in the map. Every sprite can use the hole, but with a 2-second cooldown each time
- Quantity: 0 or 2

### Spike

- Type: Map object
- Size: 1 x 1

## More

[Asset source](https://comigo.itch.io/farm-puzzle-animals)
Play the game at this repository's GitHub Pages site.
