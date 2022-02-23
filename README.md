# Starframe

## What

In disc golf, a starframe occurs when every player in a group scores a birdie
on the same hole.

This starframe, however, is a 2D game engine written in Rust as a solo hobby
project. Its main feature is the physics engine, with design driven by
sidescrolling action games. It is currently being developed alongside
[Flamegrower](https://github.com/MoleTrooper/flamegrower), a platformer
about vines and fire.

## Current features

![Current state of graphics and physics](demo.gif)

- Novel graph-based entity system inspired by [froggy](https://github.com/kvark/froggy)
  - [related blog post](https://moletrooper.github.io/blog/2020/08/starframe-1-architecture/)
    (somewhat outdated; many details have changed since)
- 2D rigid body and particle physics
  - collider shapes: boxes, circles, and capsules
  - particle-based ropes with full coupling with rigid bodies
  - constraint solver based on
    [Extended Position-Based Dynamics](https://matthias-research.github.io/pages/publications/PBDBodies.pdf)
    - [related blog post](https://moletrooper.github.io/blog/2021/03/starframe-devlog-constraints/)
- Graphics
  - Basic 2D mesh rendering with [wgpu](https://github.com/gfx-rs/wgpu-rs)
  - Dynamic outlines with the Jump Flood algorithm

Future plans and ideas are constantly changing and can be found in the form of
Obsidian kanban boards in [my notes
repo](https://github.com/MoleTrooper/notes).

## Blog

I write about this project once in a blue moon on [my website](https://moletrooper.me/blog/).

## Sandbox example

I have a little sandbox I use for testing new features where you can throw
blocks around with the mouse and move a rudimentary platformer character that
shoots some rather heavy bullets. Here's how you can check it out:

### The manual way

1. Install [Rust](https://www.rust-lang.org/learn/get-started)
2. You may need to install `pkgconfig` and drivers for Vulkan, DX12, or Metal
   depending on your platform
3. Clone and navigate to this repository
4. `cargo run --example sandbox`

### The easy way, using [Nix](https://nixos.org/nix/) (on Linux)

Note that this requires a Nix version that supports flakes (2.4 and up).

1. Clone and navigate to this repository
2. `nix develop`
3. `cargo run --example sandbox`

### Keybindings

Disclaimer: these might be out of date - the sandbox changes in quick and
dirty ways

```text
Space   - step one frame while paused

Arrows  - move the player
LShift  - jump
Z       - shoot

Left mouse  - grab a box (in grab mode)
V           - change mouse mode between camera and grab
Mouse drag  - move the camera (in camera mode)
Mouse wheel - zoom the camera (in camera mode)
```
