use super::context::{Context, ContextResult, Data};
use crate::errors::RuntimeError;
use std::fmt;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeType {
    Any,
    Int,
    Float,
    Bool,
    String,
    Array,
    Tuple,
    Map,
    Closure,
    Foreign,
}

impl NativeType {
    pub fn accepts(&self, value: &Data) -> bool {
        match self {
            Self::Any => true,
            Self::Int => matches!(value, Data::Int(_)),
            Self::Float => matches!(value, Data::Float(_)),
            Self::Bool => matches!(value, Data::Int(0 | 1)),
            Self::String => matches!(value, Data::String(_)),
            Self::Array => matches!(value, Data::Array(_)),
            Self::Tuple => matches!(value, Data::Tuple(_)),
            Self::Map => matches!(value, Data::Map(_)),
            Self::Closure => matches!(value, Data::Closure { .. }),
            Self::Foreign => matches!(
                value,
                Data::Foreign { .. }
                    | Data::BoxRef { .. }
                    | Data::ProtoBoxRef { .. }
                    | Data::ProtoRefRef { .. }
            ),
        }
    }
}

impl fmt::Display for NativeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSignature {
    pub params: Vec<NativeType>,
    pub result: Option<NativeType>,
}

impl NativeSignature {
    pub fn new(params: Vec<NativeType>, result: Option<NativeType>) -> Self {
        Self { params, result }
    }
}

pub type NativeCallback = dyn Fn(&mut Context, &[Data]) -> ContextResult<Option<Data>> + 'static;

pub(crate) fn stack_adapter(
    name: String,
    signature: NativeSignature,
    callback: Rc<NativeCallback>,
) -> impl Fn(&mut Context) -> ContextResult<()> {
    move |context| {
        let stack_len = context
            .stack
            .last()
            .ok_or(RuntimeError::NoStackFrame)?
            .stack
            .len();
        if stack_len < signature.params.len() {
            return Err(RuntimeError::Other(format!(
                "Native function '{}' expected {} argument(s), but the VM stack contains {}",
                name,
                signature.params.len(),
                stack_len
            )));
        }

        let args = {
            let stack = &mut context.stack_frame_mut()?.stack;
            stack.split_off(stack.len() - signature.params.len())
        };
        for (index, (expected, actual)) in signature.params.iter().zip(&args).enumerate() {
            if !expected.accepts(actual) {
                return Err(RuntimeError::Other(format!(
                    "Native function '{}' argument {} expected {}, got {:?}",
                    name, index, expected, actual
                )));
            }
        }

        let output = callback(context, &args)?;
        match (&signature.result, output) {
            (None, None) => Ok(()),
            (None, Some(value)) => Err(RuntimeError::Other(format!(
                "Native function '{}' declared unit but returned {:?}",
                name, value
            ))),
            (Some(expected), Some(value)) if expected.accepts(&value) => {
                context.stack_frame_mut()?.stack.push(value);
                Ok(())
            }
            (Some(expected), Some(value)) => Err(RuntimeError::Other(format!(
                "Native function '{}' expected return type {}, got {:?}",
                name, expected, value
            ))),
            (Some(expected), None) => Err(RuntimeError::Other(format!(
                "Native function '{}' expected return type {} but returned unit",
                name, expected
            ))),
        }
    }
}
