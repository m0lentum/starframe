# Starframe

## What

In disc golf, a starframe occurs when every player in a group scores a birdie on the same hole.

This starframe, however, is a 2D game engine written in Rust as a solo hobby project.
Its main feature is the physics engine, with design driven by sidescrolling action games.

## Current features

![Current state of graphics and physics](demo.gif)

- Novel graph-based entity system inspired by [froggy](https://github.com/kvark/froggy)
  - [related blog post](https://moletrooper.github.io/blog/2020/08/starframe-1-architecture/)
- 2D rigid body physics
  - collision detection for boxes and circles
  - constraint solver based on
    [Extended Position-Based Dynamics](https://matthias-research.github.io/pages/publications/PBDBodies.pdf)
- Graphics
  - Simple 2D mesh rendering with [wgpu](https://github.com/gfx-rs/wgpu-rs)

See my [kanban](https://github.com/MoleTrooper/starframe/projects/1) for the most up-to-date and fine-grained goings-on.

## Blog

I write about this project once in a blue moon on [my website](https://moletrooper.github.io/blog/).

## Running the test game

There's not much to show here, but should you wish to check out my tiny test game
where you bump physics blocks around, here's how:

### The manual way

1. Install [Rust](https://www.rust-lang.org/learn/get-started)
2. You may need to install `pkgconfig` and drivers for Vulkan, DX12, or Metal depending on your platform
3. Clone and navigate to this repository
4. `cargo run --example testgame`

### The easy way, using [Nix](https://nixos.org/nix/)

1. Clone and navigate to this repository
2. `nix-shell`
3. `cargo run --example testgame`

### Keybindings

Disclaimer: these might be out of date - the test game changes in quick and dirty ways

```text
Number keys 1-9 - load a scene
Enter           - reload current scene
P               - pause
Space           - step one frame while paused
Esc             - close the game

Arrows  - move the player
LShift  - jump
Z       - shoot
S       - spawn a box
T       - spawn a ball

Left mouse  - grab a box (in grab mode)
V           - change mouse mode between camera and grab
Mouse drag  - move the camera (in camera mode)
Mouse wheel - zoom the camera (in camera mode)
```
