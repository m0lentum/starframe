// Idea: instead of the current super-generic solution where every component has its own container
// and we need a complicated macro and RwLocks and stuff to make queries,
// we store components in larger bundles determined by System-like manager objects.
// Kind of a cross between current solution, an archetype-based ECS and object-oriented composition.
// - can query stuff from these managers in a statically typed fashion
// - they can borrow from each other as they see fit, also statically typed and borrow-checked
//    - way easier to do concurrency, no need for RwLocks
// - memory locality is improved for common operations as every component needed is in one container instead of separate ones
//   (assuming components aren't borrowed or borrowing is done using copies)
// - con: need to know which manager owns which component type, possibility of multiple indepenedent instances of one component

pub struct Space<M: 'static> {
    alive_objects: BitSet,
    enabled_objects: BitSet,
    generations: Vec<u8>,
    next_obj_id: IdType,
    capacity: IdType,
    pools: AnyMap, // would these still work the same?
    pub managers: M,
}

// fragments = collections of related components

struct TransformFragment {
    transform: Transform,
}

struct RigidBodyFragment {
    // this could also simply be borrowed, but copying would improve memory locality
    // and in this case likely give a significant speed improvement
    tr: CopyOf<TransformFragment>,
    body: RigidBody,
}

struct TriggerFragment {
    tr: CopyOf<Transform>,
    collider: Collider,
    on_enter: EventHandler, // just an example, haven't really thought the events part through yet
}

struct ShapeFragment {
    transform: CopyOf<Transform>,
    shape: Shape,
}

// managers (better name pls) store fragments and give an interface to manipulate them
// kinda like a System and the data it uses mashed into one thing
// maybe just call them Systems? or something like GameLogic? SpaceFeatures? idk

struct TransformManager {
    fragments: Container<TransformFragment>,
}

// container would be analogous to current ComponentContainer
// but needs some more functionality that's currently in the ComponentQuery macro
struct Container<T> {
    // ...
}

impl<T> Container<T> {
    // get, get_mut, insert and all that

    fn combined_users<O>(&self, other: &Container<O>) -> BitSet {
        // just merge bitsets 4Head
    }

    fn iter(&self) -> impl Iterator<Item = T> {
        // just the bitset iterator mapped to get()
        // (although maybe a custom iterator type would be useful here)
    }

    fn zipped_iter(&self, other: &Container<O>) -> impl Iterator<Item = (T, O)> {
        // still seems pretty simple, but can we scale an approach like this to arbitrarily many containers?
        // on the other hand, do we need to? part of the point of this is to have fragments mostly self-contained,
        // rarely needing to get stuff from multiple containers
    }
}

struct PhysicsManager {
    bodies: Container<RigidBodyFragment>,
    triggers: Container<TriggerFragment>,
}

impl PhysicsManager {
    // needs to write TransformManager -> asks for a mut reference
    // it will hold the mut reference for the duration of updates
    // -> no one else can read or write transforms while this runs
    //
    // too restrictive? it would be nice to be able to e.g. start running physics while something else that reads transforms
    // is still in progress, but we definitely want to block everything that writes.
    // could split this so writing results is a separate method,
    // but then we need to manage access manually so nobody gets to write in between
    // this would potentially lead to problems with two systems that write Transforms running simultaneously
    //
    // P.S. actually this is exactly equivalent to current ECS behavior but checked at compile time
    // so doing it like this is already an epic win
    fn tick(&mut self, tr: &mut TransformManager) {
        self.sync_from(tr);
        // do physics things here
        self.sync_to(tr);
    }

    fn sync_from(&mut self, tr: &TransformManager) {
        // assuming Container iterators zip good
        // (only iterate over intersection of bitsets)
        //
        // although, would it actually be better to crash if no transform?
        // since a body can't function without one, this could be a nice way to catch those logic errors
        // still, zipping is gonna be necessary in other places
        for (body_frag, tr_frag) in self.bodies.iter_mut().zip(tr.iter()) {
            body_frag.tr = tr_frag.transform;
        }
    }

    fn sync_to(&self, tr: &mut TransformManager) {
        for (body_frag, tr_frag) in self.bodies.iter().zip(tr.iter_mut()) {
            tr_frag.transform = body_frag.tr;
        }
    }

    fn add_fragment(obj: ObjectHandle, frag: PhysicsFragment) {}
}

// Would be nicer to have most rendering primitives go through this but not sure if good
pub struct ShapeManager {
    fragments: Container<ShapeFragment>,
}

impl ShapeManager {
    fn draw(&mut self, tr: &TransformManager) {
        self.fragments
    }
}

// how would using this API look?

struct Managers {
    tr: TransformManager,
    physics: PhysicsManager,
    shape: ShapeManager,
}

impl ManagerBundle for Managers {
    fn tick(&mut self, dt: f32, space: &mut Space) {
        // do we actually need &mut Space for anything here?
        // events need to be fired and handled, but how exactly?
        self.physics.tick(&mut self.tr, dt);
        // could simply fork here to run multiple systems at the same time,
        // I'm pretty borrow checker would ensure no funny business
    }

    fn render(&mut self) {
        self.shape.draw(&self.tr);
    }
}

fn my_imaginary_main_with_blackjack_and_hookers() {
    let mut space = Space::new(100, Managers::new());
    // would this automatically add a TransformFragment?
    // if so, TransformManager needs to be stored in the space by default, not in our custom Managers struct
    // seems a little unwieldy, probably don't do that
    let obj_handle = space.create_object();
    // a little awkward, but I don't think there's a good way to make this a generic method of ObjectHandle
    let m = &mut space.managers;
    m.tr.add_fragment(&obj_handle, TransformFragment::new(Transform::new()));
    // I think Recipes won't need to change much, just call these methods instead of current ones
    // also need access to the managers, but I'm pretty sure that can be a type argument somewhere between RON and Space
    m.physics
        .add_fragment(&obj_handle, PhysicsFragment::new(RigidBody::new()));

    // this would call managers.tick()
    space.tick();
}
