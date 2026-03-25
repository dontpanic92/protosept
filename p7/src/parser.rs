use std::ops::Deref;

use crate::ast::{
    Attribute, Effect, EnumVariant, Expression, FunctionCall, FunctionDeclaration, Identifier,
    InterpolatedStringPart, NamedPattern, Parameter, Pattern, ProtoMethod, Statement, StructField,
    StructMethod, Type, TypeParameter,
};
use crate::errors::{ParseError, SourcePos};
use crate::lexer::{InterpolatedStringPart as LexInterpolatedStringPart, Lexer, Token, TokenType};

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

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser {
            tokens,
            position: 0,
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn peek_ahead(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.position + offset)
    }

    fn peek_previous(&self) -> Option<&Token> {
        self.tokens.get(self.position.checked_sub(1)?)
    }

    fn ends_with_brace(&self) -> bool {
        matches!(
            self.peek_previous(),
            Some(Token {
                token_type: TokenType::CloseBrace,
                ..
            })
        )
    }

    fn peek_match(&self, token_type: TokenType) -> bool {
        match self.peek() {
            Some(t) => t.token_type.discriminant() == token_type.discriminant(),
            _ => false,
        }
    }

    fn consume_match(&mut self, token_type: TokenType) -> ParseResult<()> {
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
    fn consume_expecting(&mut self, expected: TokenType) -> ParseResult<(usize, usize)> {
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

    fn parse_identifier(&mut self) -> ParseResult<Identifier> {
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
                expected: format!("{:?}", TokenType::Identifier("".to_string())),
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

    fn consume(&mut self) -> Option<&Token> {
        if self.position < self.tokens.len() {
            let token = &self.tokens[self.position];
            self.position += 1;
            Some(token)
        } else {
            None
        }
    }

    fn parse_field_access(&mut self, object: Expression) -> ParseResult<Expression> {
        self.consume_match(TokenType::Dot)?;

        // Parse field name - can be either an identifier or an integer (for tuple-like access)
        let field = match self.peek() {
            Some(Token {
                token_type: TokenType::Integer(n),
                line,
                col,
                ..
            }) => {
                let name = n.to_string();
                let line = *line;
                let col = *col;
                self.consume(); // consume the integer token
                Identifier { name, line, col }
            }
            _ => self.parse_identifier()?,
        };

        Ok(Expression::FieldAccess {
            object: Box::new(object),
            field,
        })
    }

    fn parse_expression_suffix(&mut self, mut expression: Expression) -> ParseResult<Expression> {
        loop {
            // Check for generic type arguments after an identifier: Container<int>
            if let Expression::Identifier(ref ident) = expression {
                if self.peek_match(TokenType::LessThan) {
                    // Try to parse as generic instantiation
                    if let Ok(type_args) = self.try_parse_type_arguments() {
                        expression = Expression::GenericInstantiation {
                            base: ident.clone(),
                            type_args,
                        };
                        continue;
                    }
                    // If failed, it's a comparison operator, break out
                    break;
                }
            }

            if self.peek_match(TokenType::OpenParen) {
                expression = self.parse_function_call(expression)?;
            } else if self.peek_match(TokenType::Dot) {
                expression = self.parse_field_access(expression)?;
            } else if self.peek_match(TokenType::OpenBracket) {
                // Parse array indexing: arr[index]
                let line = self.peek().unwrap().line;
                let col = self.peek().unwrap().col;
                self.consume(); // consume '['
                let index = self.parse_expression()?;
                self.consume_match(TokenType::CloseBracket)?;
                expression = Expression::ArrayIndex {
                    array: Box::new(expression),
                    index: Box::new(index),
                    pos: (line, col),
                };
            } else if self.peek_match(TokenType::Exclamation) {
                // Parse force unwrap: expr!
                let token = self.peek().unwrap().clone();
                self.consume(); // consume '!'
                expression = Expression::ForceUnwrap {
                    operand: Box::new(expression),
                    token,
                };
            } else {
                break;
            }
        }
        Ok(expression)
    }

    fn parse_primary_expression(&mut self) -> ParseResult<Expression> {
        let expression = if let Some(token) = self.peek().cloned() {
            match &token.token_type {
                TokenType::Integer(value) => {
                    self.consume();
                    Expression::IntegerLiteral(*value)
                }
                TokenType::Float(value) => {
                    self.consume();
                    Expression::FloatLiteral(*value)
                }
                TokenType::StringLiteral(value) => {
                    self.consume();
                    Expression::StringLiteral(value.clone())
                }
                TokenType::InterpolatedString(parts) => {
                    self.consume();
                    self.parse_interpolated_string(parts.clone())?
                }
                TokenType::Null => {
                    self.consume();
                    Expression::NullLiteral
                }
                TokenType::True => {
                    self.consume();
                    Expression::BooleanLiteral(true)
                }
                TokenType::False => {
                    self.consume();
                    Expression::BooleanLiteral(false)
                }
                TokenType::Identifier(_) => {
                    let identifier = self.parse_identifier()?;
                    Expression::Identifier(identifier)
                }
                TokenType::OpenBrace => {
                    // Disambiguate: map literal {key: value, ...} vs block {stmts}
                    if self.is_map_literal_start() {
                        return self.parse_map_literal();
                    }
                    let statements = self.parse_block()?;
                    Expression::Block(statements)
                }
                TokenType::OpenParen => {
                    // Check if this is a closure: (params) => expr
                    if self.is_closure_start() {
                        return self.parse_closure_expression();
                    }
                    // Parenthesized expression or tuple literal
                    let pos = (token.line, token.col);
                    self.consume(); // consume '('
                    let first_expr = self.parse_expression()?;
                    if self.peek_match(TokenType::Comma) {
                        // Tuple literal: (e1, e2, ...)
                        self.consume(); // consume ','
                        let mut elements = vec![first_expr];
                        while !self.peek_match(TokenType::CloseParen) {
                            elements.push(self.parse_expression()?);
                            if self.peek_match(TokenType::Comma) {
                                self.consume();
                            } else if !self.peek_match(TokenType::CloseParen) {
                                return Err(ParseError::UnexpectedToken {
                                    found: format!("{:?}", self.peek().map(|t| &t.token_type)),
                                    pos: self.peek().map(|t| SourcePos {
                                        line: t.line,
                                        col: t.col,
                                        module: None,
                                    }),
                                });
                            }
                        }
                        self.consume_match(TokenType::CloseParen)?;
                        let expr = Expression::TupleLiteral { elements, pos };
                        return self.parse_expression_suffix(expr);
                    } else {
                        // Parenthesized expression (grouping)
                        self.consume_match(TokenType::CloseParen)?;
                        return self.parse_expression_suffix(first_expr);
                    }
                }
                TokenType::Try => {
                    return self.parse_try_expression();
                }
                TokenType::Match => {
                    return self.parse_match_expression();
                }
                TokenType::If => {
                    return self.parse_if_expression();
                }
                TokenType::Loop => {
                    return self.parse_loop_expression();
                }
                TokenType::While => {
                    return self.parse_while_expression();
                }
                TokenType::Break => {
                    return self.parse_break_expression();
                }
                TokenType::Continue => {
                    return self.parse_continue_expression();
                }
                TokenType::OpenBracket => {
                    // Parse array literal [e1, e2, ...]
                    let pos = (token.line, token.col);
                    self.consume(); // consume '['
                    let mut elements = Vec::new();

                    while let Some(token) = self.peek() {
                        if token.token_type == TokenType::CloseBracket {
                            self.consume();
                            break;
                        }

                        elements.push(self.parse_expression()?);

                        if self.peek_match(TokenType::Comma) {
                            self.consume();
                        } else if !self.peek_match(TokenType::CloseBracket) {
                            return Err(ParseError::UnexpectedToken {
                                found: format!("{:?}", self.peek().map(|t| &t.token_type)),
                                pos: self.peek().map(|t| SourcePos {
                                    line: t.line,
                                    col: t.col,
                                    module: None,
                                }),
                            });
                        }
                    }

                    Expression::ArrayLiteral { elements, pos }
                }
                TokenType::Ref => {
                    self.consume();
                    self.consume_match(TokenType::OpenParen)?;
                    let expr = self.parse_expression()?;
                    self.consume_match(TokenType::CloseParen)?;
                    Expression::Ref(Box::new(expr))
                }
                TokenType::Box => {
                    // box(expr) - parse as intrinsic function call
                    let pos = (token.line, token.col);
                    self.consume();
                    self.consume_match(TokenType::OpenParen)?;
                    let arg = self.parse_expression()?;
                    self.consume_match(TokenType::CloseParen)?;
                    Expression::FunctionCall(FunctionCall {
                        callee: std::boxed::Box::new(Expression::Identifier(Identifier {
                            name: "box".to_string(),
                            line: pos.0,
                            col: pos.1,
                        })),
                        arguments: vec![(None, arg)],
                    })
                }
                _ => {
                    return Err(ParseError::UnexpectedToken {
                        found: format!("{:?}", token.token_type),
                        pos: Some(SourcePos {
                            line: token.line,
                            col: token.col,
                            module: None,
                        }),
                    });
                }
            }
        } else {
            return Err(ParseError::UnexpectedEof {
                pos: self.peek_previous().map(|t| SourcePos {
                    line: t.line,
                    col: t.col,
                    module: None,
                }),
            });
        };

        self.parse_expression_suffix(expression)
    }

    fn parse_interpolated_string(
        &mut self,
        parts: Vec<LexInterpolatedStringPart>,
    ) -> ParseResult<Expression> {
        let mut ast_parts = Vec::new();

        for part in parts {
            match part {
                LexInterpolatedStringPart::Literal(text) => {
                    ast_parts.push(InterpolatedStringPart::Literal(text));
                }
                LexInterpolatedStringPart::Expr(source) => {
                    let expr = self.parse_interpolated_expression(source)?;
                    ast_parts.push(InterpolatedStringPart::Expr(expr));
                }
            }
        }

        let has_expr = ast_parts
            .iter()
            .any(|part| matches!(part, InterpolatedStringPart::Expr(_)));

        if !has_expr {
            let mut combined = String::new();
            for part in ast_parts {
                if let InterpolatedStringPart::Literal(text) = part {
                    combined.push_str(&text);
                }
            }
            return Ok(Expression::StringLiteral(combined));
        }

        Ok(Expression::InterpolatedString { parts: ast_parts })
    }

    fn parse_interpolated_expression(&mut self, source: String) -> ParseResult<Expression> {
        if source.trim().is_empty() {
            return Err(ParseError::UnexpectedToken {
                found: "empty interpolation expression".to_string(),
                pos: None,
            });
        }

        let mut lexer = Lexer::new(source);
        let mut tokens = Vec::new();

        loop {
            let token = lexer.next_token();
            let is_eof = matches!(token.token_type, TokenType::EOF);
            tokens.push(token);
            if is_eof {
                break;
            }
        }

        if let Some(err) = lexer.errors.first() {
            return Err(ParseError::UnexpectedToken {
                found: format!("interpolated expression lexer error: {}", err),
                pos: None,
            });
        }

        let mut parser = Parser::new(tokens);
        let expr = parser.parse_expression()?;

        if !parser.peek_match(TokenType::EOF) {
            let token = parser.peek().unwrap();
            return Err(ParseError::UnexpectedToken {
                found: format!("{:?}", token.token_type),
                pos: Some(SourcePos {
                    line: token.line,
                    col: token.col,
                    module: None,
                }),
            });
        }

        Ok(expr)
    }

    fn parse_function_call(&mut self, identifier: Expression) -> ParseResult<Expression> {
        let pos = (self.peek().unwrap().line, self.peek().unwrap().col);
        self.consume_match(TokenType::OpenParen)?;

        // Check for struct update syntax: Type(...base, field = val)
        if self.peek_match(TokenType::DotDotDot) {
            return self.parse_struct_update_args(identifier, pos);
        }

        let mut arguments = Vec::new();
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseParen {
                self.consume();
                break;
            }

            let expr = self.parse_expression()?;

            let arg = if let Expression::Binary {
                left,
                operator:
                    Token {
                        token_type: TokenType::Assignment,
                        ..
                    },
                right,
            } = &expr
                && let Expression::Identifier(ident) = left.as_ref()
            {
                (Some(ident.clone()), right.deref().clone())
            } else {
                (None, expr)
            };

            arguments.push(arg);

            let comma = self.consume_match(TokenType::Comma);
            if !self.peek_match(TokenType::CloseParen) {
                comma?;
            }
        }

        match identifier {
            Expression::Identifier(identifier) => Ok(Expression::FunctionCall(FunctionCall {
                callee: Box::new(Expression::Identifier(identifier)),
                arguments,
            })),
            Expression::FieldAccess { object, field } => {
                Ok(Expression::FunctionCall(FunctionCall {
                    callee: Box::new(Expression::FieldAccess { object, field }),
                    arguments,
                }))
            }
            other => Ok(Expression::FunctionCall(FunctionCall {
                callee: Box::new(other),
                arguments,
            })),
        }
    }

    /// Parse struct update arguments: Type(...base, field = val)
    /// Called after '(' and DotDotDot have been peeked.
    fn parse_struct_update_args(&mut self, struct_name: Expression, pos: (usize, usize)) -> ParseResult<Expression> {
        self.consume_match(TokenType::DotDotDot)?;

        let base = self.parse_expression()?;
        let mut updates = Vec::new();

        while self.peek_match(TokenType::Comma) {
            self.consume(); // consume ','
            if self.peek_match(TokenType::CloseParen) {
                break; // trailing comma
            }
            let field_name = self.parse_identifier()?;
            self.consume_match(TokenType::Assignment)?;
            let value = self.parse_expression()?;
            updates.push((field_name, value));
        }

        self.consume_match(TokenType::CloseParen)?;

        Ok(Expression::StructUpdate {
            struct_name: Box::new(struct_name),
            base: Box::new(base),
            updates,
            pos,
        })
    }

    fn parse_unary_expression(&mut self) -> ParseResult<Expression> {
        if let Some(token) = self.peek() {
            if UNARY_OPERATIONS.contains(&token.token_type) {
                let operator = self.consume().unwrap().clone();
                let right = self.parse_unary_expression()?;
                return Ok(Expression::Unary {
                    operator,
                    right: Box::new(right),
                });
            }
        }

        return self.parse_primary_expression();
    }

    fn parse_expression(&mut self) -> ParseResult<Expression> {
        self.parse_binary_expression(0)
    }

    fn get_precedence(token_type: &TokenType) -> u8 {
        match token_type {
            TokenType::Assignment => 1,
            TokenType::DoubleQuestion => 2, // null-coalescing
            TokenType::Or | TokenType::Pipe => 3,
            TokenType::And => 4,
            TokenType::Caret => 5,
            TokenType::Ampersand => 6,
            TokenType::Equals | TokenType::NotEquals => 7,
            TokenType::GreaterThan
            | TokenType::GreaterThanOrEqual
            | TokenType::LessThan
            | TokenType::LessThanOrEqual => 8,
            TokenType::Plus | TokenType::Minus => 9,
            TokenType::Multiply | TokenType::Divide | TokenType::Percent => 10,
            _ => 0,
        }
    }

    fn parse_binary_expression(&mut self, min_prec: u8) -> ParseResult<Expression> {
        let mut left = self.parse_unary_expression()?;

        while let Some(token) = self.peek() {
            // Handle 'as' cast expression
            if token.token_type == TokenType::As {
                self.consume(); // consume 'as'
                let target_type = self.parse_type()?;
                left = Expression::Cast {
                    expression: Box::new(left),
                    target_type,
                };
                continue;
            }

            let prec = Self::get_precedence(&token.token_type);
            if prec < min_prec || prec == 0 {
                break;
            }
            let operator = self.consume().unwrap().clone();

            let next_min_prec = prec + 1;
            let right = self.parse_binary_expression(next_min_prec)?;

            left = Expression::Binary {
                operator,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Check if the current position starts a closure: `(` followed by `)` `=>` or
    /// `identifier` `:` (parameter list pattern).
    /// Check if the current `{` starts a map literal rather than a block.
    /// Map literal: `{ expr : expr, ... }`
    /// Uses 2-token lookahead: if token after `{` is an expression-start and the
    /// token after that is `:`, it's a map literal.
    fn is_map_literal_start(&self) -> bool {
        // Current token is `{` (offset 0)
        let after_brace = match self.peek_ahead(1) {
            Some(t) => t,
            None => return false,
        };

        // If next is '}', it's an empty block
        if after_brace.token_type == TokenType::CloseBrace {
            return false;
        }

        // If next is a keyword that starts a statement, it's a block
        match &after_brace.token_type {
            TokenType::Let | TokenType::Fn | TokenType::Struct | TokenType::Enum
            | TokenType::Proto | TokenType::Import | TokenType::If | TokenType::While
            | TokenType::Loop | TokenType::Match | TokenType::Try | TokenType::Return
            | TokenType::Throw | TokenType::Break | TokenType::Continue
            | TokenType::Pub | TokenType::Mut => return false,
            _ => {}
        }

        // Check if the token two positions after `{` is `:`
        // This covers simple keys: identifiers, string literals, int literals, bool
        match self.peek_ahead(2) {
            Some(t) => t.token_type == TokenType::Colon,
            None => false,
        }
    }

    fn parse_map_literal(&mut self) -> ParseResult<Expression> {
        let brace = self.peek().unwrap();
        let pos = (brace.line, brace.col);
        self.consume(); // consume '{'

        let mut pairs = Vec::new();

        while !self.peek_match(TokenType::CloseBrace) {
            let key = self.parse_expression()?;
            self.consume_match(TokenType::Colon)?;
            let value = self.parse_expression()?;
            pairs.push((key, value));

            if self.peek_match(TokenType::Comma) {
                self.consume();
            } else if !self.peek_match(TokenType::CloseBrace) {
                return Err(ParseError::UnexpectedToken {
                    found: format!("{:?}", self.peek().map(|t| &t.token_type)),
                    pos: self.peek().map(|t| SourcePos {
                        line: t.line,
                        col: t.col,
                        module: None,
                    }),
                });
            }
        }

        self.consume_match(TokenType::CloseBrace)?;
        Ok(Expression::MapLiteral { pairs, pos })
    }

    fn is_closure_start(&self) -> bool {
        // Must start with '('
        if !self.peek_match(TokenType::OpenParen) {
            return false;
        }
        // Look ahead past the '(' 
        let mut depth = 1;
        let mut offset = 1; // start after '('
        loop {
            match self.peek_ahead(offset) {
                Some(t) => {
                    match t.token_type {
                        TokenType::OpenParen => depth += 1,
                        TokenType::CloseParen => {
                            depth -= 1;
                            if depth == 0 {
                                // Found matching ')'. Check if next token is '=>'
                                if let Some(next) = self.peek_ahead(offset + 1) {
                                    return next.token_type == TokenType::FatRightArrow;
                                }
                                return false;
                            }
                        }
                        _ => {}
                    }
                    offset += 1;
                }
                None => return false,
            }
        }
    }

    fn parse_closure_expression(&mut self) -> ParseResult<Expression> {
        let open_pos = self.consume_expecting(TokenType::OpenParen)?;
        let pos = (open_pos.0, open_pos.1);

        // Parse parameter list
        let mut parameters = Vec::new();
        if !self.peek_match(TokenType::CloseParen) {
            loop {
                let param_name = self.parse_identifier()?;
                self.consume_match(TokenType::Colon)?;
                let param_type = self.parse_type()?;
                parameters.push(Parameter {
                    name: param_name,
                    arg_type: param_type,
                    default_value: None,
                });
                if self.consume_match(TokenType::Comma).is_err() {
                    break;
                }
            }
        }
        self.consume_match(TokenType::CloseParen)?;
        self.consume_match(TokenType::FatRightArrow)?;

        // Parse body expression (can be block or single expr)
        let body = self.parse_expression()?;

        Ok(Expression::Closure {
            parameters,
            body: Box::new(body),
            pos,
        })
    }

    fn parse_if_expression(&mut self) -> ParseResult<Expression> {
        let if_pos = self.consume_expecting(TokenType::If)?;
        let condition = self.parse_expression()?;
        let then_branch = self.parse_expression()?;
        let else_branch = if self.consume_match(TokenType::Else).is_ok() {
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };

        Ok(Expression::If {
            condition: Box::new(condition),
            then_branch: Box::new(then_branch),
            else_branch,
            pos: if_pos,
        })
    }

    fn parse_loop_expression(&mut self) -> ParseResult<Expression> {
        let loop_pos = self.consume_expecting(TokenType::Loop)?;
        let body = self.parse_expression()?;
        Ok(Expression::Loop {
            body: Box::new(body),
            pos: loop_pos,
        })
    }

    fn parse_while_expression(&mut self) -> ParseResult<Expression> {
        let while_pos = self.consume_expecting(TokenType::While)?;
        let condition = self.parse_expression()?;
        let body = self.parse_expression()?;
        Ok(Expression::While {
            condition: Box::new(condition),
            body: Box::new(body),
            pos: while_pos,
        })
    }

    fn parse_break_expression(&mut self) -> ParseResult<Expression> {
        let break_pos = self.consume_expecting(TokenType::Break)?;
        let value = match self.peek() {
            Some(t)
                if matches!(
                    t.token_type,
                    TokenType::Semicolon | TokenType::CloseBrace | TokenType::EOF
                ) =>
            {
                None
            }
            Some(_) => Some(Box::new(self.parse_expression()?)),
            None => None,
        };
        Ok(Expression::Break {
            value,
            pos: break_pos,
        })
    }

    fn parse_continue_expression(&mut self) -> ParseResult<Expression> {
        let continue_pos = self.consume_expecting(TokenType::Continue)?;
        Ok(Expression::Continue { pos: continue_pos })
    }

    fn parse_pattern_suffix(&mut self, mut pattern: Pattern) -> ParseResult<Pattern> {
        loop {
            if self.consume_match(TokenType::Dot).is_ok() {
                let field = self.parse_identifier()?;
                pattern = Pattern::FieldAccess {
                    object: Box::new(pattern),
                    field,
                };
            } else if self.peek_match(TokenType::OpenParen) {
                // Check if this is a destructuring pattern: Ident(...) or Ident.Ident(...)
                match &pattern {
                    Pattern::FieldAccess { object, field } => {
                        // EnumName.Variant(...) pattern
                        if let Pattern::Identifier(enum_name) = object.as_ref() {
                            let enum_name = enum_name.clone();
                            let variant_name = field.clone();
                            self.consume_match(TokenType::OpenParen)?;
                            let sub_patterns = self.parse_sub_patterns()?;
                            self.consume_match(TokenType::CloseParen)?;
                            pattern = Pattern::EnumVariant {
                                enum_name,
                                variant_name,
                                sub_patterns,
                            };
                        } else {
                            break;
                        }
                    }
                    Pattern::Identifier(struct_name) => {
                        // StructName(...) pattern
                        let struct_name = struct_name.clone();
                        self.consume_match(TokenType::OpenParen)?;
                        let field_patterns = self.parse_sub_patterns()?;
                        self.consume_match(TokenType::CloseParen)?;
                        pattern = Pattern::StructPattern {
                            struct_name,
                            field_patterns,
                        };
                    }
                    _ => break,
                }
            } else {
                break;
            }
        }

        Ok(pattern)
    }

    fn parse_sub_patterns(&mut self) -> ParseResult<Vec<Pattern>> {
        let mut patterns = Vec::new();
        if self.peek_match(TokenType::CloseParen) {
            return Ok(patterns);
        }
        loop {
            patterns.push(self.parse_pattern()?);
            if self.consume_match(TokenType::Comma).is_err() {
                break;
            }
            // Allow trailing comma
            if self.peek_match(TokenType::CloseParen) {
                break;
            }
        }
        Ok(patterns)
    }

    fn parse_pattern(&mut self) -> ParseResult<Pattern> {
        if let Some(token) = self.peek().cloned() {
            match &token.token_type {
                TokenType::Integer(value) => {
                    self.consume();
                    Ok(Pattern::IntegerLiteral(*value))
                }
                TokenType::Float(value) => {
                    self.consume();
                    Ok(Pattern::FloatLiteral(*value))
                }
                TokenType::StringLiteral(value) => {
                    self.consume();
                    Ok(Pattern::StringLiteral(value.clone()))
                }
                TokenType::Identifier(_) => {
                    let identifier = self.parse_identifier()?;
                    let pattern = Pattern::Identifier(identifier);
                    let pattern = self.parse_pattern_suffix(pattern)?;
                    Ok(pattern)
                }
                TokenType::OpenParen => {
                    // Tuple pattern: (p1, p2, ...)
                    self.consume(); // consume '('
                    let sub_patterns = self.parse_sub_patterns()?;
                    self.consume_match(TokenType::CloseParen)?;
                    Ok(Pattern::TuplePattern { sub_patterns })
                }
                _ => Err(ParseError::UnexpectedToken {
                    found: format!("{:?}", token.token_type),
                    pos: Some(SourcePos {
                        line: token.line,
                        col: token.col,
                        module: None,
                    }),
                }),
            }
        } else {
            Err(ParseError::UnexpectedEof {
                pos: self.peek_previous().map(|t| SourcePos {
                    line: t.line,
                    col: t.col,
                    module: None,
                }),
            })
        }
    }

    /// Helper method to check if the current position has a named pattern (identifier followed by colon)
    fn has_named_pattern_binding(&mut self) -> bool {
        if let Some(token) = self.peek() {
            if matches!(token.token_type, TokenType::Identifier(_)) {
                // Look ahead to see if there's a colon after the identifier
                let saved_pos = self.position;
                let _ = self.parse_identifier();
                let has_colon = self
                    .peek()
                    .map(|t| t.token_type == TokenType::Colon)
                    .unwrap_or(false);
                self.position = saved_pos; // Restore position
                has_colon
            } else {
                false
            }
        } else {
            false
        }
    }

    fn parse_named_pattern(&mut self) -> ParseResult<NamedPattern> {
        // Try to parse as "name: pattern" first
        let name = if self.has_named_pattern_binding() {
            let ident = self.parse_identifier()?;
            self.consume_match(TokenType::Colon)?;
            Some(ident)
        } else {
            None
        };

        let pattern = self.parse_pattern()?;
        Ok(NamedPattern { name, pattern })
    }

    fn parse_try_expression(&mut self) -> ParseResult<Expression> {
        self.consume_match(TokenType::Try)?;
        let try_block = self.parse_expression()?;
        let else_arms = if self.consume_match(TokenType::Else).is_ok() {
            if self.consume_match(TokenType::OpenBrace).is_ok() {
                let mut arms = vec![];
                loop {
                    // Check if we've reached the end
                    if self.consume_match(TokenType::CloseBrace).is_ok() {
                        break;
                    }

                    let pattern = self.parse_named_pattern()?;
                    self.consume_match(TokenType::FatRightArrow)?;
                    let expression = self.parse_expression()?;

                    arms.push(crate::ast::MatchArm {
                        pattern,
                        expression,
                    });

                    let ends_with_brace = self.ends_with_brace();
                    let comma = self.consume_match(TokenType::Comma);
                    if !ends_with_brace {
                        comma?;
                    }

                    if self.consume_match(TokenType::CloseBrace).is_ok() {
                        break;
                    }
                }
                arms
            } else {
                // Single expression else: `try expr else fallback`
                // Treat as a wildcard arm that matches anything
                let pattern = NamedPattern {
                    name: None,
                    pattern: Pattern::Identifier(Identifier {
                        name: "_".to_string(),
                        line: 0,
                        col: 0,
                    }),
                };
                let expression = self.parse_expression()?;
                vec![crate::ast::MatchArm {
                    pattern,
                    expression,
                }]
            }
        } else {
            vec![]
        };

        Ok(Expression::Try {
            try_block: Box::new(try_block),
            else_arms,
        })
    }

    fn parse_match_expression(&mut self) -> ParseResult<Expression> {
        self.consume_match(TokenType::Match)?;
        let scrutinee = self.parse_expression()?;
        self.consume_match(TokenType::OpenBrace)?;

        let mut arms = vec![];
        loop {
            // Check if we've reached the end of the match expression
            if self.consume_match(TokenType::CloseBrace).is_ok() {
                break;
            }

            // Parse pattern => expression
            let pattern = self.parse_named_pattern()?;
            self.consume_match(TokenType::FatRightArrow)?;
            let expression = self.parse_expression()?;

            arms.push(crate::ast::MatchArm {
                pattern,
                expression,
            });

            // Handle optional comma
            let ends_with_brace = self.ends_with_brace();
            let comma = self.consume_match(TokenType::Comma);
            if !ends_with_brace {
                comma?;
            }

            // Check for closing brace again
            if self.consume_match(TokenType::CloseBrace).is_ok() {
                break;
            }
        }

        Ok(Expression::Match {
            scrutinee: Box::new(scrutinee),
            arms,
        })
    }

    fn parse_block(&mut self) -> ParseResult<Vec<Statement>> {
        self.consume_match(TokenType::OpenBrace)?;
        let mut statements = Vec::new();
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseBrace {
                self.consume();
                break;
            }

            statements.push(self.parse_statement()?);
        }

        return Ok(statements);
    }

    fn parse_argument_list(&mut self) -> ParseResult<Vec<Parameter>> {
        let mut parameters = Vec::new();
        self.consume_match(TokenType::OpenParen)?;
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseParen {
                self.consume();
                break;
            }

            let parameter = match_token! {
                self.peek(),
                TokenType::Ref => {
                    // Receiver shortcut: `ref self` == `self: ref Self`
                    // or `ref mut self` (ephemeral mutable borrow receiver)
                    self.consume(); // consume 'ref'

                    // Check for 'ref mut self'
                    let is_mut = matches!(self.peek().map(|t| &t.token_type), Some(TokenType::Mut));
                    if is_mut {
                        self.consume(); // consume 'mut'
                    }

                    let name = self.parse_identifier()?;
                    if name.name != "self" {
                        return Err(ParseError::UnexpectedToken {
                            found: format!("{:?}", TokenType::Identifier(name.name)),
                            pos: Some(SourcePos { line: name.line, col: name.col, module: None }),
                        });
                    }

                    let self_type = Type::Identifier(Identifier {
                        name: "Self".to_string(),
                        line: name.line,
                        col: name.col,
                    });

                    let arg_type = if is_mut {
                        Type::MutableReference(Box::new(self_type))
                    } else {
                        Type::Reference(Box::new(self_type))
                    };

                    Ok(Parameter { name, arg_type, default_value: None })
                },
                TokenType::Box => {
                    // Receiver shortcut: `box self` == `self: box<Self>`
                    self.consume();
                    let name = self.parse_identifier()?;
                    if name.name != "self" {
                        return Err(ParseError::UnexpectedToken {
                            found: format!("{:?}", TokenType::Identifier(name.name)),
                            pos: Some(SourcePos { line: name.line, col: name.col, module: None }),
                        });
                    }

                    let arg_type = Type::Generic {
                        base: Identifier {
                            name: "box".to_string(),
                            line: name.line,
                            col: name.col,
                        },
                        type_args: vec![Type::Identifier(Identifier {
                            name: "Self".to_string(),
                            line: name.line,
                            col: name.col,
                        })],
                    };

                    Ok(Parameter { name, arg_type, default_value: None })
                },
                TokenType::Identifier(ref ident) if ident == "self" => {
                    // `self` receiver; optional explicit type via `self: ...`.
                    let (line, col) = match self.consume() {
                        Some(t) => (t.line, t.col),
                        None => return Err(ParseError::UnexpectedEof { pos: self.peek_previous().map(|t| SourcePos { line: t.line, col: t.col, module: None }) }),
                    };

                    let name = Identifier { name: "self".to_string(), line, col };

                    let arg_type = if self.consume_match(TokenType::Colon).is_ok() {
                        self.parse_type()?
                    } else {
                        Type::Identifier(Identifier { name: "Self".to_string(), line, col })
                    };

                    Ok(Parameter { name, arg_type, default_value: None })
                },
                TokenType::Identifier(_) => {
                    let name = self.parse_identifier()?;
                    self.consume_match(TokenType::Colon)?;

                    let arg_type = self.parse_type()?;
                    // Optional default value
                    let default_value = if self.consume_match(TokenType::Assignment).is_ok() {
                        Some(self.parse_expression()?)
                    } else {
                        None
                    };
                    Ok(Parameter { name, arg_type, default_value })
                },
            };

            parameters.push(parameter?);

            let _ = self.consume_match(TokenType::Comma);
        }

        Ok(parameters)
    }

    fn parse_attribute(&mut self) -> ParseResult<Attribute> {
        // Expect @ token
        self.consume_match(TokenType::At)?;

        // Parse attribute name (must be an identifier)
        let name = self.parse_identifier()?;

        // Parse arguments (same as struct construction / function call)
        self.consume_match(TokenType::OpenParen)?;
        let mut arguments = Vec::new();

        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseParen {
                self.consume();
                break;
            }

            // Parse expression, which might be "name = value" or just "value"
            let expr = self.parse_expression()?;

            // Check if the expression is a named argument (name = value)
            let arg = if let Expression::Binary {
                left,
                operator:
                    Token {
                        token_type: TokenType::Assignment,
                        ..
                    },
                right,
            } = &expr
                && let Expression::Identifier(ident) = left.as_ref()
            {
                (Some(ident.clone()), right.deref().clone())
            } else {
                (None, expr)
            };

            arguments.push(arg);

            // Handle comma separator
            let comma = self.consume_match(TokenType::Comma);
            if !self.peek_match(TokenType::CloseParen) {
                comma?;
            }
        }

        Ok(Attribute { name, arguments })
    }

    fn parse_attributes(&mut self) -> ParseResult<Vec<Attribute>> {
        let mut attributes = Vec::new();

        while self.peek_match(TokenType::At) {
            attributes.push(self.parse_attribute()?);
        }

        Ok(attributes)
    }

    fn parse_enum_declaration(
        &mut self,
        attributes: Vec<Attribute>,
        is_pub: bool,
    ) -> ParseResult<Statement> {
        self.consume_match(TokenType::Enum)?;

        // Parse optional conformance list: enum[Proto1, Proto2]
        let conformance = if self.peek_match(TokenType::OpenBracket) {
            self.consume();
            let mut protos = vec![];

            // Parse first protocol
            protos.push(self.parse_identifier()?);

            // Parse additional protocols separated by commas
            while self.peek_match(TokenType::Comma) {
                self.consume();
                protos.push(self.parse_identifier()?);
            }

            self.consume_match(TokenType::CloseBracket)?;
            protos
        } else {
            vec![]
        };

        let name = self.parse_identifier()?;
        let type_parameters = self.parse_type_parameters()?;

        // New syntax: enum Name( ... ) or enum Name( ... );
        self.consume_match(TokenType::OpenParen)?;

        let mut values = Vec::new();
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseParen {
                self.consume();
                break;
            }

            let variant_name = self.parse_identifier()?;

            // Check if this is a payload variant (has colon)
            let fields = if self.peek_match(TokenType::Colon) {
                self.consume_match(TokenType::Colon)?;

                // Check if we have a tuple type (multi-field payload)
                if self.peek_match(TokenType::OpenParen) {
                    self.consume_match(TokenType::OpenParen)?;
                    let mut field_types = Vec::new();

                    // Parse comma-separated list of types in the tuple
                    while !self.peek_match(TokenType::CloseParen) {
                        field_types.push(self.parse_type()?);

                        if !self.peek_match(TokenType::CloseParen) {
                            self.consume_match(TokenType::Comma)?;
                        }
                    }

                    self.consume_match(TokenType::CloseParen)?;
                    field_types
                } else {
                    // Single-field payload
                    vec![self.parse_type()?]
                }
            } else {
                // Unit variant - no fields
                Vec::new()
            };

            values.push(EnumVariant {
                name: variant_name.name,
                fields,
            });

            let comma = self.consume_match(TokenType::Comma);
            if !self.peek_match(TokenType::CloseParen) {
                comma?;
            }
        }

        // Check if there's a method block or just a semicolon
        let methods = if self.peek_match(TokenType::OpenBrace) {
            self.consume_match(TokenType::OpenBrace)?;
            let mut methods = Vec::new();

            while !self.peek_match(TokenType::CloseBrace) {
                // Parse attributes for the method
                let attributes = self.parse_attributes()?;
                let is_pub = self.consume_match(TokenType::Pub).is_ok();
                let function = self.parse_function_declaration(attributes, is_pub)?;
                methods.push(StructMethod { is_pub, function });
            }

            self.consume_match(TokenType::CloseBrace)?;
            methods
        } else {
            // No method block, expect semicolon
            self.consume_match(TokenType::Semicolon)?;
            Vec::new()
        };

        Ok(Statement::EnumDeclaration {
            is_pub,
            name,
            attributes,
            conformance,
            type_parameters,
            values,
            methods,
        })
    }

    fn parse_struct_field(&mut self) -> ParseResult<StructField> {
        let is_pub = self.consume_match(TokenType::Pub).is_ok();

        // Try to parse as named field first (identifier followed by colon)
        // We need to check if the next two tokens are identifier and colon
        let is_named = if let Some(token) = self.peek() {
            if matches!(token.token_type, TokenType::Identifier(_)) {
                // Look ahead to see if there's a colon
                if let Some(next_token) = self.peek_ahead(1) {
                    next_token.token_type == TokenType::Colon
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        if is_named {
            // Parse named field: name: type [= default]
            let field_name = self.parse_identifier()?;
            self.consume_match(TokenType::Colon)?;
            let field_type = self.parse_type()?;
            let default_value = if self.consume_match(TokenType::Assignment).is_ok() {
                Some(self.parse_expression()?)
            } else {
                None
            };

            Ok(StructField {
                is_pub,
                name: Some(field_name),
                field_type,
                default_value,
            })
        } else {
            // Parse unnamed field (tuple struct): type
            let field_type = self.parse_type()?;

            Ok(StructField {
                is_pub,
                name: None,
                field_type,
                default_value: None,
            })
        }
    }

    fn parse_comma_separated_list<T, F>(&mut self, parse_item: F) -> ParseResult<Vec<T>>
    where
        F: Fn(&mut Self) -> ParseResult<T>,
    {
        self.consume_match(TokenType::OpenParen)?;
        let mut items = Vec::new();

        while let Some(token) = self.peek().cloned() {
            if !items.is_empty() && token.token_type == TokenType::Comma {
                self.consume();
            }

            if let Some(TokenType::CloseParen) = self.peek().map(|t| &t.token_type) {
                self.consume();
                break;
            }

            items.push(parse_item(self)?);
        }

        Ok(items)
    }

    fn parse_struct_method(&mut self) -> ParseResult<StructMethod> {
        // Parse any attributes before the method
        let attributes = self.parse_attributes()?;
        let is_pub = self.consume_match(TokenType::Pub).is_ok();
        let function = self.parse_function_declaration(attributes, is_pub)?;

        Ok(StructMethod { is_pub, function })
    }

    fn parse_struct_field_list(&mut self) -> ParseResult<Vec<StructField>> {
        self.parse_comma_separated_list(|s| s.parse_struct_field())
    }

    fn parse_struct_method_list(&mut self) -> ParseResult<Vec<StructMethod>> {
        self.consume_match(TokenType::OpenBrace)?;

        let mut methods = Vec::new();
        while let Some(token) = self.peek() {
            if token.token_type == TokenType::CloseBrace {
                self.consume();
                break;
            }

            methods.push(self.parse_struct_method()?);
        }

        Ok(methods)
    }

    fn parse_struct_declaration(
        &mut self,
        attributes: Vec<Attribute>,
        is_pub: bool,
    ) -> ParseResult<Statement> {
        self.consume_match(TokenType::Struct)?;

        // Parse optional conformance list: struct[Proto1, Proto2]
        let conformance = if self.peek_match(TokenType::OpenBracket) {
            self.consume();
            let mut protos = vec![];

            // Parse first protocol
            protos.push(self.parse_identifier()?);

            // Parse additional protocols separated by commas
            while self.peek_match(TokenType::Comma) {
                self.consume();
                protos.push(self.parse_identifier()?);
            }

            self.consume_match(TokenType::CloseBracket)?;
            protos
        } else {
            vec![]
        };

        let name = self.parse_identifier()?;
        let type_parameters = self.parse_type_parameters()?;

        let fields = if self.peek_match(TokenType::OpenParen) {
            self.parse_struct_field_list()?
        } else {
            vec![]
        };

        match_token! {
            self.peek(),
            TokenType::Semicolon => {
                self.consume();
                return Ok(Statement::StructDeclaration { is_pub, name, attributes, conformance, type_parameters, fields, methods: vec![] });
            },
            TokenType::OpenBrace => {
                let methods = self.parse_struct_method_list()?;
                return Ok(Statement::StructDeclaration { is_pub, name, attributes, conformance, type_parameters, fields, methods });
            },
        }
    }

    fn parse_proto_method(&mut self) -> ParseResult<ProtoMethod> {
        self.consume_match(TokenType::Fn)?;
        let name = self.parse_identifier()?;
        let parameters = self.parse_argument_list()?;
        let return_type = if self.consume_match(TokenType::RightArrow).is_ok() {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.consume_match(TokenType::Semicolon)?;

        Ok(ProtoMethod {
            name,
            parameters,
            return_type,
        })
    }

    fn parse_proto_method_list(&mut self) -> ParseResult<Vec<ProtoMethod>> {
        self.consume_match(TokenType::OpenBrace)?;
        let mut methods = vec![];

        while !self.peek_match(TokenType::CloseBrace) && !self.peek_match(TokenType::EOF) {
            methods.push(self.parse_proto_method()?);
        }

        self.consume_match(TokenType::CloseBrace)?;
        Ok(methods)
    }

    fn parse_proto_declaration(
        &mut self,
        attributes: Vec<Attribute>,
        is_pub: bool,
    ) -> ParseResult<Statement> {
        self.consume_match(TokenType::Proto)?;
        let name = self.parse_identifier()?;

        let methods = if self.peek_match(TokenType::OpenBrace) {
            self.parse_proto_method_list()?
        } else {
            self.consume_match(TokenType::Semicolon)?;
            vec![]
        };

        Ok(Statement::ProtoDeclaration {
            is_pub,
            name,
            attributes,
            methods,
        })
    }

    fn parse_function_declaration(
        &mut self,
        attributes: Vec<Attribute>,
        is_pub: bool,
    ) -> ParseResult<FunctionDeclaration> {
        self.consume_match(TokenType::Fn)?;

        // Parse effect qualifiers in square brackets after fn: fn[effect1, effect2, ...]
        let effects = if self.peek_match(TokenType::OpenBracket) {
            self.consume(); // consume '['
            let mut effects = vec![];

            while !self.peek_match(TokenType::CloseBracket) && !self.peek_match(TokenType::EOF) {
                // Parse effect identifier (e.g., "throws", "suspend")
                let effect_name = self.parse_identifier()?;

                // Check for parameterized effect: throws<ErrorType>
                if self.peek_match(TokenType::LessThan) {
                    self.consume(); // consume '<'
                    let type_param = self.parse_type()?;
                    self.consume_match(TokenType::GreaterThan)?;

                    effects.push(Effect::Parameterized {
                        name: effect_name,
                        type_param,
                    });
                } else {
                    effects.push(Effect::Simple(effect_name));
                }

                // Consume comma if present (for multiple effects)
                if self.peek_match(TokenType::Comma) {
                    self.consume();
                }
            }

            self.consume_match(TokenType::CloseBracket)?;
            effects
        } else {
            vec![]
        };

        let name = self.parse_identifier()?;
        let type_parameters = self.parse_type_parameters()?;
        let parameters = self.parse_argument_list()?;
        let return_type = if self.consume_match(TokenType::RightArrow).is_ok() {
            Some(self.parse_type()?)
        } else {
            None
        };

        // Check if this function has an @intrinsic attribute
        let has_intrinsic = attributes.iter().any(|attr| attr.name.name == "intrinsic");

        // For intrinsic functions, allow semicolon instead of body
        let body = if has_intrinsic && self.peek_match(TokenType::Semicolon) {
            self.consume(); // consume ';'
            vec![] // Empty body for intrinsic functions
        } else {
            self.parse_block()?
        };

        Ok(FunctionDeclaration {
            is_pub,
            name,
            attributes,
            effects,
            type_parameters,
            parameters,
            body,
            return_type,
        })
    }

    fn parse_type(&mut self) -> ParseResult<Type> {
        if let Some(token) = self.peek() {
            match &token.token_type {
                TokenType::Question => {
                    self.consume();
                    // ?Type syntax
                    let ty = self.parse_type()?;
                    Ok(Type::Nullable(Box::new(ty)))
                }
                TokenType::Ref => {
                    self.consume();
                    // ref<Type> syntax - must have angle brackets
                    self.consume_match(TokenType::LessThan)?;
                    let ty = self.parse_type()?;
                    self.consume_match(TokenType::GreaterThan)?;
                    Ok(Type::Reference(Box::new(ty)))
                }
                TokenType::Box => {
                    let (line, col) = (token.line, token.col);
                    self.consume();
                    // box<Type> syntax - must have angle brackets
                    self.consume_match(TokenType::LessThan)?;
                    let ty = self.parse_type()?;
                    self.consume_match(TokenType::GreaterThan)?;
                    Ok(Type::Generic {
                        base: Identifier {
                            name: "box".to_string(),
                            line,
                            col,
                        },
                        type_args: vec![ty],
                    })
                }
                TokenType::OpenBracket => {
                    self.consume();
                    let ty = self.parse_type()?;
                    self.consume_match(TokenType::CloseBracket)?;
                    Ok(Type::Array(Box::new(ty)))
                }
                TokenType::OpenParen => {
                    // Tuple type: (T1, T2, ...) or parenthesized type: (T)
                    self.consume(); // consume '('
                    let first_ty = self.parse_type()?;
                    if self.peek_match(TokenType::Comma) {
                        // Tuple type: (T1, T2, ...)
                        self.consume(); // consume ','
                        let mut types = vec![first_ty];
                        while !self.peek_match(TokenType::CloseParen) {
                            types.push(self.parse_type()?);
                            if self.peek_match(TokenType::Comma) {
                                self.consume();
                            } else if !self.peek_match(TokenType::CloseParen) {
                                return Err(ParseError::UnexpectedToken {
                                    found: format!("{:?}", self.peek().map(|t| &t.token_type)),
                                    pos: self.peek().map(|t| SourcePos {
                                        line: t.line,
                                        col: t.col,
                                        module: None,
                                    }),
                                });
                            }
                        }
                        self.consume_match(TokenType::CloseParen)?;
                        Ok(Type::Tuple(types))
                    } else {
                        // Parenthesized type (grouping): (T)
                        self.consume_match(TokenType::CloseParen)?;
                        Ok(first_ty)
                    }
                }
                TokenType::Fn => {
                    // fn(T1, T2) -> R function type
                    self.consume(); // consume 'fn'
                    self.consume_match(TokenType::OpenParen)?;
                    let mut param_types = Vec::new();
                    if !self.peek_match(TokenType::CloseParen) {
                        param_types.push(self.parse_type()?);
                        while self.consume_match(TokenType::Comma).is_ok() {
                            param_types.push(self.parse_type()?);
                        }
                    }
                    self.consume_match(TokenType::CloseParen)?;
                    let return_type = if self.consume_match(TokenType::RightArrow).is_ok() {
                        self.parse_type()?
                    } else {
                        Type::Identifier(Identifier {
                            name: "unit".to_string(),
                            line: 0,
                            col: 0,
                        })
                    };
                    Ok(Type::Function {
                        param_types,
                        return_type: Box::new(return_type),
                    })
                }
                TokenType::Identifier(_) => {
                    let mut ident = self.parse_identifier()?;

                    // Support module-qualified types like `ui.Message`
                    if self.peek_match(TokenType::Dot) {
                        let mut full_name = ident.name.clone();
                        let line = ident.line;
                        let col = ident.col;

                        while self.peek_match(TokenType::Dot) {
                            self.consume(); // consume '.'
                            let next = self.parse_identifier()?;
                            full_name.push('.');
                            full_name.push_str(&next.name);
                        }

                        ident = Identifier {
                            name: full_name,
                            line,
                            col,
                        };
                    }

                    // Check for generic type syntax: identifier<type_args>
                    if self.peek_match(TokenType::LessThan) {
                        self.consume(); // consume '<'

                        // Handle empty type argument list: identifier<>
                        if self.peek_match(TokenType::GreaterThan) {
                            self.consume(); // consume '>'
                            return Err(ParseError::UnexpectedToken {
                                found: "empty type argument list".to_string(),
                                pos: Some(SourcePos {
                                    line: ident.line,
                                    col: ident.col,
                                    module: None,
                                }),
                            });
                        }

                        let mut type_args = vec![];

                        loop {
                            type_args.push(self.parse_type()?);

                            if self.peek_match(TokenType::Comma) {
                                self.consume(); // consume ','
                            } else {
                                break;
                            }
                        }

                        self.consume_match(TokenType::GreaterThan)?; // consume '>'

                        Ok(Type::Generic {
                            base: ident,
                            type_args,
                        })
                    } else {
                        Ok(Type::Identifier(ident))
                    }
                }
                _ => Err(ParseError::UnexpectedToken {
                    found: format!("{:?}", token.token_type),
                    pos: Some(SourcePos {
                        line: token.line,
                        col: token.col,
                        module: None,
                    }),
                }),
            }
        } else {
            Err(ParseError::UnexpectedEof {
                pos: self.peek_previous().map(|t| SourcePos {
                    line: t.line,
                    col: t.col,
                    module: None,
                }),
            })
        }
    }

    /// Try to parse type arguments in expression context (e.g., Container<int>)
    /// Returns Ok with type args if successful, Err otherwise.
    ///
    /// This is used to disambiguate between generic instantiation (`Container<int>`)
    /// and comparison operators (`a < b`). The parser saves its position and attempts
    /// to parse type arguments. If successful, it's a generic instantiation; if it fails,
    /// the parser backtracks to the saved position and treats `<` as a comparison operator.
    fn try_parse_type_arguments(&mut self) -> ParseResult<Vec<Type>> {
        // Save parser state for potential backtracking
        let saved_pos = self.position;

        // Consume the '<'
        if !self.peek_match(TokenType::LessThan) {
            return Err(ParseError::UnexpectedToken {
                found: "not <".to_string(),
                pos: None,
            });
        }
        self.consume();

        // Check for empty type argument list: identifier<>
        // This is not allowed (consistent with parse_type behavior)
        if self.peek_match(TokenType::GreaterThan) {
            self.position = saved_pos;
            return Err(ParseError::UnexpectedToken {
                found: "empty type argument list".to_string(),
                pos: None,
            });
        }

        // Try to parse type arguments
        let mut type_args = vec![];

        loop {
            // Try to parse a type
            match self.parse_type() {
                Ok(ty) => type_args.push(ty),
                Err(_) => {
                    // Failed to parse type - this might be a comparison operator
                    // Restore position and fail gracefully
                    self.position = saved_pos;
                    return Err(ParseError::UnexpectedToken {
                        found: "not a type argument".to_string(),
                        pos: None,
                    });
                }
            }

            if self.peek_match(TokenType::Comma) {
                self.consume();
            } else {
                break;
            }
        }

        // Must end with '>'
        if !self.peek_match(TokenType::GreaterThan) {
            self.position = saved_pos;
            return Err(ParseError::UnexpectedToken {
                found: "expected '>' to close type arguments".to_string(),
                pos: None,
            });
        }
        self.consume();

        Ok(type_args)
    }

    fn parse_type_parameters(&mut self) -> ParseResult<Vec<TypeParameter>> {
        if !self.peek_match(TokenType::LessThan) {
            return Ok(vec![]);
        }

        self.consume(); // consume '<'

        // Handle empty type parameter list: <>
        if self.peek_match(TokenType::GreaterThan) {
            self.consume(); // consume '>'
            return Ok(vec![]);
        }

        let mut type_params = vec![];

        loop {
            let name = self.parse_identifier()?;

            // Check for bounds: T: Proto1 + Proto2 + ...
            let bounds = if self.peek_match(TokenType::Colon) {
                self.consume(); // consume ':'
                let mut bounds = vec![self.parse_identifier()?];
                while self.peek_match(TokenType::Plus) {
                    self.consume(); // consume '+'
                    let bound = self.parse_identifier()?;
                    // Check for duplicate bounds (spec §20.5: listing same proto twice is ERROR)
                    if bounds.iter().any(|b| b.name == bound.name) {
                        return Err(ParseError::UnexpectedToken {
                            found: format!("duplicate bound '{}' on type parameter '{}'", bound.name, name.name),
                            pos: Some(SourcePos { line: bound.line, col: bound.col, module: None }),
                        });
                    }
                    bounds.push(bound);
                }
                bounds
            } else {
                vec![]
            };

            type_params.push(TypeParameter { name, bounds });

            if self.peek_match(TokenType::Comma) {
                self.consume(); // consume ','
            } else {
                break;
            }
        }

        self.consume_match(TokenType::GreaterThan)?; // consume '>'
        Ok(type_params)
    }

    fn parse_import_statement(&mut self) -> ParseResult<Statement> {
        self.consume_match(TokenType::Import)?;

        // Parse the module path (can be dotted identifier or relative path starting with .)
        let mut module_path = String::new();

        // Check if it's a relative import starting with .
        if self.peek_match(TokenType::Dot) {
            module_path.push('.');
            self.consume();
        }

        // Parse the rest of the path
        loop {
            match self.peek() {
                Some(Token {
                    token_type: TokenType::Identifier(id),
                    ..
                }) => {
                    module_path.push_str(id);
                    self.consume();

                    // Check for another dot
                    if self.peek_match(TokenType::Dot) {
                        module_path.push('.');
                        self.consume();
                    } else {
                        break;
                    }
                }
                _ => {
                    // If we haven't parsed any identifiers or if the path ends with a dot, it's an error
                    if module_path.ends_with('.') {
                        return Err(ParseError::UnexpectedToken {
                            found: format!("{:?}", self.peek().map(|t| &t.token_type)),
                            pos: self.peek().map(|t| SourcePos {
                                line: t.line,
                                col: t.col,
                                module: None,
                            }),
                        });
                    }
                    break;
                }
            }
        }

        // Check for optional "as" alias
        let alias = if self.peek_match(TokenType::As) {
            self.consume();
            Some(self.parse_identifier()?.name)
        } else {
            None
        };

        self.consume_match(TokenType::Semicolon)?;

        Ok(Statement::Import { module_path, alias })
    }

    fn parse_statement(&mut self) -> ParseResult<Statement> {
        // Parse attributes first (they come before pub in the syntax)
        let attributes = self.parse_attributes()?;

        // Then, check for pub keyword
        let is_pub = if self.peek_match(TokenType::Pub) {
            self.consume();
            true
        } else {
            false
        };

        match self.peek().map(|t| t.token_type.clone()) {
            Some(TokenType::Import) => {
                if is_pub {
                    return Err(ParseError::UnexpectedToken {
                        found: "pub keyword on import statement".to_string(),
                        pos: self.peek().map(|t| SourcePos {
                            line: t.line,
                            col: t.col,
                            module: None,
                        }),
                    });
                }
                if !attributes.is_empty() {
                    return Err(ParseError::UnexpectedToken {
                        found: "attributes on import statement".to_string(),
                        pos: Some(SourcePos {
                            line: attributes[0].name.line,
                            col: attributes[0].name.col,
                            module: None,
                        }),
                    });
                }
                self.parse_import_statement()
            }
            Some(TokenType::Fn) => self
                .parse_function_declaration(attributes, is_pub)
                .map(Statement::FunctionDeclaration),
            Some(TokenType::Enum) => self.parse_enum_declaration(attributes, is_pub),
            Some(TokenType::Struct) => self.parse_struct_declaration(attributes, is_pub),
            Some(TokenType::Proto) => self.parse_proto_declaration(attributes, is_pub),
            // Some(TokenType::If) => self.parse_if_expression().map(Statement::Expression),
            Some(TokenType::Return) => {
                if is_pub {
                    return Err(ParseError::UnexpectedToken {
                        found: "pub keyword on return statement".to_string(),
                        pos: self.peek().map(|t| SourcePos {
                            line: t.line,
                            col: t.col,
                            module: None,
                        }),
                    });
                }
                if !attributes.is_empty() {
                    return Err(ParseError::UnexpectedToken {
                        found: "attributes on return statement".to_string(),
                        pos: Some(SourcePos {
                            line: attributes[0].name.line,
                            col: attributes[0].name.col,
                            module: None,
                        }),
                    });
                }
                self.consume();
                // Support bare `return;` (unit return) by checking for semicolon
                if self.peek().map(|t| &t.token_type) == Some(&TokenType::Semicolon) {
                    self.consume(); // consume the semicolon
                    Ok(Statement::Return(Box::new(Expression::IntegerLiteral(0))))
                } else {
                    let expr = self.parse_expression()?;
                    self.consume_match(TokenType::Semicolon)?;
                    Ok(Statement::Return(Box::new(expr)))
                }
            }
            Some(TokenType::Throw) => {
                if is_pub {
                    return Err(ParseError::UnexpectedToken {
                        found: "pub keyword on throw statement".to_string(),
                        pos: self.peek().map(|t| SourcePos {
                            line: t.line,
                            col: t.col,
                            module: None,
                        }),
                    });
                }
                if !attributes.is_empty() {
                    return Err(ParseError::UnexpectedToken {
                        found: "attributes on throw statement".to_string(),
                        pos: Some(SourcePos {
                            line: attributes[0].name.line,
                            col: attributes[0].name.col,
                            module: None,
                        }),
                    });
                }
                self.consume();
                let expr = self.parse_expression()?;
                self.consume_match(TokenType::Semicolon)?;
                Ok(Statement::Throw(expr))
            }
            Some(TokenType::Let) => {
                if is_pub {
                    return Err(ParseError::UnexpectedToken {
                        found: "pub keyword on let statement".to_string(),
                        pos: self.peek().map(|t| SourcePos {
                            line: t.line,
                            col: t.col,
                            module: None,
                        }),
                    });
                }
                if !attributes.is_empty() {
                    return Err(ParseError::UnexpectedToken {
                        found: "attributes on let statement".to_string(),
                        pos: Some(SourcePos {
                            line: attributes[0].name.line,
                            col: attributes[0].name.col,
                            module: None,
                        }),
                    });
                }

                self.consume(); // consume 'let'
                let is_mutable = matches!(self.peek().map(|t| &t.token_type), Some(TokenType::Mut));
                if is_mutable {
                    self.consume(); // consume 'mut'
                }

                // Check for tuple destructuring: let (a, b) = ... or var (x, y) = ...
                if self.peek_match(TokenType::OpenParen) {
                    self.consume_match(TokenType::OpenParen)?;
                    let sub_patterns = self.parse_sub_patterns()?;
                    self.consume_match(TokenType::CloseParen)?;
                    self.consume_match(TokenType::Assignment)?;
                    let expression = self.parse_expression()?;
                    self.consume_match(TokenType::Semicolon)?;
                    return Ok(Statement::LetDestructure {
                        is_mutable,
                        pattern: Pattern::TuplePattern { sub_patterns },
                        expression,
                    });
                }

                let identifier = self.parse_identifier()?;

                // Check for destructuring pattern: let Pos(r, c) = ...
                if self.peek_match(TokenType::OpenParen) {
                    let struct_name = identifier;
                    self.consume_match(TokenType::OpenParen)?;
                    let field_patterns = self.parse_sub_patterns()?;
                    self.consume_match(TokenType::CloseParen)?;
                    self.consume_match(TokenType::Assignment)?;
                    let expression = self.parse_expression()?;
                    self.consume_match(TokenType::Semicolon)?;
                    return Ok(Statement::LetDestructure {
                        is_mutable,
                        pattern: Pattern::StructPattern {
                            struct_name,
                            field_patterns,
                        },
                        expression,
                    });
                }

                // Check for enum destructuring: let Result.Ok(n) = ...
                if self.peek_match(TokenType::Dot) {
                    let saved_pos = self.position;
                    self.consume(); // consume '.'
                    if let Ok(variant_name) = self.parse_identifier() {
                        if self.peek_match(TokenType::OpenParen) {
                            let enum_name = identifier;
                            self.consume_match(TokenType::OpenParen)?;
                            let sub_patterns = self.parse_sub_patterns()?;
                            self.consume_match(TokenType::CloseParen)?;
                            self.consume_match(TokenType::Assignment)?;
                            let expression = self.parse_expression()?;
                            self.consume_match(TokenType::Semicolon)?;
                            return Ok(Statement::LetDestructure {
                                is_mutable,
                                pattern: Pattern::EnumVariant {
                                    enum_name,
                                    variant_name,
                                    sub_patterns,
                                },
                                expression,
                            });
                        }
                    }
                    self.position = saved_pos;
                }

                // Check for optional type annotation: let p: Type = ...
                let type_annotation = if self.peek_match(TokenType::Colon) {
                    self.consume(); // consume ':'
                    Some(self.parse_type()?)
                } else {
                    None
                };

                self.consume_match(TokenType::Assignment)?;
                let expression = self.parse_expression()?;
                self.consume_match(TokenType::Semicolon)?;

                Ok(Statement::Let {
                    is_mutable,
                    identifier,
                    type_annotation,
                    expression,
                })
            }
            _ => {
                if is_pub {
                    return Err(ParseError::UnexpectedToken {
                        found: "pub keyword on expression statement".to_string(),
                        pos: self.peek().map(|t| SourcePos {
                            line: t.line,
                            col: t.col,
                            module: None,
                        }),
                    });
                }
                if !attributes.is_empty() {
                    return Err(ParseError::UnexpectedToken {
                        found: "attributes on expression statement".to_string(),
                        pos: Some(SourcePos {
                            line: attributes[0].name.line,
                            col: attributes[0].name.col,
                            module: None,
                        }),
                    });
                }
                let expression = self.parse_expression()?;
                let ends_with_brace = self.ends_with_brace();

                match_token! {
                    self.peek(),
                    TokenType::Semicolon => {
                        self.consume();
                        Ok(Statement::Expression(expression))
                    }
                    TokenType::CloseBrace => Ok(Statement::Expression(
                        Expression::BlockValue(Box::new(expression)),
                    )),
                    _ if ends_with_brace => {
                        Ok(Statement::Expression(expression))
                    },
                }
            }
        }
    }

    pub fn parse(&mut self) -> ParseResult<Vec<Statement>> {
        let mut statements = Vec::new();

        while self.peek().is_some() {
            statements.push(self.parse_statement()?);
        }

        Ok(statements)
    }
}
