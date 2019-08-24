**ECS**
- serialize and deserialize levels using Serde
- more storage types (see specs)
- component containers should probably use Option instead of mem::uninitialized
  (at least try and see how much this affects performance)
- better error reporting for Systems
    - currently just fails and tells you it failed, could tell which component was missing etc.
    - don't panic when using recipe with unset variable
- ability to run Systems in parallel
    - this is already kind of possible but the API doesn't have good tools for it
    - investigate usefulness of Futures (maybe wait for async/await)
    - alternatively, a macro (something like `run_systems_par!(space, system1, system2, ...))`)
- reconsider Space builder syntax (use `cascade` instead?)
- preset recipes for common objects (can use as template for more specific stuff)
- figure out a better way to add a lot of components at once when loading level
  (currently does a hashmap lookup and a RwLock write access for every single component)
---
- ~~LockedAnyMap wrapper type to tidy up the syntax for Space-global state (AnyMap with everything RwLocked)~~
    - Consider using RwLock::try_read instead of read for non-blocking failure on untimely access
- ~~figure out a way to generate a Shape from a Collider in a recipe~~
    - probably not necessary now that recipes are good
- ~~automatic object pooling API~~
    - added but with some usability concerns, will have to try it in practice
- ~~optional components in ComponentFilters~~
    - ~~investigate using these instead of event listeners~~\
      probably not a good idea - event listeners should be called when no Systems are running
      so that they can have effects on any component of their choosing and be guaranteed not to block\
      **however**, this might gain a few microseconds e.g. with collision events
      if I don't push them into queue at all if there's no listener to receive them

**physics**
- collider types: ~~circle~~, ~~rect~~, polygon
- rigid body constraint solver
- ~~calculate masses from collider shape~~
- surface properties: restitution, friction
- use temporal coherence as heuristic to optimize collision detection narrow phase
    - i.e. start the SAT test from last frame's separating axis if any
- spatial partitioning
    - possible algorithms (probably try many; they can be easily swapped):
    - flat grid
    - hierarchical grid
    - quadtree
    - AABB tree
    - k-d tree
- joints

**graphics**
- camera
    - store in or out of Space? how best to access in rendering Systems?
    - smoothed movement
    - attach to game objects (as a separate thing linked to the object somehow or component of the object itself?)

**misc**
- add loading level from file to the project template
- try making an actual game with multiple levels, see how the
  design scales (level loading from RON? game state management
  between loading, playing and paused? etc etc)
- ability to 1. set and 2. dynamically reload settings (screen size, keybinds etc.)
- some UI framework (conrod / imgui?) or write my own?
