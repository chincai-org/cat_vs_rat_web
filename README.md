# Cat vs Rat (Rust + WASM)

Mortis66666/Cat-vs-Rat rewrite in rust. Cats (arrow keys) hunt rats; rats (W A S D) eat all tomatoes.
Includes an in-browser map editor with local saving and share links.

## Run

- Tests:   cargo test -p cat_vs_rat_core
- Desktop: cargo run -p cat-vs-rat (run from the repo root so Assets/ is found)
- Web:     ./build-web.sh && python3 -m http.server -d dist 8000
- Pages:   push to main, then Settings > Pages > Source: GitHub Actions

## Map format

Maps are plain text: 10 lines of 20 characters, one character per cell.

```
.........#R......#..
.........##......#R.
.....##......##..#..
.....#T..CT...#.....
..#R.#..T##.C.#.##..
..##.#.C.##T..#.R#..
.....#...TC..T#.....
..#..##......##.....
.R#......##.........
..#......R#.........
```

| Symbol | Meaning |
| --- | --- |
| `.` | empty |
| `#` | box |
| `C` | cat |
| `R` | rat |
| `T` | tomato |
| `S` | spike |
| `[` `]` | hole (two adjacent cells) |

Rules:

- Exactly 10 lines, 20 characters each.
- Every cell holds one object or is empty.
- At least one cat, one rat, and one tomato.
- Holes: 0 or 2. Each hole is `[` followed by `]` on the same line.
- Unknown characters are rejected with a line and column.

Edit `maps/map_1.txt` in any text editor, or use the in-game editor. The map
list has **Copy map** and **Paste map** for moving maps as text. Custom maps
are stored in the browser's localStorage (a local file on desktop).

## Design choices (change in crates/core/src/game.rs)

- Fixed 60 Hz simulation; speeds are per second (cat 3.5, rat 7.0 cells/s).
- Collision uses a ground footprint at the bottom of each cell, so a sprite
  can walk behind the top of a box and stops at its base (2.5D). Vertically
  stacked boxes merge into one footprint, so there are no seams to slip through.
- Spikes start off and toggle every 1 s (original code; its README says 2 s).
- Only living rats eat tomatoes; cats only kill living rats; dead rats still block rats.
- Last direction key pressed wins (falls back to the next held key on release).
- If every cat is removed, the round ends as a draw (tomatoes remain).

## Editor and browser features

Custom maps can be renamed, copied as text, and imported from clipboard with **Paste map**. The editor warns when a cat cannot reach any rat, a rat cannot reach any tomato, or a tomato cannot be reached by any rat. During a round, the HUD counts down to the next cat removal. Movement is keyboard-only: arrow keys for cats and WASD for rats by default. Open **Controls** on the main menu to rebind either side; changes are saved locally and can be reset to defaults.

### URL parameters

- `?map=<name>` opens a saved map; `?map=1` selects the built-in map. `?map=` also accepts a share token or URL-encoded map text.
- `?debug` shows round time and remaining-object counts; `?debug=0` disables it.
- Existing share links using `#m=<token>` still work.

## How to play

### Cat

- Type: Player sprite
- Size: 1 x 1
- Controls: Keyboard arrow keys ↑ ← ↓ → (remappable in **Controls**)
- Game goal: Kill all rats and protect tomatoes
- Attributes: Can't walk over tomatoes and other cats

### Rat

- Type: Player sprite
- Size: 1 x 1
- Controls: W A S D (remappable in **Controls**)
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

## License

Source code is licensed under the [Apache License 2.0](LICENSE).

Game art and audio in `Assets/` come from
[Farm Puzzle Animals](https://comigo.itch.io/farm-puzzle-animals) and remain
under the asset author's terms; the Apache license covers the code only.
