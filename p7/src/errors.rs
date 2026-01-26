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
        match self {
            ParseError::UnexpectedToken { found, pos } => match pos {
                Some(p) => write!(f, "Unexpected token: {} at {}", found, p),
                None => write!(f, "Unexpected token: {}", found),
            },
            ParseError::ExpectedToken {
                expected,
                found,
                pos,
            } => match pos {
                Some(p) => write!(f, "Expected token: {}, found: {} at {}", expected, found, p),
                None => write!(f, "Expected token: {}, found: {}", expected, found),
            },
            ParseError::UnexpectedEof { pos } => match pos {
                Some(p) => write!(f, "Unexpected end of file at {}", p),
                None => write!(f, "Unexpected end of file"),
            },
        }
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
    Other(String),
}

impl std::error::Error for SemanticError {}

impl fmt::Display for SemanticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SemanticError::TypeNotFound { name, pos } => match pos {
                Some(p) => write!(f, "Type not found: {} at {}", name, p),
                None => write!(f, "Type not found: {}", name),
            },
            SemanticError::FunctionNotFound { name, pos } => match pos {
                Some(p) => write!(f, "Function not found: {} at {}", name, p),
                None => write!(f, "Function not found: {}", name),
            },
            SemanticError::VariableNotFound { name, pos } => match pos {
                Some(p) => write!(f, "Variable not found: {} at {}", name, p),
                None => write!(f, "Variable not found: {}", name),
            },
            SemanticError::TypeMismatch { lhs, rhs, pos } => match pos {
                Some(p) => write!(f, "Type mismatch: {} != {} at {}", lhs, rhs, p),
                None => write!(f, "Type mismatch: {} != {}", lhs, rhs),
            },
            SemanticError::MixedNamedAndPositional { name, pos } => match pos {
                Some(p) => write!(
                    f,
                    "Mixed positional and named arguments in call: {} at {}",
                    name, p
                ),
                None => write!(f, "Mixed positional and named arguments in call: {}", name),
            },
            SemanticError::VariableOutsideFunction { name, pos } => match pos {
                Some(p) => write!(
                    f,
                    "Variable cannot be defined outside functions: {} at {}",
                    name, p
                ),
                None => write!(f, "Variable cannot be defined outside functions: {}", name),
            },
            SemanticError::ImportError { module_path, pos } => {
                write!(f, "Cannot import module: {} at {}", module_path, pos)
            }
            SemanticError::Other(msg) => write!(f, "Semantic error: {}", msg),
        }
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
