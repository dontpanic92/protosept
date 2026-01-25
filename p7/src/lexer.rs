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
    Let,
    Pub,
    Return,
    If,
    Throw,
    Try,
    Else,

    // Identifiers and Literals
    Identifier(String),
    Integer(i64),
    Float(f64),
    StringLiteral(String),

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

    // Punctuation
    Colon,
    Comma,
    Dot,
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

impl TokenType {
    pub fn discriminant(&self) -> std::mem::Discriminant<TokenType> {
        std::mem::discriminant(self)
    }
}

#[derive(Debug, PartialEq)]
pub enum LexerError {
    UnexpectedCharacter(char, (usize, usize)),
    UnterminatedString((usize, usize)),
    InvalidNumber(String, (usize, usize)),
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
            LexerError::InvalidNumber(num, (line, col)) => {
                write!(
                    f,
                    "Invalid number: {} at line: {} column: {}",
                    num, line, col
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
        if self.peek_char() == Some('\n') {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }

        if self.position < self.input.len() {
            self.position += 1;
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.input.chars().nth(self.position)
    }

    fn peek_char2(&self) -> Option<char> {
        self.input.chars().nth(self.position + 1)
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

        let start_position = self.position + 1;
        self.read_char();
        while let Some(c) = self.peek_char() {
            if c == '"' {
                break;
            }
            self.read_char();
        }
        if self.peek_char() != Some('"') {
            return Err(LexerError::UnterminatedString((self.line, self.col)));
        }

        let result = self.input[start_position..self.position].to_string();
        self.read_char();
        Ok(result)
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
                    TokenType::Not
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
                TokenType::Ampersand
            }
            Some(',') => {
                self.read_char();
                TokenType::Comma
            }
            Some('.') => {
                self.read_char();
                TokenType::Dot
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

            Some(c) => {
                if c.is_alphabetic() || c == '_' {
                    let ident = self.read_identifier();
                    match ident.as_str() {
                        "enum" => TokenType::Enum,
                        "fn" => TokenType::Fn,
                        "struct" => TokenType::Struct,
                        "let" => TokenType::Let,
                        "pub" => TokenType::Pub,
                        "return" => TokenType::Return,
                        "if" => TokenType::If,
                        "throw" => TokenType::Throw,
                        "try" => TokenType::Try,
                        "else" => TokenType::Else,
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
