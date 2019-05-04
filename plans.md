Everything I've thought of doing but haven't gotten around to yet is here.

**ECS**
- more storage types (see specs)
- automatic object pooling API
- optional components in ComponentFilters
    - ~~investigate using these instead of event listeners~~\
      probably not a good idea - event listeners should be called when no Systems are running
      so that they can have effects on any component of their choosing and be guaranteed not to block
- better error reporting for Systems
    - currently just fails and tells you it failed, could tell which component was missing etc.
- ability to run Systems in parallel
    - this is already kind of possible but the API doesn't have good tools for it
    - investigate usefulness of Futures (maybe wait for async/await)
    - alternatively, a macro (something like `run_systems_par!(space, system1, system2, ...))`)
- no panic on unset recipe var
- reconsider Space builder syntax (use `cascade` instead?)
- preset recipes for common objects (can use as template for more specific stuff)
- ~~LockedAnyMap wrapper type to tidy up the syntax for Space-global state (AnyMap with everything RwLocked)~~
    - Consider using RwLock::try_read instead of read for non-blocking failure on untimely access

**physics**
- collider types: ~~circle~~, ~~rect~~, polygon
- rigid body constraint solver
- use temporal coherence as heuristic to optimize collision detection
- spatial partitioning (probably hierarchical grid)
- joints
- some form of fluid simulation (SPH, PBF, something mesh-based??)

**graphics**
- camera
    - store in or out of Space? how best to access in rendering Systems?
    - smoothed movement
    - attach to game objects (as a separate thing linked to the object somehow or component of the object itself?)

**misc**
- add loading level from file to the project template
- try making an actual game with multiple levels, see how the
  design scales (level loading from MES? game state management
  between loading, playing and paused? etc etc)

**open questions**
- additions to MES format?
- some UI framework (conrod / imgui?) or write my own?
- ~~graphics library? Piston feels too limited~~
    - glium implemented; possibly investigate vulkano in future
