use crate::errors::RuntimeError;

pub type ContextResult<T> = std::result::Result<T, RuntimeError>;

/// Type for host functions that can be called from p7 code
/// Takes a mutable reference to the context to access the stack
pub type HostFunction = fn(&mut super::Context) -> ContextResult<()>;

#[derive(Debug, Clone, PartialEq)]
pub enum Data {
    Int(i64),
    Float(f64),
    String(String),
    /// Reference to a heap-allocated struct (index into Context.heap).
    StructRef(u32),
    /// Reference to a heap-allocated box (index into Context.box_heap).
    /// For box<proto>, stores both the box index and the concrete type_id for dynamic dispatch.
    BoxRef(u32),
    /// Proto box reference: stores box index and concrete struct type_id for dynamic dispatch
    ProtoBoxRef {
        box_idx: u32,
        concrete_type_id: u32,
    },
    /// Proto ref reference: stores ref index and concrete struct type_id for dynamic dispatch
    ProtoRefRef {
        ref_idx: u32,
        concrete_type_id: u32,
    },
    /// Exception value (enum variant ID) - used for try-catch as special return value
    Exception(i64),
    /// Array value - immutable collection of Data values
    Array(Vec<Data>),
    /// Null value for nullable types
    Null,
    /// Some(value) for nullable types
    Some(Box<Data>),
    /// Closure value: function address + captured values
    Closure {
        func_addr: u32,
        captures: Vec<Data>,
    },
    /// Tuple value - immutable fixed-size collection of heterogeneous Data values
    Tuple(Vec<Data>),
    /// Map value - ordered collection of key-value pairs
    Map(Vec<(Data, Data)>),
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
        Data::String(value)
    }
}

macro_rules! arithmetic_op {
    ($self: ident, $op:tt) => {
        let b = $self.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
        let a = $self.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
        match (a, b) {
            (Data::Int(a), Data::Int(b)) => {
                $self.stack_frame_mut()?.stack.push(Data::Int(a $op b));
            }
            (Data::Float(a), Data::Float(b)) => {
                $self.stack_frame_mut()?.stack.push(Data::Float(a $op b));
            }
            (Data::Int(a), Data::Float(b)) => {
                $self.stack_frame_mut()?.stack.push(Data::Float((a as f64) $op b));
            }
            (Data::Float(a), Data::Int(b)) => {
                $self.stack_frame_mut()?.stack.push(Data::Float(a $op (b as f64)));
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
            (Data::BoxRef(_), _) | (_, Data::BoxRef(_))
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
        let b = $self.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
        let a = $self.stack_frame_mut()?.stack.pop().ok_or(RuntimeError::StackUnderflow)?;
        let is_equality_op = matches!(stringify!($op), "==" | "!=");
        match (a, b) {
            (Data::Int(a), Data::Int(b)) => {
                $self.stack_frame_mut()?.stack.push(Data::Int((a $op b) as i64));
            }
            (Data::Float(a), Data::Float(b)) => {
                $self.stack_frame_mut()?.stack.push(Data::Int((a $op b) as i64));
            }
            (Data::Int(a), Data::Float(b)) => {
                $self.stack_frame_mut()?.stack.push(Data::Int(((a as f64) $op b) as i64));
            }
            (Data::Float(a), Data::Int(b)) => {
                $self.stack_frame_mut()?.stack.push(Data::Int((a $op (b as f64)) as i64));
            }
            (Data::String(a), Data::String(b)) if is_equality_op => {
                $self.stack_frame_mut()?.stack.push(Data::Int((a $op b) as i64));
            }
            // Null equality comparisons
            (Data::Null, Data::Null) if is_equality_op => {
                $self.stack_frame_mut()?.stack.push(Data::Int(("null" $op "null") as i64));
            }
            (Data::Null, Data::Some(_)) | (Data::Some(_), Data::Null) if is_equality_op => {
                $self.stack_frame_mut()?.stack.push(Data::Int(("null" $op "some") as i64));
            }
            (Data::Some(_), Data::Some(_)) if is_equality_op => {
                // Two Some values: consider them equal for null-checking purposes
                // (the type system ensures this is only used for == null / != null)
                $self.stack_frame_mut()?.stack.push(Data::Int(("some" $op "some") as i64));
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
            (Data::BoxRef(_), _) | (_, Data::BoxRef(_))
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
}

pub struct Struct {
    pub fields: Vec<Data>,
}
