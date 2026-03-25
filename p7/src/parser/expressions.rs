use std::ops::Deref;

use crate::ast::{
    Expression, FunctionCall, Identifier, InterpolatedStringPart, Parameter,
};
use crate::errors::{ParseError, SourcePos};
use crate::lexer::{InterpolatedStringPart as LexInterpolatedStringPart, Lexer, Token, TokenType};

use super::{ParseResult, Parser, UNARY_OPERATIONS};

impl Parser {
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
            if let Expression::Identifier(ref ident) = expression
                && self.peek_match(TokenType::LessThan) {
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
        if let Some(token) = self.peek()
            && UNARY_OPERATIONS.contains(&token.token_type) {
                let operator = self.consume().unwrap().clone();
                let right = self.parse_unary_expression()?;
                return Ok(Expression::Unary {
                    operator,
                    right: Box::new(right),
                });
            }

        self.parse_primary_expression()
    }

    pub(crate) fn parse_expression(&mut self) -> ParseResult<Expression> {
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
}
