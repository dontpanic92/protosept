use std::fmt;

#[derive(Debug, Clone)]
pub struct SourcePos {
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for SourcePos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {} column {}", self.line, self.col)
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
            } => format_error_with_pos!(&format!("Expected token: {}, found: {}", expected, found), pos),
            ParseError::UnexpectedEof { pos } => {
                format_error_with_pos!("Unexpected end of file", pos)
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
    UnexpectedStructRef,
    FunctionNotFound,
    VariableNotFound,
    Other(String),
}

impl std::error::Error for RuntimeError {}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeError::NoStackFrame => write!(f, "No stack frame available"),
            RuntimeError::EntryPointNotFound => write!(f, "Entry point not found"),
            RuntimeError::StackUnderflow => write!(f, "Stack underflow"),
            RuntimeError::UnexpectedStructRef => write!(f, "Unexpected struct reference"),
            RuntimeError::FunctionNotFound => write!(f, "Function not found"),
            RuntimeError::VariableNotFound => write!(f, "Variable not found"),
            RuntimeError::Other(msg) => write!(f, "Runtime error: {}", msg),
        }
    }
}
