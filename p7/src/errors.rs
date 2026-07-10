use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct SourcePos {
    pub line: usize,
    pub col: usize,
    pub module: Option<String>,
}

impl SourcePos {
    /// Create an Option<SourcePos> from line and column numbers
    pub fn at(line: usize, col: usize) -> Option<Self> {
        Some(SourcePos {
            line,
            col,
            module: None,
        })
    }
}

impl fmt::Display for SourcePos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.module {
            Some(m) if m != "$root" => write!(
                f,
                "line {} column {} in module '{}'",
                self.line, self.col, m
            ),
            _ => write!(f, "line {} column {}", self.line, self.col),
        }
    }
}

// Helper macro to reduce boilerplate in error Display implementations
// Formats error messages with optional position information
macro_rules! format_error_with_pos {
    ($msg:expr, $pos:expr) => {
        match $pos {
            Some(p) => format!("{} at {}", $msg, p),
            None => $msg.to_string(),
        }
    };
}

#[derive(Debug)]
pub enum Proto7Error {
    ParseError(ParseError),
    SemanticError(SemanticError),
    RuntimeError(RuntimeError),
}

impl std::error::Error for Proto7Error {}

impl fmt::Display for Proto7Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Proto7Error::ParseError(e) => write!(f, "{}", e),
            Proto7Error::SemanticError(e) => write!(f, "{}", e),
            Proto7Error::RuntimeError(e) => write!(f, "{}", e),
        }
    }
}

impl From<ParseError> for Proto7Error {
    fn from(err: ParseError) -> Self {
        Proto7Error::ParseError(err)
    }
}

impl From<SemanticError> for Proto7Error {
    fn from(err: SemanticError) -> Self {
        Proto7Error::SemanticError(err)
    }
}

impl From<RuntimeError> for Proto7Error {
    fn from(err: RuntimeError) -> Self {
        Proto7Error::RuntimeError(err)
    }
}

#[derive(Debug)]
pub enum ParseError {
    UnexpectedToken {
        found: String,
        pos: Option<SourcePos>,
    },
    ExpectedToken {
        expected: String,
        found: String,
        pos: Option<SourcePos>,
    },
    UnexpectedEof {
        pos: Option<SourcePos>,
    },
    Other {
        message: String,
        pos: Option<SourcePos>,
    },
}

impl std::error::Error for ParseError {}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            ParseError::UnexpectedToken { found, pos } => {
                format_error_with_pos!(&format!("Unexpected token: {}", found), pos)
            }
            ParseError::ExpectedToken {
                expected,
                found,
                pos,
            } => format_error_with_pos!(
                &format!("Expected token: {}, found: {}", expected, found),
                pos
            ),
            ParseError::UnexpectedEof { pos } => {
                format_error_with_pos!("Unexpected end of file", pos)
            }
            ParseError::Other { message, pos } => {
                format_error_with_pos!(message.as_str(), pos)
            }
        };
        write!(f, "{}", msg)
    }
}

#[derive(Debug)]
pub enum SemanticError {
    TypeNotFound {
        name: String,
        pos: Option<SourcePos>,
    },
    FunctionNotFound {
        name: String,
        pos: Option<SourcePos>,
    },
    VariableNotFound {
        name: String,
        pos: Option<SourcePos>,
    },
    TypeMismatch {
        lhs: String,
        rhs: String,
        pos: Option<SourcePos>,
    },
    MixedNamedAndPositional {
        name: String,
        pos: Option<SourcePos>,
    },
    MissingArgument {
        param_name: String,
        func_name: String,
        pos: Option<SourcePos>,
    },
    VariableOutsideFunction {
        name: String,
        pos: Option<SourcePos>,
    },
    ImportError {
        module_path: String,
        pos: SourcePos,
    },
    UseAfterMove {
        name: String,
        pos: Option<SourcePos>,
    },
    DiscardedMustUseValue {
        ty: String,
        pos: Option<SourcePos>,
    },
    NonExhaustiveMatch {
        scrutinee_ty: String,
        missing: String,
        pos: Option<SourcePos>,
    },
    Other(String),
}

impl std::error::Error for SemanticError {}

impl fmt::Display for SemanticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            SemanticError::TypeNotFound { name, pos } => {
                format_error_with_pos!(&format!("Type not found: {}", name), pos)
            }
            SemanticError::FunctionNotFound { name, pos } => {
                format_error_with_pos!(&format!("Function not found: {}", name), pos)
            }
            SemanticError::VariableNotFound { name, pos } => {
                format_error_with_pos!(&format!("Variable not found: {}", name), pos)
            }
            SemanticError::TypeMismatch { lhs, rhs, pos } => {
                format_error_with_pos!(&format!("Type mismatch: {} != {}", lhs, rhs), pos)
            }
            SemanticError::MissingArgument {
                param_name,
                func_name,
                pos,
            } => {
                format_error_with_pos!(
                    &format!(
                        "Missing required argument '{}' in call to '{}'",
                        param_name, func_name
                    ),
                    pos
                )
            }
            SemanticError::MixedNamedAndPositional { name, pos } => {
                format_error_with_pos!(
                    &format!("Mixed positional and named arguments in call: {}", name),
                    pos
                )
            }
            SemanticError::VariableOutsideFunction { name, pos } => {
                format_error_with_pos!(
                    &format!("Variable cannot be defined outside functions: {}", name),
                    pos
                )
            }
            SemanticError::ImportError { module_path, pos } => {
                format!("Cannot import module: {} at {}", module_path, pos)
            }
            SemanticError::UseAfterMove { name, pos } => {
                format_error_with_pos!(&format!("Use of moved value: {}", name), pos)
            }
            SemanticError::DiscardedMustUseValue { ty, pos } => {
                format_error_with_pos!(
                    &format!(
                        "Discarded value of `#[must_use]` type `{}`; bind it with `let _ = ...` or use it",
                        ty
                    ),
                    pos
                )
            }
            SemanticError::NonExhaustiveMatch {
                scrutinee_ty,
                missing,
                pos,
            } => {
                format_error_with_pos!(
                    &format!(
                        "Non-exhaustive match on `{}`: {} not covered. Add a wildcard arm `_ => ...` or cover the missing case.",
                        scrutinee_ty, missing
                    ),
                    pos
                )
            }
            SemanticError::Other(msg) => format!("Semantic error: {}", msg),
        };
        write!(f, "{}", msg)
    }
}

#[derive(Debug)]
pub enum RuntimeError {
    NoStackFrame,
    EntryPointNotFound,
    StackUnderflow,
    UnexpectedStructRef(String),
    FunctionNotFound,
    VariableNotFound(String),
    /// Dereference of a `box_idx` whose slot has been freed and possibly
    /// reused. The carried generation does not match the slot's current
    /// generation. This typically indicates that a Rust-owned `Data`
    /// value was held across a script call that triggered GC.
    StaleBoxHandle {
        idx: u32,
        expected_gen: u32,
        actual_gen: u32,
    },
    StaleForeignHandle {
        type_tag: String,
        handle: i64,
    },
    Other(String),
}

impl std::error::Error for RuntimeError {}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeError::NoStackFrame => write!(f, "No stack frame available"),
            RuntimeError::EntryPointNotFound => write!(f, "Entry point not found"),
            RuntimeError::StackUnderflow => write!(f, "Stack underflow"),
            RuntimeError::UnexpectedStructRef(detail) => {
                write!(f, "Unexpected struct reference: {}", detail)
            }
            RuntimeError::FunctionNotFound => write!(f, "Function not found"),
            RuntimeError::VariableNotFound(detail) => write!(f, "Variable not found: {}", detail),
            RuntimeError::StaleBoxHandle {
                idx,
                expected_gen,
                actual_gen,
            } => write!(
                f,
                "Stale box handle: box {} expected generation {} but slot is at generation {} (the slot was freed by GC and possibly reused)",
                idx, expected_gen, actual_gen
            ),
            RuntimeError::StaleForeignHandle { type_tag, handle } => write!(
                f,
                "Stale foreign handle: '{}' object {} has been invalidated",
                type_tag, handle
            ),
            RuntimeError::Other(msg) => write!(f, "Runtime error: {}", msg),
        }
    }
}
