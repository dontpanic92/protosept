use crate::ast::Statement;
use crate::errors::ParseError;
use crate::lexer::{Token, TokenType};

const UNARY_OPERATIONS: &[TokenType] = &[
    TokenType::Not,
    TokenType::Exclamation,
    TokenType::Plus,
    TokenType::Minus,
    TokenType::Multiply, // unary `*` for deref of `ref<T>`
];

type ParseResult<T> = Result<T, ParseError>;
pub struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

macro_rules! match_token {
    ($token:expr, $($pattern:pat $(if $guard:expr)? => $body: expr $(,)?)*) => {
        match $token {
            $(Some(&Token {
                token_type: $pattern,
                ..
            }) $(if $guard)? => $body,)*
                        Some(t) => Err(ParseError::UnexpectedToken { found: format!("{:?}", t.token_type), pos: Some(SourcePos { line: t.line, col: t.col, module: None }) }),
            _ => Err(ParseError::UnexpectedEof { pos: None }),
        }
    };
}

mod declarations;
mod expressions;
mod statements;
mod tokens;
mod types;

impl Parser {
    pub fn parse(&mut self) -> ParseResult<Vec<Statement>> {
        let mut statements = Vec::new();

        while self.peek().is_some() {
            statements.push(self.parse_statement()?);
        }

        Ok(statements)
    }
}
