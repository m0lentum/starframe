use crate::event::{EventListener, EventListenerComponent, SpaceEvent};
use crate::space::Space;
use crate::storage::{ComponentStorage, CreateWithCapacity, VecStorage};
use crate::IdType;

/// A reusable builder type that creates objects in a Space with a given set of components.
/// Because this is reusable, all component types used must implement Clone.
/// To create an object from a recipe, call create() or try_create().
/// Recipes can also be cloned, so it's easy to create multiple different variants of one thing.
/// # Example
/// ```
/// let mut space = Space::with_capacity(100)
///     .with_component::<Position, VecStorage<_>>()
///     .with_component::<Shape, VecStorage<_>>();
///
/// let mut recipe = ObjectRecipe::new()
/// recipe
///     .add(Position{x: 1.0, y: 2.0})
///     .add(Shape::new_square(50.0, [1.0, 1.0, 1.0, 1.0]));
///
/// recipe.create(&mut space);
/// recipe.create(&mut space);
/// ```
pub struct ObjectRecipe {
    steps: Vec<Box<dyn ClonableStep>>,
    vars: VarMap,
    default_vars: VarMap,
}

type VarMap = anymap::Map<anymap::any::CloneAny>;

impl Clone for ObjectRecipe {
    fn clone(&self) -> Self {
        let mut cloned_steps = Vec::with_capacity(self.steps.len());
        for step in &self.steps {
            cloned_steps.push(step.clone_step());
        }
        ObjectRecipe {
            steps: cloned_steps,
            vars: self.vars.clone(),
            default_vars: self.default_vars.clone(),
        }
    }
}

trait ClonableStep {
    fn clone_step(&self) -> Box<dyn ClonableStep>;
    fn call(&self, space: &mut Space, id: IdType, vars: &VarMap);
}

impl<T> ClonableStep for T
where
    T: Fn(&mut Space, IdType, &VarMap) + Clone + 'static,
{
    fn clone_step(&self) -> Box<dyn ClonableStep> {
        Box::new(self.clone())
    }

    fn call(&self, space: &mut Space, id: IdType, vars: &VarMap) {
        self(space, id, vars);
    }
}

impl ObjectRecipe {
    pub fn new() -> Self {
        ObjectRecipe {
            steps: Vec::new(),
            vars: VarMap::new(),
            default_vars: VarMap::new(),
        }
    }

    /// Add the given component to the recipe.
    pub fn add<T: Clone + 'static>(&mut self, component: T) -> &mut Self {
        self.steps.push(Box::new(
            move |space: &mut Space, id: IdType, _: &VarMap| {
                space.create_component(id, component.clone())
            },
        ));

        self
    }

    /// Add the given EventListener to the recipe.
    /// Internally these are stored as components.
    pub fn add_listener<E, L>(&mut self, listener: L) -> &mut Self
    where
        E: SpaceEvent + 'static,
        L: EventListener<E> + Clone + 'static,
    {
        self.steps.push(Box::new(
            move |space: &mut Space, id: IdType, _: &VarMap| {
                space.create_component_safe::<_, VecStorage<_>>(
                    id,
                    EventListenerComponent {
                        listener: Box::new(listener.clone()),
                    },
                );
            },
        ));

        self
    }

    /// Add a component that is allowed to be modified to the recipe.
    /// If no default value is provided, the variable must be set elsewhere
    /// with `set_variable` before creating objects with the recipe.
    /// # Example
    /// ```
    /// let mut thingy = ObjectRecipe::new();
    /// thingy
    ///     .add_variable(Some(Position { x: 0.0, y: 0.0 }))
    ///     .add_variable(None::<Velocity>);
    ///
    /// // uncommenting this causes a panic because there's no Velocity
    /// //thingy.apply(&mut space);
    /// thingy.set_variable(Velocity { x: 1.0, y: 2.0 });
    /// thingy.apply(&mut space);
    /// ```
    pub fn add_variable<T: Clone + 'static>(&mut self, default: Option<T>) -> &mut Self {
        self.vars.insert(default.clone());
        self.default_vars.insert(default);

        self.steps
            .push(Box::new(|space: &mut Space, id: IdType, vars: &VarMap| {
                space.create_component(
                    id,
                    vars.get::<Option<T>>()
                        .unwrap()
                        .as_ref()
                        .expect("A recipe variable has not been set")
                        .clone(),
                );
            }));

        self
    }

    /// Set the value of a variable in the recipe.
    /// # Panics
    /// Panics if this type of variable does not exist in this recipe.
    pub fn set_variable<T: Clone + 'static>(&mut self, var: T) -> &mut Self {
        assert!(
            self.vars.insert(Some(var)).is_some(),
            "Attempted to set a variable that does not exist in the recipe"
        );

        self
    }

    /// Get the default value of a variable in the recipe, or None if it does not have one.
    pub fn get_default<T: Clone + 'static>(&self) -> Option<&T> {
        self.default_vars
            .get::<Option<T>>()
            .expect("Attempted to get the default value of a recipe variable that does not exist")
            .as_ref()
    }

    /// Reset all variables in the recipe to their default values.
    /// This includes removing values from the variables without default values.
    pub fn reset_variables(&mut self) {
        self.vars = self.default_vars.clone();
    }

    /// Have the object initially disabled. You'll probably want some mechanism that
    /// enables it later (an object pool, usually).
    pub fn start_disabled(&mut self) -> &mut Self {
        self.steps.push(Box::new(
            move |space: &mut Space, id: IdType, _: &VarMap| {
                space.actually_disable_object(id);
            },
        ));

        self
    }

    /// Like `add`, but adds a storage to the Space first if one doesn't exist yet.
    /// Creating the containers explicitly before adding objects is strongly encouraged
    /// (i.e. don't use this unless you have a good reason!).
    pub fn add_safe<T, S>(&mut self, component: T) -> &mut Self
    where
        T: Clone + 'static,
        S: ComponentStorage<T> + CreateWithCapacity + 'static,
    {
        self.steps.push(Box::new(
            move |space: &mut Space, id: IdType, _: &VarMap| {
                space.create_component_safe::<T, S>(id, component.clone());
            },
        ));

        self
    }

    /// Use this recipe to create an object in a Space.
    /// Returns the id of the object created.
    /// # Panics
    /// Panics if there is no room left in the Space, or if a variable has not been set.
    pub fn apply(&self, space: &mut Space) -> IdType {
        let id = space.create_object();
        for step in &self.steps {
            step.call(space, id, &self.vars);
        }
        id
    }

    /// Like `create`, but returns None instead of panicking if the Space is full.
    /// # Panics
    /// Panics if a variable included in the recipe has not been set.
    pub fn try_apply(&self, space: &mut Space) -> Option<IdType> {
        let id = space.try_create_object();
        match id {
            Some(id) => {
                for step in &self.steps {
                    step.call(space, id, &self.vars);
                }
            }
            None => {}
        }
        id
    }
}
