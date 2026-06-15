use crate::ast::Identifier;
use crate::errors::{ParseError, SourcePos};
use crate::intern::InternedString;
use crate::lexer::{Token, TokenType};

use super::{ParseResult, Parser};

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser {
            tokens,
            position: 0,
        }
    }

    pub(crate) fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    pub(crate) fn peek_ahead(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.position + offset)
    }

    pub(crate) fn peek_previous(&self) -> Option<&Token> {
        self.tokens.get(self.position.checked_sub(1)?)
    }

    pub(crate) fn ends_with_brace(&self) -> bool {
        matches!(
            self.peek_previous(),
            Some(Token {
                token_type: TokenType::CloseBrace,
                ..
            })
        )
    }

    pub(crate) fn peek_match(&self, token_type: TokenType) -> bool {
        match self.peek() {
            Some(t) => t.token_type.discriminant() == token_type.discriminant(),
            _ => false,
        }
    }

    pub(crate) fn consume_match(&mut self, token_type: TokenType) -> ParseResult<()> {
        match self.peek() {
            Some(t) if t.token_type == token_type => {
                self.consume().unwrap();
                Ok(())
            }
            Some(t) => Err(ParseError::ExpectedToken {
                expected: format!("{:?}", token_type),
                found: format!("{:?}", t.token_type),
                pos: Some(SourcePos {
                    line: t.line,
                    col: t.col,
                    module: None,
                }),
            }),
            _ => Err(ParseError::UnexpectedEof {
                pos: self.peek_previous().map(|t| SourcePos {
                    line: t.line,
                    col: t.col,
                    module: None,
                }),
            }),
        }
    }

    /// Helper: Consume a specific token type and return its position, or error
    pub(crate) fn consume_expecting(&mut self, expected: TokenType) -> ParseResult<(usize, usize)> {
        match self.consume() {
            Some(Token {
                token_type,
                line,
                col,
                ..
            }) if *token_type == expected => Ok((*line, *col)),
            Some(t) => Err(ParseError::ExpectedToken {
                expected: format!("{:?}", expected),
                found: format!("{:?}", t.token_type),
                pos: Some(SourcePos {
                    line: t.line,
                    col: t.col,
                    module: None,
                }),
            }),
            None => Err(ParseError::UnexpectedEof {
                pos: self.peek_previous().map(|t| SourcePos {
                    line: t.line,
                    col: t.col,
                    module: None,
                }),
            }),
        }
    }

    pub(crate) fn parse_identifier(&mut self) -> ParseResult<Identifier> {
        match self.consume() {
            Some(Token {
                token_type: TokenType::Identifier(literal),
                line,
                col,
                ..
            }) => Ok(Identifier {
                name: literal.clone(),
                line: *line,
                col: *col,
            }),
            Some(t) => Err(ParseError::ExpectedToken {
                expected: format!("{:?}", TokenType::Identifier(InternedString::from(""))),
                found: format!("{:?}", t.token_type),
                pos: Some(SourcePos {
                    line: t.line,
                    col: t.col,
                    module: None,
                }),
            }),
            _ => Err(ParseError::UnexpectedEof {
                pos: self.peek_previous().map(|t| SourcePos {
                    line: t.line,
                    col: t.col,
                    module: None,
                }),
            }),
        }
    }

    pub(crate) fn consume(&mut self) -> Option<&Token> {
        if self.position < self.tokens.len() {
            let token = &self.tokens[self.position];
            self.position += 1;
            Some(token)
        } else {
            None
        }
    }
}
