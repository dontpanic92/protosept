#[derive(Debug, PartialEq, Clone)]
pub struct Token {
    pub token_type: TokenType,
    pub line: usize,
    pub col: usize,
    pub length: usize,
}

#[derive(Debug, PartialEq, Clone)]
pub enum TokenType {
    // Keywords
    Enum,
    Fn,
    Struct,
    Proto,
    Let,
    Mut,
    Pub,
    Return,
    If,
    Throw,
    Try,
    Else,
    Ref,
    Box,
    Import,
    As,
    Match,
    Loop,
    While,
    Break,
    Continue,
    Null,
    True,
    False,

    // Identifiers and Literals
    Identifier(String),
    Integer(i64),
    Float(f64),
    StringLiteral(String),
    InterpolatedString(Vec<InterpolatedStringPart>),

    // Operators
    Plus,
    Minus,
    Equals,
    NotEquals,
    Multiply,
    Divide,
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
    And,
    Or,
    Ampersand,
    Not,
    Assignment,
    Question,
    DoubleQuestion,
    Exclamation,

    // Punctuation
    Colon,
    Comma,
    Dot,
    DotDotDot,
    Semicolon,
    OpenBrace,
    CloseBrace,
    OpenParen,
    CloseParen,
    OpenBracket,
    CloseBracket,
    RightArrow,
    FatRightArrow,
    At,

    // End of File
    EOF,
}

#[derive(Debug, PartialEq, Clone)]
pub enum InterpolatedStringPart {
    Literal(String),
    Expr(String),
}

impl TokenType {
    pub fn discriminant(&self) -> std::mem::Discriminant<TokenType> {
        std::mem::discriminant(self)
    }
}

#[derive(Debug, PartialEq)]
pub enum LexerError {
    UnexpectedCharacter(char, (usize, usize)),
    UnterminatedString((usize, usize)),
    UnterminatedInterpolation((usize, usize)),
    InvalidNumber(String, (usize, usize)),
    InvalidEscapeSequence(String, (usize, usize)),
    InvalidUnicodeEscape(String, (usize, usize)),
    InvalidInterpolation(String, (usize, usize)),
}

impl std::error::Error for LexerError {}

impl std::fmt::Display for LexerError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            LexerError::UnexpectedCharacter(c, (line, col)) => {
                write!(
                    f,
                    "Unexpected character: {} at line: {} column: {}",
                    c, line, col
                )
            }
            LexerError::UnterminatedString((line, col)) => {
                write!(f, "Unterminated string at line: {} column: {}", line, col)
            }
            LexerError::UnterminatedInterpolation((line, col)) => {
                write!(
                    f,
                    "Unterminated interpolation at line: {} column: {}",
                    line, col
                )
            }
            LexerError::InvalidNumber(num, (line, col)) => {
                write!(
                    f,
                    "Invalid number: {} at line: {} column: {}",
                    num, line, col
                )
            }
            LexerError::InvalidEscapeSequence(seq, (line, col)) => {
                write!(
                    f,
                    "Invalid escape sequence: {} at line: {} column: {}",
                    seq, line, col
                )
            }
            LexerError::InvalidUnicodeEscape(val, (line, col)) => {
                write!(
                    f,
                    "Invalid unicode escape: {} at line: {} column: {}",
                    val, line, col
                )
            }
            LexerError::InvalidInterpolation(msg, (line, col)) => {
                write!(
                    f,
                    "Invalid interpolation: {} at line: {} column: {}",
                    msg, line, col
                )
            }
        }
    }
}

#[derive(Debug)]
pub struct Lexer {
    input: String,
    position: usize,
    line: usize,
    col: usize,
    pub errors: Vec<LexerError>,
}

// Maximum number of hex digits allowed in a \u{...} Unicode escape sequence
const MAX_UNICODE_ESCAPE_DIGITS: usize = 6;

impl Lexer {
    pub fn new(input: String) -> Self {
        let lexer = Lexer {
            input,
            position: 0,
            line: 1,
            col: 1,
            errors: vec![],
        };

        lexer
    }

    fn read_char(&mut self) {
        if let Some(c) = self.peek_char() {
            if c == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            self.position += c.len_utf8();
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }

    fn peek_char2(&self) -> Option<char> {
        let mut chars = self.input[self.position..].chars();
        chars.next();
        chars.next()
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek_char() {
            if !c.is_whitespace() {
                break;
            }

            self.read_char();
        }
    }

    fn read_identifier(&mut self) -> String {
        let start_position = self.position;
        while let Some(c) = self.peek_char() {
            if !c.is_alphanumeric() && c != '_' {
                break;
            }

            self.read_char();
        }

        self.input[start_position..self.position].to_string()
    }

    fn read_number(&mut self) -> String {
        let start_position = self.position;
        while let Some(c) = self.peek_char() {
            if !c.is_numeric() && c != '.' && c != '_' {
                break;
            }
            self.read_char();
        }
        self.input[start_position..self.position].to_string()
    }

    fn read_string(&mut self) -> Result<String, LexerError> {
        if self.peek_char() != Some('"') {
            return Err(LexerError::UnexpectedCharacter(
                self.peek_char().unwrap(),
                (self.line, self.col),
            ));
        }

        if self.position == self.input.len() - 1 {
            return Err(LexerError::UnterminatedString((self.line, self.col)));
        }

        let mut result = String::new();
        self.read_char(); // Skip opening "

        while let Some(c) = self.peek_char() {
            if c == '"' {
                break;
            }
            if c == '\n' {
                return Err(LexerError::UnterminatedString((self.line, self.col)));
            }
            if c == '\\' {
                // Handle escape sequence
                self.read_char(); // Skip backslash
                match self.peek_char() {
                    Some('\\') => {
                        result.push('\\');
                        self.read_char();
                    }
                    Some('"') => {
                        result.push('"');
                        self.read_char();
                    }
                    Some('n') => {
                        result.push('\n');
                        self.read_char();
                    }
                    Some('r') => {
                        result.push('\r');
                        self.read_char();
                    }
                    Some('t') => {
                        result.push('\t');
                        self.read_char();
                    }
                    Some('0') => {
                        result.push('\0');
                        self.read_char();
                    }
                    Some('u') => {
                        self.read_char(); // Skip 'u'
                        if self.peek_char() != Some('{') {
                            return Err(LexerError::InvalidEscapeSequence(
                                "\\u".to_string(),
                                (self.line, self.col),
                            ));
                        }
                        self.read_char(); // Skip '{'

                        let mut hex_digits = String::new();
                        while let Some(c) = self.peek_char() {
                            if c == '}' {
                                break;
                            }
                            if !c.is_ascii_hexdigit() {
                                return Err(LexerError::InvalidUnicodeEscape(
                                    format!("\\u{{{}}}", hex_digits),
                                    (self.line, self.col),
                                ));
                            }
                            hex_digits.push(c);
                            self.read_char();
                        }

                        if self.peek_char() != Some('}') {
                            return Err(LexerError::UnterminatedString((self.line, self.col)));
                        }
                        self.read_char(); // Skip '}'

                        if hex_digits.is_empty() || hex_digits.len() > MAX_UNICODE_ESCAPE_DIGITS {
                            return Err(LexerError::InvalidUnicodeEscape(
                                format!("\\u{{{}}}", hex_digits),
                                (self.line, self.col),
                            ));
                        }

                        let code_point = u32::from_str_radix(&hex_digits, 16).map_err(|_| {
                            LexerError::InvalidUnicodeEscape(
                                format!("\\u{{{}}}", hex_digits),
                                (self.line, self.col),
                            )
                        })?;

                        // Check if it's a valid Unicode scalar (not a surrogate)
                        let ch = char::from_u32(code_point).ok_or_else(|| {
                            LexerError::InvalidUnicodeEscape(
                                format!("\\u{{{:x}}}", code_point),
                                (self.line, self.col),
                            )
                        })?;

                        result.push(ch);
                    }
                    Some(other) => {
                        return Err(LexerError::InvalidEscapeSequence(
                            format!("\\{}", other),
                            (self.line, self.col),
                        ));
                    }
                    None => {
                        return Err(LexerError::UnterminatedString((self.line, self.col)));
                    }
                }
            } else {
                result.push(c);
                self.read_char();
            }
        }

        if self.peek_char() != Some('"') {
            return Err(LexerError::UnterminatedString((self.line, self.col)));
        }

        self.read_char(); // Skip closing "
        Ok(result)
    }

    fn read_interpolated_string(&mut self) -> Result<Vec<InterpolatedStringPart>, LexerError> {
        if self.peek_char() != Some('f') || self.peek_char2() != Some('"') {
            return Err(LexerError::InvalidInterpolation(
                "expected f\" for interpolated string".to_string(),
                (self.line, self.col),
            ));
        }

        if self.position >= self.input.len().saturating_sub(2) {
            return Err(LexerError::UnterminatedString((self.line, self.col)));
        }

        let mut parts = Vec::new();
        let mut current = String::new();

        self.read_char(); // Skip 'f'
        self.read_char(); // Skip opening "

        while let Some(c) = self.peek_char() {
            if c == '"' {
                break;
            }
            if c == '\n' {
                return Err(LexerError::UnterminatedString((self.line, self.col)));
            }
            if c == '\\' {
                self.read_char(); // Skip backslash
                match self.peek_char() {
                    Some('\\') => {
                        current.push('\\');
                        self.read_char();
                    }
                    Some('"') => {
                        current.push('"');
                        self.read_char();
                    }
                    Some('n') => {
                        current.push('\n');
                        self.read_char();
                    }
                    Some('r') => {
                        current.push('\r');
                        self.read_char();
                    }
                    Some('t') => {
                        current.push('\t');
                        self.read_char();
                    }
                    Some('0') => {
                        current.push('\0');
                        self.read_char();
                    }
                    Some('u') => {
                        self.read_char(); // Skip 'u'
                        if self.peek_char() != Some('{') {
                            return Err(LexerError::InvalidEscapeSequence(
                                "\\u".to_string(),
                                (self.line, self.col),
                            ));
                        }
                        self.read_char(); // Skip '{'

                        let mut hex_digits = String::new();
                        while let Some(c) = self.peek_char() {
                            if c == '}' {
                                break;
                            }
                            if !c.is_ascii_hexdigit() {
                                return Err(LexerError::InvalidUnicodeEscape(
                                    format!("\\u{{{}}}", hex_digits),
                                    (self.line, self.col),
                                ));
                            }
                            hex_digits.push(c);
                            self.read_char();
                        }

                        if self.peek_char() != Some('}') {
                            return Err(LexerError::UnterminatedString((self.line, self.col)));
                        }
                        self.read_char(); // Skip '}'

                        if hex_digits.is_empty() || hex_digits.len() > MAX_UNICODE_ESCAPE_DIGITS {
                            return Err(LexerError::InvalidUnicodeEscape(
                                format!("\\u{{{}}}", hex_digits),
                                (self.line, self.col),
                            ));
                        }

                        let code_point = u32::from_str_radix(&hex_digits, 16).map_err(|_| {
                            LexerError::InvalidUnicodeEscape(
                                format!("\\u{{{}}}", hex_digits),
                                (self.line, self.col),
                            )
                        })?;

                        let ch = char::from_u32(code_point).ok_or_else(|| {
                            LexerError::InvalidUnicodeEscape(
                                format!("\\u{{{:x}}}", code_point),
                                (self.line, self.col),
                            )
                        })?;

                        current.push(ch);
                    }
                    Some(other) => {
                        return Err(LexerError::InvalidEscapeSequence(
                            format!("\\{}", other),
                            (self.line, self.col),
                        ));
                    }
                    None => {
                        return Err(LexerError::UnterminatedString((self.line, self.col)));
                    }
                }
                continue;
            }

            if c == '{' {
                if self.peek_char2() == Some('{') {
                    current.push('{');
                    self.read_char();
                    self.read_char();
                    continue;
                }

                if !current.is_empty() {
                    parts.push(InterpolatedStringPart::Literal(current));
                    current = String::new();
                }

                self.read_char(); // Skip '{'
                let expr = self.read_interpolation_expr()?;
                parts.push(InterpolatedStringPart::Expr(expr));
                continue;
            }

            if c == '}' {
                if self.peek_char2() == Some('}') {
                    current.push('}');
                    self.read_char();
                    self.read_char();
                    continue;
                }
                return Err(LexerError::InvalidInterpolation(
                    "unmatched '}'".to_string(),
                    (self.line, self.col),
                ));
            }

            current.push(c);
            self.read_char();
        }

        if self.peek_char() != Some('"') {
            return Err(LexerError::UnterminatedString((self.line, self.col)));
        }

        if !current.is_empty() {
            parts.push(InterpolatedStringPart::Literal(current));
        }

        self.read_char(); // Skip closing "
        Ok(parts)
    }

    fn read_interpolation_expr(&mut self) -> Result<String, LexerError> {
        let mut expr = String::new();
        let mut depth = 1;
        let mut in_string = false;
        let mut in_char = false;
        let mut in_line_comment = false;
        let mut in_block_comment = false;

        while let Some(c) = self.peek_char() {
            if in_line_comment {
                expr.push(c);
                self.read_char();
                if c == '\n' {
                    in_line_comment = false;
                }
                continue;
            }

            if in_block_comment {
                if c == '*' && self.peek_char2() == Some('/') {
                    expr.push('*');
                    self.read_char();
                    expr.push('/');
                    self.read_char();
                    in_block_comment = false;
                    continue;
                }
                expr.push(c);
                self.read_char();
                continue;
            }

            if in_string {
                expr.push(c);
                self.read_char();
                if c == '\\' {
                    if let Some(next) = self.peek_char() {
                        expr.push(next);
                        self.read_char();
                    }
                    continue;
                }
                if c == '"' {
                    in_string = false;
                }
                continue;
            }

            if in_char {
                expr.push(c);
                self.read_char();
                if c == '\\' {
                    if let Some(next) = self.peek_char() {
                        expr.push(next);
                        self.read_char();
                    }
                    continue;
                }
                if c == '\'' {
                    in_char = false;
                }
                continue;
            }

            if c == '"' {
                in_string = true;
                expr.push(c);
                self.read_char();
                continue;
            }

            if c == '\'' {
                in_char = true;
                expr.push(c);
                self.read_char();
                continue;
            }

            if c == '/' && self.peek_char2() == Some('/') {
                in_line_comment = true;
                expr.push('/');
                self.read_char();
                expr.push('/');
                self.read_char();
                continue;
            }

            if c == '/' && self.peek_char2() == Some('*') {
                in_block_comment = true;
                expr.push('/');
                self.read_char();
                expr.push('*');
                self.read_char();
                continue;
            }

            if c == '{' {
                depth += 1;
                expr.push(c);
                self.read_char();
                continue;
            }

            if c == '}' {
                depth -= 1;
                if depth == 0 {
                    self.read_char();
                    return Ok(expr);
                }
                expr.push(c);
                self.read_char();
                continue;
            }

            expr.push(c);
            self.read_char();
        }

        Err(LexerError::UnterminatedInterpolation((self.line, self.col)))
    }

    pub fn next_token(&mut self) -> Token {
        self.skip_whitespace();

        let start_line = self.line;
        let start_col = self.col;
        let start_position = self.position;

        let token_type = match self.peek_char() {
            Some('+') => {
                self.read_char();
                TokenType::Plus
            }
            Some('-') => {
                self.read_char();
                if self.peek_char() == Some('>') {
                    self.read_char();
                    TokenType::RightArrow
                } else {
                    TokenType::Minus
                }
            }
            Some('*') => {
                self.read_char();
                TokenType::Multiply
            }
            Some('/') => {
                self.read_char();
                if self.peek_char() == Some('/') {
                    while let Some(c) = self.peek_char() {
                        if c == '\n' {
                            break;
                        }
                        self.read_char();
                    }
                    return self.next_token();
                } else if self.peek_char() == Some('*') {
                    while let Some(c) = self.peek_char() {
                        if c == '*' && self.peek_char2() == Some('/') {
                            self.read_char();
                            self.read_char();
                            break;
                        }
                        self.read_char();
                    }
                    return self.next_token();
                } else {
                    TokenType::Divide
                }
            }
            Some('=') => {
                self.read_char();
                if self.peek_char() == Some('=') {
                    self.read_char();
                    TokenType::Equals
                } else if self.peek_char() == Some('>') {
                    self.read_char();
                    TokenType::FatRightArrow
                } else {
                    TokenType::Assignment
                }
            }
            Some('!') => {
                self.read_char();
                if self.peek_char() == Some('=') {
                    self.read_char();
                    TokenType::NotEquals
                } else {
                    TokenType::Exclamation
                }
            }
            Some('?') => {
                self.read_char();
                if self.peek_char() == Some('?') {
                    self.read_char();
                    TokenType::DoubleQuestion
                } else {
                    TokenType::Question
                }
            }
            Some('>') => {
                self.read_char();
                if self.peek_char() == Some('=') {
                    self.read_char();
                    TokenType::GreaterThanOrEqual
                } else {
                    TokenType::GreaterThan
                }
            }
            Some('<') => {
                self.read_char();
                if self.peek_char() == Some('=') {
                    self.read_char();
                    TokenType::LessThanOrEqual
                } else {
                    TokenType::LessThan
                }
            }
            Some('&') => {
                self.read_char();
                if self.peek_char() == Some('&') {
                    self.read_char();
                    TokenType::And
                } else {
                    TokenType::Ampersand
                }
            }
            Some('|') => {
                self.read_char();
                if self.peek_char() == Some('|') {
                    self.read_char();
                    TokenType::Or
                } else {
                    panic!("Unexpected character: |");
                }
            }
            Some(',') => {
                self.read_char();
                TokenType::Comma
            }
            Some('.') => {
                self.read_char();
                if self.peek_char() == Some('.') {
                    self.read_char();
                    if self.peek_char() == Some('.') {
                        self.read_char();
                        TokenType::DotDotDot
                    } else {
                        // Two dots — put second dot back and return single Dot
                        self.position -= 1;
                        TokenType::Dot
                    }
                } else {
                    TokenType::Dot
                }
            }
            Some(';') => {
                self.read_char();
                TokenType::Semicolon
            }
            Some(':') => {
                self.read_char();
                TokenType::Colon
            }
            Some('{') => {
                self.read_char();
                TokenType::OpenBrace
            }
            Some('}') => {
                self.read_char();
                TokenType::CloseBrace
            }
            Some('(') => {
                self.read_char();
                TokenType::OpenParen
            }
            Some(')') => {
                self.read_char();
                TokenType::CloseParen
            }
            Some('[') => {
                self.read_char();
                TokenType::OpenBracket
            }
            Some(']') => {
                self.read_char();
                TokenType::CloseBracket
            }
            Some('@') => {
                self.read_char();
                TokenType::At
            }
            Some('"') => match self.read_string() {
                Ok(string) => TokenType::StringLiteral(string),
                Err(err) => {
                    self.errors.push(err);
                    TokenType::EOF
                }
            },

            Some('f') if self.peek_char2() == Some('"') => match self.read_interpolated_string() {
                Ok(parts) => {
                    let has_expr = parts
                        .iter()
                        .any(|part| matches!(part, InterpolatedStringPart::Expr(_)));
                    if has_expr {
                        TokenType::InterpolatedString(parts)
                    } else {
                        let mut literal = String::new();
                        for part in parts {
                            if let InterpolatedStringPart::Literal(chunk) = part {
                                literal.push_str(&chunk);
                            }
                        }
                        TokenType::StringLiteral(literal)
                    }
                }
                Err(err) => {
                    self.errors.push(err);
                    TokenType::EOF
                }
            },

            Some(c) => {
                if c.is_alphabetic() || c == '_' {
                    let ident = self.read_identifier();
                    match ident.as_str() {
                        "enum" => TokenType::Enum,
                        "fn" => TokenType::Fn,
                        "struct" => TokenType::Struct,
                        "proto" => TokenType::Proto,
                        "let" => TokenType::Let,
                        "mut" => TokenType::Mut,
                        "pub" => TokenType::Pub,
                        "return" => TokenType::Return,
                        "if" => TokenType::If,
                        "throw" => TokenType::Throw,
                        "try" => TokenType::Try,
                        "else" => TokenType::Else,
                        "ref" => TokenType::Ref,
                        "box" => TokenType::Box,
                        "import" => TokenType::Import,
                        "as" => TokenType::As,
                        "match" => TokenType::Match,
                        "loop" => TokenType::Loop,
                        "while" => TokenType::While,
                        "break" => TokenType::Break,
                        "continue" => TokenType::Continue,
                        "null" => TokenType::Null,
                        "true" => TokenType::True,
                        "false" => TokenType::False,
                        _ => TokenType::Identifier(ident),
                    }
                } else if c.is_numeric() {
                    let num = self.read_number();

                    let parsed_number = num.parse::<f64>();

                    match parsed_number {
                        Ok(n) => {
                            if num.contains('.') {
                                TokenType::Float(n)
                            } else {
                                TokenType::Integer(n as i64)
                            }
                        }
                        Err(_) => {
                            self.errors
                                .push(LexerError::InvalidNumber(num, (self.line, self.col)));
                            TokenType::EOF
                        }
                    }
                } else {
                    panic!("Unexpected character: {}", c);
                }
            }
            None => TokenType::EOF,
        };

        Token {
            token_type,
            line: start_line,
            col: start_col,
            length: self.position - start_position,
        }
    }
}
