use super::event::{EventListener, EventListenerComponent, SpaceEvent};
use super::space::Space;
use super::storage::{ComponentStorage, CreateWithCapacity, VecStorage};
use super::IdType;

use std::collections::HashMap;
use std::str::FromStr;

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
    parsers: HashMap<String, ParseFn>,
}

type ParseFn = fn(&str, &mut VarMap) -> Result<(), ()>;

type VarMap = anymap::Map<anymap::any::CloneAny>;

/// Wrapper trait for a Fn(&mut Space, IdType, &VarMap) + Clone + 'static
/// that can be used as a trait object.
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
            parsers: self.parsers.clone(),
        }
    }
}

impl ObjectRecipe {
    pub fn new() -> Self {
        ObjectRecipe {
            steps: Vec::new(),
            vars: VarMap::new(),
            default_vars: VarMap::new(),
            parsers: HashMap::new(),
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
    /// If you want to be able to parse the variable from a string, use `add_variable_named` instead.
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

    /// Like `add_variable`, but with the additional restriction of T: FromStr.
    /// A variable created with this can have its value parsed from a string.
    pub fn add_named_variable<T>(&mut self, name: &str, default: Option<T>) -> &mut Self
    where
        T: FromStr + Clone + 'static,
        <T as FromStr>::Err: std::fmt::Debug,
    {
        self.add_variable(default);

        self.parsers
            .insert(name.to_string(), Self::do_parse_var::<T>);

        self
    }

    fn do_parse_var<T>(src: &str, vars: &mut VarMap) -> Result<(), ()>
    where
        T: FromStr + Clone + 'static,
        <T as FromStr>::Err: std::fmt::Debug,
    {
        let item = src.parse::<T>().map_err(|_| ())?;
        vars.insert(Some(item));

        Ok(())
    }

    /// Parse a variable from a string and apply it to this recipe.
    pub(self) fn parse_variable(&mut self, name: &str, value: &str) -> Result<(), ParseVarError> {
        let parser = self.parsers.get(name).ok_or(ParseVarError::UnknownVar)?;

        parser(value, &mut self.vars).map_err(|_| ParseVarError::InvalidFormat)
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
        if let Some(id) = id {
            for step in &self.steps {
                step.call(space, id, &self.vars);
            }
        }
        id
    }
}

/// A (String, ObjectRecipe) map. The strings identify recipes when
/// parsing the MoleEngineSpace (MES) format.
pub struct RecipeBook {
    recipes: HashMap<String, ObjectRecipe>,
}

impl RecipeBook {
    /// Create an empty RecipeBook.
    pub fn new() -> Self {
        RecipeBook {
            recipes: HashMap::new(),
        }
    }

    /// Add a recipe with the given identifier to the book.
    pub fn add(&mut self, key: &str, recipe: ObjectRecipe) {
        self.recipes.insert(key.to_string(), recipe);
    }

    /// Get an immutable reference to a recipe in the book.
    pub fn get(&self, key: &str) -> Option<&ObjectRecipe> {
        self.recipes.get(key)
    }

    /// Get a mutable reference to a recipe in the book.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut ObjectRecipe> {
        self.recipes.get_mut(key)
    }
}

use pest::iterators::Pair;
use pest::Parser;

#[derive(Parser)]
#[grammar = "ecs/space.pest"]
struct SpaceParser;

/// Parse a string in the plaintext MoleEngineSpace (MES) format (currently undocumented)
/// into the given Space using the given RecipeBook to identify recipes.
/// If an error occurs during parsing, everything up to that point is still added to the Space.
pub fn parse_into_space(
    src: &str,
    space: &mut Space,
    recipes: &mut RecipeBook,
) -> Result<(), ParseSpaceError> {
    let everything = SpaceParser::parse(Rule::everything, src)
        .map_err(ParseSpaceError::Format)?
        .next()
        .unwrap();

    for object in everything.into_inner() {
        match object.as_rule() {
            Rule::object => eval_object(object, space, recipes)?,
            Rule::EOI => (),
            _ => unreachable!(),
        }
    }

    Ok(())
}

fn eval_object(
    object: Pair<Rule>,
    space: &mut Space,
    recipes: &mut RecipeBook,
) -> Result<(), ParseSpaceError> {
    let mut pairs = object.into_inner();
    let ident = pairs.next().unwrap();
    let (line_num, _) = ident.as_span().start_pos().line_col();
    let recipe = recipes
        .get_mut(ident.as_str())
        .ok_or_else(|| ParseSpaceError::UnknownRecipe(line_num, String::from(ident.as_str())))?;

    for var in pairs {
        eval_var(var, recipe)?;
    }

    recipe.apply(space);
    recipe.reset_variables();

    Ok(())
}

fn eval_var(var: Pair<Rule>, recipe: &mut ObjectRecipe) -> Result<(), ParseSpaceError> {
    let mut pairs = var.into_inner();
    let ident = pairs.next().unwrap();
    let value = pairs.next().unwrap();

    recipe
        .parse_variable(ident.as_str(), value.as_str())
        .map_err(|e| {
            let (line_num, _) = ident.as_span().start_pos().line_col();
            match e {
                ParseVarError::UnknownVar => {
                    ParseSpaceError::UnknownVar(line_num, String::from(value.as_str()))
                }
                ParseVarError::InvalidFormat => ParseSpaceError::ObjectFormat(line_num),
            }
        })
}

enum ParseVarError {
    UnknownVar,
    InvalidFormat,
}

/// An error type for parse errors on reading a MoleEngineSpace.
/// At least for now, failure to parse a component for an object only gives you
/// the line number and no details. As the format is very simple, this should
/// hopefully be enough to easily figure out the problem.
#[derive(Debug)]
pub enum ParseSpaceError {
    Format(pest::error::Error<Rule>),
    UnknownRecipe(usize, String),
    UnknownVar(usize, String),
    ObjectFormat(usize),
}

impl std::error::Error for ParseSpaceError {}

impl std::fmt::Display for ParseSpaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ParseSpaceError::Format(pest_err) => write!(f, "Invalid Space format! {}", pest_err),
            ParseSpaceError::UnknownRecipe(line, name) => {
                write!(f, "Unknown recipe identifier on line {}: '{}'", line, name)
            }
            ParseSpaceError::UnknownVar(line, name) => {
                write!(f, "Unknown recipe variable on line {}: '{}'", line, name)
            }
            ParseSpaceError::ObjectFormat(num) => {
                write!(f, "Failed to parse an object on line {}", num)
            }
        }
    }
}
