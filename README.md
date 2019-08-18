MoleEngine is a general-purpose 2D game engine written in Rust as a solo hobby project.
Its primary focuses are cool physics tools and a simple interface that allows for quick prototyping.

[Introductory blog post](https://moletrooper.github.io/blog/2018/09/moleengine-part-0-introduction/)

# Implemented features

* Entity-Component-System framework
    * functional but with some loose ends
* 2D rigid body physics
    * narrow phase collision detection
    * rudimentary collision impulse solver
* Graphics
    * Simple 2D mesh rendering with [glium](https://github.com/glium/glium); heavily WIP

See [plans.md](./plans.md) for notes on future developments.

# Blog

Blog posts regarding this project and other things can be found
once in a blue moon on my [personal website](https://moletrooper.github.io/blog/).

# Running the test game

There's not much to show here, but thanks to the Rust toolchain it's very easy to
check out the test game should you wish to do so.

1. Install [Rust](https://www.rust-lang.org/learn/get-started)
2. Clone and navigate to this repository
3. `cargo run --features all --example testgame`

### Testgame keybindings

Disclaimer: these might be out of date - the test game changes in quick and dirty ways

```
Space   - pause
Enter   - load / reload level
Esc     - close the game
S       - spawn a box
Arrows  - nudge the "player" box around
PgUp/Dn - spin the "player" box
LShift  - stop the "player" box from moving
```
