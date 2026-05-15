use crate::errors::RuntimeError;
use indexmap::IndexMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

pub type ContextResult<T> = std::result::Result<T, RuntimeError>;
pub type SharedStr = Rc<str>;
pub type SharedArray = Rc<Vec<Data>>;
pub type SharedTuple = Rc<Vec<Data>>;
pub type SharedMap = Rc<RuntimeMap>;
pub type SharedData = Rc<Data>;
pub type SharedCaptures = Rc<Vec<Data>>;

pub(crate) const SMALL_MAP_MAX_ENTRIES: usize = 8;

/// Type for host functions that can be called from p7 code
/// Takes a mutable reference to the context to access the stack
pub type HostFunction = fn(&mut super::Context) -> ContextResult<()>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MapKey {
    Int(i64),
    String(SharedStr),
    Null,
    Some(Box<MapKey>),
    Tuple(Vec<MapKey>),
    Exception(i64),
}

impl MapKey {
    pub(crate) fn try_from_data(data: &Data) -> ContextResult<Self> {
        match data {
            Data::Int(value) => Ok(MapKey::Int(*value)),
            Data::String(value) => Ok(MapKey::String(value.clone())),
            Data::Null => Ok(MapKey::Null),
            Data::Some(value) => Ok(MapKey::Some(Box::new(MapKey::try_from_data(value)?))),
            Data::Tuple(elements) => {
                let mut keys = Vec::with_capacity(elements.len());
                for element in elements.iter() {
                    keys.push(MapKey::try_from_data(element)?);
                }
                Ok(MapKey::Tuple(keys))
            }
            Data::Exception(value) => Ok(MapKey::Exception(*value)),
            Data::Float(_) => Err(RuntimeError::Other(
                "HashMap key type is not hashable at runtime: float".to_string(),
            )),
            Data::StructRef(_) => Err(RuntimeError::Other(
                "HashMap key type is not hashable at runtime: struct reference".to_string(),
            )),
            Data::BoxRef { .. } | Data::ProtoBoxRef { .. } | Data::ProtoRefRef { .. } => {
                Err(RuntimeError::Other(
                    "HashMap key type is not hashable at runtime: box/proto reference".to_string(),
                ))
            }
            Data::Array(_) => Err(RuntimeError::Other(
                "HashMap key type is not hashable at runtime: array".to_string(),
            )),
            Data::Closure { .. } => Err(RuntimeError::Other(
                "HashMap key type is not hashable at runtime: closure".to_string(),
            )),
            Data::Map(_) => Err(RuntimeError::Other(
                "HashMap key type is not hashable at runtime: map".to_string(),
            )),
            Data::Foreign { .. } => Err(RuntimeError::Other(
                "HashMap key type is not hashable at runtime: foreign value".to_string(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapEntry {
    key_hash: MapKey,
    key: Data,
    value: Data,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeMap {
    storage: RuntimeMapStorage,
}

#[derive(Debug, Clone, PartialEq)]
enum RuntimeMapStorage {
    Small(Vec<MapEntry>),
    Large(IndexMap<MapKey, MapEntry>),
}

impl RuntimeMap {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        if capacity > SMALL_MAP_MAX_ENTRIES {
            return Self {
                storage: RuntimeMapStorage::Large(IndexMap::with_capacity(capacity)),
            };
        }

        Self {
            storage: RuntimeMapStorage::Small(Vec::with_capacity(capacity)),
        }
    }

    pub(crate) fn from_pairs(pairs: Vec<(Data, Data)>) -> ContextResult<Self> {
        let mut map = Self::with_capacity(pairs.len());
        for (key, value) in pairs {
            map.insert(key, value)?;
        }
        Ok(map)
    }

    pub(crate) fn len(&self) -> usize {
        match &self.storage {
            RuntimeMapStorage::Small(entries) => entries.len(),
            RuntimeMapStorage::Large(entries) => entries.len(),
        }
    }

    pub(crate) fn insert(&mut self, key: Data, value: Data) -> ContextResult<()> {
        let key_hash = MapKey::try_from_data(&key)?;

        match &mut self.storage {
            RuntimeMapStorage::Small(entries) => {
                if let Some(entry) = entries.iter_mut().find(|entry| entry.key_hash == key_hash) {
                    entry.value = value;
                    return Ok(());
                }

                if entries.len() >= SMALL_MAP_MAX_ENTRIES {
                    self.promote_to_large();
                    return self.insert_large(key_hash, key, value);
                }

                entries.push(MapEntry {
                    key_hash,
                    key,
                    value,
                });
            }
            RuntimeMapStorage::Large(_) => self.insert_large(key_hash, key, value)?,
        }
        Ok(())
    }

    pub(crate) fn get(&self, key: &Data) -> ContextResult<Option<&Data>> {
        let key_hash = MapKey::try_from_data(key)?;
        Ok(match &self.storage {
            RuntimeMapStorage::Small(entries) => entries
                .iter()
                .find(|entry| entry.key_hash == key_hash)
                .map(|entry| &entry.value),
            RuntimeMapStorage::Large(entries) => entries.get(&key_hash).map(|entry| &entry.value),
        })
    }

    pub(crate) fn contains_key(&self, key: &Data) -> ContextResult<bool> {
        let key_hash = MapKey::try_from_data(key)?;
        Ok(match &self.storage {
            RuntimeMapStorage::Small(entries) => {
                entries.iter().any(|entry| entry.key_hash == key_hash)
            }
            RuntimeMapStorage::Large(entries) => entries.contains_key(&key_hash),
        })
    }

    pub(crate) fn remove(&mut self, key: &Data) -> ContextResult<Option<Data>> {
        let key_hash = MapKey::try_from_data(key)?;
        Ok(match &mut self.storage {
            RuntimeMapStorage::Small(entries) => entries
                .iter()
                .position(|entry| entry.key_hash == key_hash)
                .map(|idx| entries.remove(idx).value),
            RuntimeMapStorage::Large(entries) => {
                entries.shift_remove(&key_hash).map(|entry| entry.value)
            }
        })
    }

    pub(crate) fn keys(&self) -> Vec<Data> {
        match &self.storage {
            RuntimeMapStorage::Small(entries) => {
                entries.iter().map(|entry| entry.key.clone()).collect()
            }
            RuntimeMapStorage::Large(entries) => {
                entries.values().map(|entry| entry.key.clone()).collect()
            }
        }
    }

    pub(crate) fn values(&self) -> Vec<Data> {
        match &self.storage {
            RuntimeMapStorage::Small(entries) => {
                entries.iter().map(|entry| entry.value.clone()).collect()
            }
            RuntimeMapStorage::Large(entries) => {
                entries.values().map(|entry| entry.value.clone()).collect()
            }
        }
    }

    pub(crate) fn entries(&self) -> Vec<&MapEntry> {
        match &self.storage {
            RuntimeMapStorage::Small(entries) => entries.iter().collect(),
            RuntimeMapStorage::Large(entries) => entries.values().collect(),
        }
    }

    pub(crate) fn values_mut(&mut self) -> Vec<&mut Data> {
        match &mut self.storage {
            RuntimeMapStorage::Small(entries) => {
                entries.iter_mut().map(|entry| &mut entry.value).collect()
            }
            RuntimeMapStorage::Large(entries) => entries
                .values_mut()
                .map(|entry| &mut entry.value)
                .collect(),
        }
    }

    fn insert_large(&mut self, key_hash: MapKey, key: Data, value: Data) -> ContextResult<()> {
        let RuntimeMapStorage::Large(entries) = &mut self.storage else {
            unreachable!("insert_large requires large storage");
        };

        if let Some(entry) = entries.get_mut(&key_hash) {
            entry.value = value;
        } else {
            entries.insert(
                key_hash.clone(),
                MapEntry {
                    key_hash,
                    key,
                    value,
                },
            );
        }
        Ok(())
    }

    fn promote_to_large(&mut self) {
        let RuntimeMapStorage::Small(entries) =
            std::mem::replace(&mut self.storage, RuntimeMapStorage::Small(Vec::new()))
        else {
            return;
        };

        let mut indexed = IndexMap::with_capacity(entries.len() + 1);
        for entry in entries {
            indexed.insert(entry.key_hash.clone(), entry);
        }
        self.storage = RuntimeMapStorage::Large(indexed);
    }
}

impl MapEntry {
    pub(crate) fn key(&self) -> &Data {
        &self.key
    }

    pub(crate) fn value(&self) -> &Data {
        &self.value
    }
}

impl Eq for RuntimeMap {}

impl Hash for MapKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            MapKey::Int(value) => {
                0u8.hash(state);
                value.hash(state);
            }
            MapKey::String(value) => {
                1u8.hash(state);
                value.hash(state);
            }
            MapKey::Null => {
                2u8.hash(state);
            }
            MapKey::Some(value) => {
                3u8.hash(state);
                value.hash(state);
            }
            MapKey::Tuple(values) => {
                4u8.hash(state);
                values.hash(state);
            }
            MapKey::Exception(value) => {
                5u8.hash(state);
                value.hash(state);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Data {
    Int(i64),
    Float(f64),
    String(SharedStr),
    /// Reference to a heap-allocated struct (index into Context.heap).
    StructRef(u32),
    /// Reference to a heap-allocated box (slab handle into Context.box_heap).
    /// `generation` is the slot generation at allocation time; it is validated on
    /// every dereference so that handles to freed (and possibly reused)
    /// slots fail fast instead of silently aliasing.
    BoxRef { idx: u32, generation: u32 },
    /// Proto box reference: stores box slab handle and concrete struct
    /// type_id for dynamic dispatch.
    ///
    /// `origin_module_idx` records the module that produced this reference
    /// (i.e. the module whose `types[concrete_type_id]` is the concrete
    /// struct). Lookups that consult `Context::modules` to resolve method
    /// dispatch must use this module index; the executing frame's module
    /// may not own the receiver's type.
    ///
    /// `generation` is the slot generation at allocation time; see [`Data::BoxRef`].
    ProtoBoxRef {
        box_idx: u32,
        generation: u32,
        concrete_type_id: u32,
        origin_module_idx: u32,
    },
    /// Proto ref reference: stores struct heap index and concrete struct
    /// type_id for dynamic dispatch. `generation` is reserved (currently always 0)
    /// because the struct heap does not yet have a GC; included for symmetry
    /// and to make stale-handle detection a uniform concern.
    /// See [`Data::ProtoBoxRef`] for the meaning of `origin_module_idx`.
    ProtoRefRef {
        ref_idx: u32,
        generation: u32,
        concrete_type_id: u32,
        origin_module_idx: u32,
    },
    /// Exception value (enum variant ID) - used for try-catch as special return value
    Exception(i64),
    /// Array value - immutable collection of Data values
    Array(SharedArray),
    /// Null value for nullable types
    Null,
    /// Some(value) for nullable types
    Some(SharedData),
    /// Closure value: function address + captured values
    Closure {
        func_addr: u32,
        captures: SharedCaptures,
    },
    /// Tuple value - immutable fixed-size collection of heterogeneous Data values
    Tuple(SharedTuple),
    /// Map value - ordered collection of key-value pairs
    Map(SharedMap),
    /// A handle to a host-owned object backing an `@foreign` proto value.
    /// `type_tag` identifies the proto (matches `@foreign(type_tag="...")`),
    /// `handle` is an opaque host-defined token, and `owned` distinguishes
    /// owning `box<F>` (finalizer fires on drop) from borrowing `ref<F>`.
    Foreign {
        type_tag: String,
        handle: i64,
        owned: bool,
    },
}

impl Data {
    pub fn string(value: impl AsRef<str>) -> Self {
        Data::String(Rc::<str>::from(value.as_ref()))
    }

    pub fn array(elements: Vec<Data>) -> Self {
        Data::Array(Rc::new(elements))
    }

    pub fn tuple(elements: Vec<Data>) -> Self {
        Data::Tuple(Rc::new(elements))
    }

    pub fn map(pairs: Vec<(Data, Data)>) -> Self {
        Data::Map(Rc::new(
            RuntimeMap::from_pairs(pairs).expect("Data::map requires hashable keys"),
        ))
    }

    pub(crate) fn try_map(pairs: Vec<(Data, Data)>) -> ContextResult<Self> {
        Ok(Data::Map(Rc::new(RuntimeMap::from_pairs(pairs)?)))
    }

    pub fn some(value: Data) -> Self {
        Data::Some(Rc::new(value))
    }

    pub fn closure(func_addr: u32, captures: Vec<Data>) -> Self {
        Data::Closure {
            func_addr,
            captures: Rc::new(captures),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Data::String(value) => Some(value.as_ref()),
            _ => None,
        }
    }

    pub fn array_elements(&self) -> Option<&[Data]> {
        match self {
            Data::Array(elements) => Some(elements.as_slice()),
            _ => None,
        }
    }

    pub fn tuple_elements(&self) -> Option<&[Data]> {
        match self {
            Data::Tuple(elements) => Some(elements.as_slice()),
            _ => None,
        }
    }

    pub fn map_entries(&self) -> Option<Vec<&MapEntry>> {
        match self {
            Data::Map(map) => Some(map.entries()),
            _ => None,
        }
    }

    pub fn as_some(&self) -> Option<&Data> {
        match self {
            Data::Some(value) => Some(value.as_ref()),
            _ => None,
        }
    }

    pub fn array_elements_mut_in_box(&mut self) -> Option<&mut Vec<Data>> {
        match self {
            Data::Array(elements) => Some(Rc::make_mut(elements)),
            _ => None,
        }
    }

    pub fn map_mut_in_box(&mut self) -> Option<&mut RuntimeMap> {
        match self {
            Data::Map(map) => Some(Rc::make_mut(map)),
            _ => None,
        }
    }
}

impl From<i64> for Data {
    fn from(value: i64) -> Self {
        Data::Int(value)
    }
}

impl From<f64> for Data {
    fn from(value: f64) -> Self {
        Data::Float(value)
    }
}

impl From<String> for Data {
    fn from(value: String) -> Self {
        Data::string(value)
    }
}

impl From<&str> for Data {
    fn from(value: &str) -> Self {
        Data::string(value)
    }
}

macro_rules! arithmetic_op {
    ($self: ident, $op:tt) => {
        let frame = $self.stack_frame_mut()?;
        let (a, b) = frame.pop2()?;
        match (a, b) {
            (Data::Int(a), Data::Int(b)) => {
                frame.push(Data::Int(a $op b));
            }
            (Data::Float(a), Data::Float(b)) => {
                frame.push(Data::Float(a $op b));
            }
            (Data::Int(a), Data::Float(b)) => {
                frame.push(Data::Float((a as f64) $op b));
            }
            (Data::Float(a), Data::Int(b)) => {
                frame.push(Data::Float(a $op (b as f64)));
            }
            (Data::String(_), _) | (_, Data::String(_)) => {
                // Arithmetic on strings is invalid.
                return Err(RuntimeError::Other("Arithmetic on string".to_string()));
            }
            (Data::StructRef(r), _) | (_, Data::StructRef(r)) => {
                return Err(RuntimeError::UnexpectedStructRef(format!(
                    "cannot perform arithmetic on struct reference (ref {})",
                    r
                )));
            }
            (Data::BoxRef { .. }, _) | (_, Data::BoxRef { .. })
            | (Data::ProtoBoxRef { .. }, _) | (_, Data::ProtoBoxRef { .. })
            | (Data::ProtoRefRef { .. }, _) | (_, Data::ProtoRefRef { .. }) => {
                // Arithmetic on box/proto references is invalid.
                return Err(RuntimeError::Other("Arithmetic on box/proto reference".to_string()));
            }
            (Data::Exception(_), _) | (_, Data::Exception(_)) => {
                // Arithmetic on exceptions is invalid.
                return Err(RuntimeError::Other("Arithmetic on exception value".to_string()));
            }
            (Data::Array(_), _) | (_, Data::Array(_)) => {
                // Arithmetic on arrays is invalid.
                return Err(RuntimeError::Other("Arithmetic on array".to_string()));
            }
            (Data::Null, _) | (_, Data::Null) | (Data::Some(_), _) | (_, Data::Some(_)) => {
                // Arithmetic on nullable values is invalid.
                return Err(RuntimeError::Other("Arithmetic on nullable value".to_string()));
            }
            (Data::Closure { .. }, _) | (_, Data::Closure { .. }) => {
                return Err(RuntimeError::Other("Arithmetic on closure".to_string()));
            }
            (Data::Tuple(_), _) | (_, Data::Tuple(_)) => {
                return Err(RuntimeError::Other("Arithmetic on tuple".to_string()));
            }
            (Data::Map(_), _) | (_, Data::Map(_)) => {
                return Err(RuntimeError::Other("Arithmetic on map".to_string()));
            }
            (Data::Foreign { .. }, _) | (_, Data::Foreign { .. }) => {
                return Err(RuntimeError::Other("Arithmetic on foreign value".to_string()));
            }
        }
    };
}

macro_rules! comparison_op {
    ($self: ident, $op:tt) => {
        let frame = $self.stack_frame_mut()?;
        let (a, b) = frame.pop2()?;
        let is_equality_op = matches!(stringify!($op), "==" | "!=");
        match (a, b) {
            (Data::Int(a), Data::Int(b)) => {
                frame.push(Data::Int((a $op b) as i64));
            }
            (Data::Float(a), Data::Float(b)) => {
                frame.push(Data::Int((a $op b) as i64));
            }
            (Data::Int(a), Data::Float(b)) => {
                frame.push(Data::Int(((a as f64) $op b) as i64));
            }
            (Data::Float(a), Data::Int(b)) => {
                frame.push(Data::Int((a $op (b as f64)) as i64));
            }
            (Data::String(a), Data::String(b)) if is_equality_op => {
                frame.push(Data::Int((a $op b) as i64));
            }
            // Null equality comparisons
            (Data::Null, Data::Null) if is_equality_op => {
                frame.push(Data::Int(("null" $op "null") as i64));
            }
            (Data::Null, Data::Some(_)) | (Data::Some(_), Data::Null) if is_equality_op => {
                frame.push(Data::Int(("null" $op "some") as i64));
            }
            (Data::Some(_), Data::Some(_)) if is_equality_op => {
                // Two Some values: consider them equal for null-checking purposes
                // (the type system ensures this is only used for == null / != null)
                frame.push(Data::Int(("some" $op "some") as i64));
            }
            (Data::String(_), _) | (_, Data::String(_)) => {
                return Err(RuntimeError::Other("Comparison on string".to_string()));
            }
            (Data::StructRef(r), _) | (_, Data::StructRef(r)) => {
                return Err(RuntimeError::UnexpectedStructRef(format!(
                    "cannot compare struct reference (ref {}) with non-struct value",
                    r
                )));
            }
            (Data::BoxRef { .. }, _) | (_, Data::BoxRef { .. })
            | (Data::ProtoBoxRef { .. }, _) | (_, Data::ProtoBoxRef { .. })
            | (Data::ProtoRefRef { .. }, _) | (_, Data::ProtoRefRef { .. }) => {
                return Err(RuntimeError::Other("Comparison on box/proto reference".to_string()));
            }
            (Data::Exception(_), _) | (_, Data::Exception(_)) => {
                return Err(RuntimeError::Other("Comparison on exception value".to_string()));
            }
            (Data::Array(_), _) | (_, Data::Array(_)) => {
                return Err(RuntimeError::Other("Comparison on array".to_string()));
            }
            (Data::Null, _) | (_, Data::Null) | (Data::Some(_), _) | (_, Data::Some(_)) => {
                return Err(RuntimeError::Other("Comparison on nullable value".to_string()));
            }
            (Data::Closure { .. }, _) | (_, Data::Closure { .. }) => {
                return Err(RuntimeError::Other("Comparison on closure".to_string()));
            }
            (Data::Tuple(_), _) | (_, Data::Tuple(_)) => {
                return Err(RuntimeError::Other("Comparison on tuple".to_string()));
            }
            (Data::Map(_), _) | (_, Data::Map(_)) => {
                return Err(RuntimeError::Other("Comparison on map".to_string()));
            }
            (Data::Foreign { .. }, _) | (_, Data::Foreign { .. }) => {
                return Err(RuntimeError::Other("Comparison on foreign value".to_string()));
            }
        }
    };
}

pub struct StackFrame {
    pub params: Vec<Data>,
    pub locals: Vec<Data>,
    pub stack: Vec<Data>,
    pub pc: usize,
    pub module_idx: usize, // Which module this frame is executing from
}

impl StackFrame {
    pub(crate) fn new() -> Self {
        Self {
            params: Vec::new(),
            locals: Vec::new(),
            stack: Vec::new(),
            pc: std::usize::MAX,
            module_idx: 0, // Default to main module
        }
    }

    pub(crate) fn pop(&mut self) -> ContextResult<Data> {
        self.stack.pop().ok_or(RuntimeError::StackUnderflow)
    }

    pub(crate) fn pop2(&mut self) -> ContextResult<(Data, Data)> {
        let b = self.pop()?;
        let a = self.pop()?;
        Ok((a, b))
    }

    pub(crate) fn push(&mut self, value: Data) {
        self.stack.push(value);
    }
}

pub struct Struct {
    pub fields: Vec<Data>,
}
