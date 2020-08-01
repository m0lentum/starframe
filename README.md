# What

In disc golf, a starframe occurs when every player in a group scores a birdie on the same hole.

This starframe, however, is a general-purpose 2D game engine written in Rust as a solo hobby project.
Its design is driven by sidescrolling action games and cool physics tools for them,
but its core building blocks should generalize to other styles of game quite well.

# Current features

- Novel graph-based object model inspired by [froggy](https://github.com/kvark/froggy)
- 2D rigid body physics
  - narrow phase collision detection
  - rudimentary collision impulse solver
- Graphics
  - Simple 2D mesh rendering with [wgpu](https://github.com/gfx-rs/wgpu-rs)

See the [kanban](https://github.com/MoleTrooper/starframe/projects/1) for the most up-to-date goings-on.

# Blog

There used to be a blog link here, but
progress on this project has been slow and I've put it on ice for now.
I plan to get back to it in the fall of 2020 as I begin my master's degree studies;
until then, stay tuned.

# Running the test game

There's not much to show here, but should you wish to check out my tiny test game
where you bump physics blocks around, here's how:

**The manual way**

1. Install [Rust](https://www.rust-lang.org/learn/get-started)
2. You may need to install `pkgconfig` and drivers for Vulkan, DX12, or Metal depending on your platform
3. Clone and navigate to this repository
4. `cargo run --example testgame`

**The easy way, using [Nix](https://nixos.org/nix/)**

1. Clone and navigate to this repository
2. `nix-shell`
3. `cargo run --example testgame`

### Keybindings

Disclaimer: these might be out of date - the test game changes in quick and dirty ways

```
Arrows  - move the player
LShift  - jump
Z       - shoot
Space   - pause
Enter   - reload level
Esc     - close the game
S       - spawn a box
T       - spawn a ball

Mouse drag  - move the camera
Mouse wheel - zoom the camera
```
