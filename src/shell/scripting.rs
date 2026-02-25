//! Shell Scripting Language
//!
//! echOS shell scripting support:
//! - Variables (local and environment)
//! - Conditionals (if/elif/else/fi)
//! - Loops (while/for/until)
//! - Functions
//! - Arithmetic expansion $((expr))
//! - Command substitution $(cmd)

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

// ============================================================================
// SCRIPT TOKENS
// ============================================================================

/// Script token types
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptToken {
    // Keywords
    If,
    Then,
    Elif,
    Else,
    Fi,
    While,
    For,
    In,
    Do,
    Done,
    Until,
    Break,
    Continue,
    Return,
    Function,
    Local,
    Export,
    Readonly,
    Declare,
    
    // Operators
    Assign,         // =
    Plus,           // +
    Minus,          // -
    Star,           // *
    Slash,          // /
    Percent,        // %
    Equal,          // ==
    NotEqual,       // !=
    Less,           // <
    Greater,        // >
    LessEqual,      // <=
    GreaterEqual,   // >=
    And,            // &&
    Or,             // ||
    Not,            // !
    
    // Delimiters
    LeftParen,      // (
    RightParen,     // )
    LeftBracket,    // [
    RightBracket,   // ]
    LeftBrace,      // {
    RightBrace,     // }
    Semicolon,      // ;
    Newline,
    
    // Literals
    Word(String),
    Number(i64),
    String(String),
    
    // Special
    ArithStart,     // $((
    ArithEnd,       // ))
    CommandSubStart, // $(
    CommandSubEnd,   // )
    Variable(String), // $VAR or ${VAR}
    Eof,
}

// ============================================================================
// SCRIPT LEXER
// ============================================================================

/// Script lexer
pub struct ScriptLexer;

impl ScriptLexer {
    /// Tokenize script source
    pub fn tokenize(source: &str) -> Vec<ScriptToken> {
        let mut tokens = Vec::new();
        let mut chars = source.chars().peekable();
        
        while let Some(c) = chars.next() {
            match c {
                ' ' | '\t' | '\r' => continue,
                
                '\n' => {
                    tokens.push(ScriptToken::Newline);
                }
                
                ';' => {
                    tokens.push(ScriptToken::Semicolon);
                }
                
                '(' => {
                    if chars.peek() == Some(&'(') {
                        chars.next();
                        tokens.push(ScriptToken::ArithStart);
                    } else {
                        tokens.push(ScriptToken::LeftParen);
                    }
                }
                
                ')' => {
                    if chars.peek() == Some(&')') {
                        chars.next();
                        tokens.push(ScriptToken::ArithEnd);
                    } else {
                        tokens.push(ScriptToken::RightParen);
                    }
                }
                
                '[' => {
                    tokens.push(ScriptToken::LeftBracket);
                }
                
                ']' => {
                    tokens.push(ScriptToken::RightBracket);
                }
                
                '{' => {
                    tokens.push(ScriptToken::LeftBrace);
                }
                
                '}' => {
                    tokens.push(ScriptToken::RightBrace);
                }
                
                '=' => {
                    tokens.push(ScriptToken::Assign);
                }
                
                '+' => {
                    tokens.push(ScriptToken::Plus);
                }
                
                '-' => {
                    tokens.push(ScriptToken::Minus);
                }
                
                '*' => {
                    tokens.push(ScriptToken::Star);
                }
                
                '/' => {
                    tokens.push(ScriptToken::Slash);
                }
                
                '%' => {
                    tokens.push(ScriptToken::Percent);
                }
                
                '!' => {
                    if chars.peek() == Some(&'=') {
                        chars.next();
                        tokens.push(ScriptToken::NotEqual);
                    } else {
                        tokens.push(ScriptToken::Not);
                    }
                }
                
                '<' => {
                    if chars.peek() == Some(&'=') {
                        chars.next();
                        tokens.push(ScriptToken::LessEqual);
                    } else {
                        tokens.push(ScriptToken::Less);
                    }
                }
                
                '>' => {
                    if chars.peek() == Some(&'=') {
                        chars.next();
                        tokens.push(ScriptToken::GreaterEqual);
                    } else {
                        tokens.push(ScriptToken::Greater);
                    }
                }
                
                '&' => {
                    if chars.peek() == Some(&'&') {
                        chars.next();
                        tokens.push(ScriptToken::And);
                    }
                }
                
                '|' => {
                    if chars.peek() == Some(&'|') {
                        chars.next();
                        tokens.push(ScriptToken::Or);
                    }
                }
                
                '$' => {
                    if chars.peek() == Some(&'(') {
                        chars.next();
                        if chars.peek() == Some(&'(') {
                            chars.next();
                            tokens.push(ScriptToken::ArithStart);
                        } else {
                            tokens.push(ScriptToken::CommandSubStart);
                        }
                    } else if chars.peek() == Some(&'{') {
                        chars.next();
                        let mut var_name = String::new();
                        while let Some(&ch) = chars.peek() {
                            if ch == '}' {
                                chars.next();
                                break;
                            }
                            var_name.push(ch);
                            chars.next();
                        }
                        tokens.push(ScriptToken::Variable(var_name));
                    } else {
                        let mut var_name = String::new();
                        while let Some(&ch) = chars.peek() {
                            if ch.is_alphanumeric() || ch == '_' {
                                var_name.push(ch);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        tokens.push(ScriptToken::Variable(var_name));
                    }
                }
                
                '#' => {
                    // Comment - skip until newline
                    while let Some(&ch) = chars.peek() {
                        if ch == '\n' {
                            break;
                        }
                        chars.next();
                    }
                }
                
                '\'' => {
                    // Single-quoted string
                    let mut s = String::new();
                    while let Some(ch) = chars.next() {
                        if ch == '\'' {
                            break;
                        }
                        s.push(ch);
                    }
                    tokens.push(ScriptToken::String(s));
                }
                
                '"' => {
                    // Double-quoted string
                    let mut s = String::new();
                    while let Some(ch) = chars.next() {
                        if ch == '"' {
                            break;
                        }
                        if ch == '\\' {
                            if let Some(escaped) = chars.next() {
                                s.push(escaped);
                            }
                        } else {
                            s.push(ch);
                        }
                    }
                    tokens.push(ScriptToken::String(s));
                }
                
                '0'..='9' => {
                    let mut num_str = String::new();
                    num_str.push(c);
                    while let Some(&ch) = chars.peek() {
                        if ch.is_ascii_digit() {
                            num_str.push(ch);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    if let Ok(n) = num_str.parse::<i64>() {
                        tokens.push(ScriptToken::Number(n));
                    } else {
                        tokens.push(ScriptToken::Word(num_str));
                    }
                }
                
                'a'..='z' | 'A'..='Z' | '_' => {
                    let mut word = String::new();
                    word.push(c);
                    while let Some(&ch) = chars.peek() {
                        if ch.is_alphanumeric() || ch == '_' {
                            word.push(ch);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    
                    // Check for keywords
                    let token = match word.as_str() {
                        "if" => ScriptToken::If,
                        "then" => ScriptToken::Then,
                        "elif" => ScriptToken::Elif,
                        "else" => ScriptToken::Else,
                        "fi" => ScriptToken::Fi,
                        "while" => ScriptToken::While,
                        "for" => ScriptToken::For,
                        "in" => ScriptToken::In,
                        "do" => ScriptToken::Do,
                        "done" => ScriptToken::Done,
                        "until" => ScriptToken::Until,
                        "break" => ScriptToken::Break,
                        "continue" => ScriptToken::Continue,
                        "return" => ScriptToken::Return,
                        "function" => ScriptToken::Function,
                        "local" => ScriptToken::Local,
                        "export" => ScriptToken::Export,
                        "readonly" => ScriptToken::Readonly,
                        "declare" => ScriptToken::Declare,
                        _ => ScriptToken::Word(word),
                    };
                    tokens.push(token);
                }
                
                _ => {
                    // Unknown character - skip
                }
            }
        }
        
        tokens.push(ScriptToken::Eof);
        tokens
    }
}

// ============================================================================
// AST NODES
// ============================================================================

/// AST node for script
#[derive(Clone, Debug)]
pub enum Stmt {
    /// Variable assignment: VAR=value
    Assign {
        name: String,
        value: Expr,
        local: bool,
        export: bool,
    },
    /// Simple command
    Command {
        args: Vec<Expr>,
    },
    /// If statement
    If {
        condition: Expr,
        then_body: Vec<Stmt>,
        elif_clauses: Vec<(Expr, Vec<Stmt>)>,
        else_body: Option<Vec<Stmt>>,
    },
    /// While loop
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    /// For loop
    For {
        var: String,
        items: Vec<Expr>,
        body: Vec<Stmt>,
    },
    /// Until loop
    Until {
        condition: Expr,
        body: Vec<Stmt>,
    },
    /// Function definition
    Function {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    /// Return statement
    Return(Option<Expr>),
    /// Break statement
    Break,
    /// Continue statement
    Continue,
    /// No-op
    Nop,
}

/// Expression node
#[derive(Clone, Debug)]
pub enum Expr {
    /// Literal string
    String(String),
    /// Literal number
    Number(i64),
    /// Variable reference
    Variable(String),
    /// Arithmetic expression
    Arithmetic(Box<Expr>),
    /// Command substitution
    CommandSub(Vec<Expr>),
    /// Binary operation
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// Unary operation
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    /// Test expression [ expr ]
    Test(Box<Expr>),
    /// String comparison
    StrCompare {
        op: StrCompareOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnaryOp {
    Not,
    Neg,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StrCompareOp {
    Eq,      // =
    Ne,      // !=
    Lt,      // <
    Gt,      // >
    Le,      // <=
    Ge,      // >=
    Match,   // =~
    Nmatch,  // !~
}

// ============================================================================
// SCRIPT PARSER
// ============================================================================

/// Script parser
pub struct ScriptParser {
    tokens: Vec<ScriptToken>,
    pos: usize,
}

impl ScriptParser {
    pub fn new(tokens: Vec<ScriptToken>) -> Self {
        Self { tokens, pos: 0 }
    }
    
    /// Parse script into statements
    pub fn parse(&mut self) -> Result<Vec<Stmt>, ScriptError> {
        let mut stmts = Vec::new();
        
        while !self.is_at_end() {
            // Skip newlines and semicolons
            while self.check(&ScriptToken::Newline) || self.check(&ScriptToken::Semicolon) {
                self.advance();
            }
            
            if self.is_at_end() {
                break;
            }
            
            stmts.push(self.parse_stmt()?);
        }
        
        Ok(stmts)
    }
    
    fn parse_stmt(&mut self) -> Result<Stmt, ScriptError> {
        match self.peek() {
            ScriptToken::If => self.parse_if(),
            ScriptToken::While => self.parse_while(),
            ScriptToken::For => self.parse_for(),
            ScriptToken::Until => self.parse_until(),
            ScriptToken::Function => self.parse_function(),
            ScriptToken::Return => {
                self.advance();
                let expr = if !self.check(&ScriptToken::Newline) && !self.check(&ScriptToken::Semicolon) {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                Ok(Stmt::Return(expr))
            }
            ScriptToken::Break => {
                self.advance();
                Ok(Stmt::Break)
            }
            ScriptToken::Continue => {
                self.advance();
                Ok(Stmt::Continue)
            }
            ScriptToken::Local | ScriptToken::Export | ScriptToken::Declare => {
                self.parse_declaration()
            }
            ScriptToken::Word(name) if self.check_next(&ScriptToken::Assign) => {
                self.parse_assignment(false, false)
            }
            ScriptToken::Word(name) if self.check_next(&ScriptToken::LeftParen) => {
                // Function call or command
                self.parse_command()
            }
            _ => self.parse_command(),
        }
    }
    
    fn parse_if(&mut self) -> Result<Stmt, ScriptError> {
        self.expect(ScriptToken::If)?;
        
        let condition = self.parse_expr()?;
        self.expect(ScriptToken::Then)?;
        
        let mut then_body = Vec::new();
        while !self.check(&ScriptToken::Elif) 
           && !self.check(&ScriptToken::Else) 
           && !self.check(&ScriptToken::Fi) {
            then_body.push(self.parse_stmt()?);
        }
        
        let mut elif_clauses = Vec::new();
        while self.check(&ScriptToken::Elif) {
            self.advance();
            let elif_cond = self.parse_expr()?;
            self.expect(ScriptToken::Then)?;
            
            let mut elif_body = Vec::new();
            while !self.check(&ScriptToken::Elif) 
               && !self.check(&ScriptToken::Else) 
               && !self.check(&ScriptToken::Fi) {
                elif_body.push(self.parse_stmt()?);
            }
            elif_clauses.push((elif_cond, elif_body));
        }
        
        let mut else_body = None;
        if self.check(&ScriptToken::Else) {
            self.advance();
            let mut body = Vec::new();
            while !self.check(&ScriptToken::Fi) {
                body.push(self.parse_stmt()?);
            }
            else_body = Some(body);
        }
        
        self.expect(ScriptToken::Fi)?;
        
        Ok(Stmt::If {
            condition,
            then_body,
            elif_clauses,
            else_body,
        })
    }
    
    fn parse_while(&mut self) -> Result<Stmt, ScriptError> {
        self.expect(ScriptToken::While)?;
        
        let condition = self.parse_expr()?;
        self.expect(ScriptToken::Do)?;
        
        let mut body = Vec::new();
        while !self.check(&ScriptToken::Done) {
            body.push(self.parse_stmt()?);
        }
        self.expect(ScriptToken::Done)?;
        
        Ok(Stmt::While { condition, body })
    }
    
    fn parse_for(&mut self) -> Result<Stmt, ScriptError> {
        self.expect(ScriptToken::For)?;
        
        let var = if let ScriptToken::Word(name) = self.advance() {
            name.clone()
        } else {
            return Err(ScriptError::ExpectedVariable);
        };
        
        self.expect(ScriptToken::In)?;
        
        let mut items = Vec::new();
        while !self.check(&ScriptToken::Do) {
            items.push(self.parse_expr()?);
        }
        
        self.expect(ScriptToken::Do)?;
        
        let mut body = Vec::new();
        while !self.check(&ScriptToken::Done) {
            body.push(self.parse_stmt()?);
        }
        self.expect(ScriptToken::Done)?;
        
        Ok(Stmt::For { var, items, body })
    }
    
    fn parse_until(&mut self) -> Result<Stmt, ScriptError> {
        self.expect(ScriptToken::Until)?;
        
        let condition = self.parse_expr()?;
        self.expect(ScriptToken::Do)?;
        
        let mut body = Vec::new();
        while !self.check(&ScriptToken::Done) {
            body.push(self.parse_stmt()?);
        }
        self.expect(ScriptToken::Done)?;
        
        Ok(Stmt::Until { condition, body })
    }
    
    fn parse_function(&mut self) -> Result<Stmt, ScriptError> {
        self.expect(ScriptToken::Function)?;
        
        let name = if let ScriptToken::Word(name) = self.advance() {
            name.clone()
        } else {
            return Err(ScriptError::ExpectedFunctionName);
        };
        
        self.expect(ScriptToken::LeftParen)?;
        self.expect(ScriptToken::RightParen)?;
        
        self.expect(ScriptToken::LeftBrace)?;
        
        let mut body = Vec::new();
        while !self.check(&ScriptToken::RightBrace) {
            body.push(self.parse_stmt()?);
        }
        self.expect(ScriptToken::RightBrace)?;
        
        Ok(Stmt::Function {
            name,
            params: Vec::new(),
            body,
        })
    }
    
    fn parse_declaration(&mut self) -> Result<Stmt, ScriptError> {
        let is_local = self.check(&ScriptToken::Local);
        let is_export = self.check(&ScriptToken::Export);
        
        self.advance();
        
        self.parse_assignment(is_local, is_export)
    }
    
    fn parse_assignment(&mut self, local: bool, export: bool) -> Result<Stmt, ScriptError> {
        let name = if let ScriptToken::Word(name) = self.advance() {
            name.clone()
        } else {
            return Err(ScriptError::ExpectedVariable);
        };
        
        self.expect(ScriptToken::Assign)?;
        
        let value = self.parse_expr()?;
        
        Ok(Stmt::Assign { name, value, local, export })
    }
    
    fn parse_command(&mut self) -> Result<Stmt, ScriptError> {
        let mut args = Vec::new();
        
        while !self.check(&ScriptToken::Newline) 
           && !self.check(&ScriptToken::Semicolon)
           && !self.check(&ScriptToken::Eof)
           && !self.is_keyword() {
            args.push(self.parse_expr()?);
        }
        
        Ok(Stmt::Command { args })
    }
    
    fn parse_expr(&mut self) -> Result<Expr, ScriptError> {
        self.parse_or()
    }
    
    fn parse_or(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.parse_and()?;
        
        while self.check(&ScriptToken::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::Binary {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        
        Ok(left)
    }
    
    fn parse_and(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.parse_comparison()?;
        
        while self.check(&ScriptToken::And) {
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr::Binary {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        
        Ok(left)
    }
    
    fn parse_comparison(&mut self) -> Result<Expr, ScriptError> {
        let left = self.parse_additive()?;
        
        let op = match self.peek() {
            ScriptToken::Equal => BinOp::Eq,
            ScriptToken::NotEqual => BinOp::Ne,
            ScriptToken::Less => BinOp::Lt,
            ScriptToken::Greater => BinOp::Gt,
            ScriptToken::LessEqual => BinOp::Le,
            ScriptToken::GreaterEqual => BinOp::Ge,
            _ => return Ok(left),
        };
        
        self.advance();
        let right = self.parse_additive()?;
        
        Ok(Expr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }
    
    fn parse_additive(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.parse_multiplicative()?;
        
        loop {
            let op = match self.peek() {
                ScriptToken::Plus => BinOp::Add,
                ScriptToken::Minus => BinOp::Sub,
                _ => break,
            };
            
            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        
        Ok(left)
    }
    
    fn parse_multiplicative(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.parse_unary()?;
        
        loop {
            let op = match self.peek() {
                ScriptToken::Star => BinOp::Mul,
                ScriptToken::Slash => BinOp::Div,
                ScriptToken::Percent => BinOp::Mod,
                _ => break,
            };
            
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        
        Ok(left)
    }
    
    fn parse_unary(&mut self) -> Result<Expr, ScriptError> {
        let op = match self.peek() {
            ScriptToken::Not => UnaryOp::Not,
            ScriptToken::Minus => UnaryOp::Neg,
            _ => return self.parse_primary(),
        };
        
        self.advance();
        let operand = self.parse_unary()?;
        
        Ok(Expr::Unary {
            op,
            operand: Box::new(operand),
        })
    }
    
    fn parse_primary(&mut self) -> Result<Expr, ScriptError> {
        match self.peek() {
            ScriptToken::Number(n) => {
                let n = *n;
                self.advance();
                Ok(Expr::Number(n))
            }
            ScriptToken::String(s) => {
                let s = s.clone();
                self.advance();
                Ok(Expr::String(s))
            }
            ScriptToken::Variable(name) => {
                let name = name.clone();
                self.advance();
                Ok(Expr::Variable(name))
            }
            ScriptToken::ArithStart => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(ScriptToken::ArithEnd)?;
                Ok(Expr::Arithmetic(Box::new(expr)))
            }
            ScriptToken::CommandSubStart => {
                self.advance();
                let mut args = Vec::new();
                while !self.check(&ScriptToken::CommandSubEnd) && !self.check(&ScriptToken::Eof) {
                    args.push(self.parse_expr()?);
                }
                self.expect(ScriptToken::CommandSubEnd)?;
                Ok(Expr::CommandSub(args))
            }
            ScriptToken::LeftBracket => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(ScriptToken::RightBracket)?;
                Ok(Expr::Test(Box::new(expr)))
            }
            ScriptToken::LeftParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(ScriptToken::RightParen)?;
                Ok(expr)
            }
            ScriptToken::Word(w) => {
                let w = w.clone();
                self.advance();
                Ok(Expr::String(w))
            }
            _ => Err(ScriptError::UnexpectedToken),
        }
    }
    
    // Helper methods
    fn peek(&self) -> &ScriptToken {
        self.tokens.get(self.pos).unwrap_or(&ScriptToken::Eof)
    }
    
    fn advance(&mut self) -> &ScriptToken {
        self.pos += 1;
        self.tokens.get(self.pos - 1).unwrap_or(&ScriptToken::Eof)
    }
    
    fn check(&self, token: &ScriptToken) -> bool {
        self.peek() == token
    }
    
    fn check_next(&self, token: &ScriptToken) -> bool {
        self.tokens.get(self.pos + 1).unwrap_or(&ScriptToken::Eof) == token
    }
    
    fn is_at_end(&self) -> bool {
        self.peek() == &ScriptToken::Eof
    }
    
    fn is_keyword(&self) -> bool {
        matches!(self.peek(), 
            ScriptToken::If | ScriptToken::Then | ScriptToken::Elif |
            ScriptToken::Else | ScriptToken::Fi | ScriptToken::While |
            ScriptToken::For | ScriptToken::Do | ScriptToken::Done |
            ScriptToken::Until | ScriptToken::Function | ScriptToken::Return |
            ScriptToken::Break | ScriptToken::Continue
        )
    }
    
    fn expect(&mut self, token: ScriptToken) -> Result<(), ScriptError> {
        if self.check(&token) {
            self.advance();
            Ok(())
        } else {
            Err(ScriptError::ExpectedToken(token))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScriptError {
    UnexpectedToken,
    ExpectedToken(ScriptToken),
    ExpectedVariable,
    ExpectedFunctionName,
    UndefinedVariable(String),
    UndefinedFunction(String),
    DivisionByZero,
    RuntimeError(String),
}

// ============================================================================
// SCRIPT INTERPRETER
// ============================================================================

/// Script interpreter state
pub struct ScriptState {
    /// Local variables (function scope)
    pub local_vars: Mutex<BTreeMap<String, String>>,
    /// Functions
    pub functions: Mutex<BTreeMap<String, Stmt>>,
    /// Return value
    pub return_value: Mutex<Option<i64>>,
    /// Break flag
    pub break_flag: Mutex<bool>,
    /// Continue flag
    pub continue_flag: Mutex<bool>,
}

impl ScriptState {
    pub const fn new() -> Self {
        Self {
            local_vars: Mutex::new(BTreeMap::new()),
            functions: Mutex::new(BTreeMap::new()),
            return_value: Mutex::new(None),
            break_flag: Mutex::new(false),
            continue_flag: Mutex::new(false),
        }
    }
    
    /// Set local variable
    pub fn set_local(&self, name: &str, value: &str) {
        self.local_vars.lock().insert(name.to_string(), value.to_string());
    }
    
    /// Get variable (local first, then environment)
    pub fn get_var(&self, name: &str) -> Option<String> {
        // Check local vars first
        if let Some(val) = self.local_vars.lock().get(name) {
            return Some(val.clone());
        }
        // Then check environment
        super::advanced::ENV.get(name)
    }
    
    /// Clear break/continue flags
    pub fn clear_flags(&self) {
        *self.break_flag.lock() = false;
        *self.continue_flag.lock() = false;
    }
    
    /// Check if break requested
    pub fn should_break(&self) -> bool {
        *self.break_flag.lock()
    }
    
    /// Check if continue requested
    pub fn should_continue(&self) -> bool {
        *self.continue_flag.lock()
    }
}

lazy_static::lazy_static! {
    /// Global script state
    pub static ref SCRIPT_STATE: ScriptState = ScriptState::new();
}

/// Script interpreter
pub struct Interpreter;

impl Interpreter {
    /// Execute script statements
    pub fn execute(stmts: &[Stmt]) -> Result<i64, ScriptError> {
        let mut last_exit_code = 0;
        
        for stmt in stmts {
            last_exit_code = Self::exec_stmt(stmt)?;
            
            // Check for return/break/continue
            if SCRIPT_STATE.return_value.lock().is_some() {
                return Ok(SCRIPT_STATE.return_value.lock().unwrap_or(0));
            }
            if SCRIPT_STATE.should_break() || SCRIPT_STATE.should_continue() {
                break;
            }
        }
        
        Ok(last_exit_code)
    }
    
    fn exec_stmt(stmt: &Stmt) -> Result<i64, ScriptError> {
        match stmt {
            Stmt::Assign { name, value, local, export } => {
                let val = Self::eval_expr(value)?;
                
                if *local {
                    SCRIPT_STATE.set_local(name, &val);
                } else if *export {
                    super::advanced::ENV.set(name, &val);
                } else {
                    // Set in both local and env
                    SCRIPT_STATE.set_local(name, &val);
                    super::advanced::ENV.set(name, &val);
                }
                
                Ok(0)
            }
            
            Stmt::Command { args } => {
                // Evaluate arguments
                let evaluated: Vec<String> = args.iter()
                    .map(|a| Self::eval_expr(a))
                    .collect::<Result<Vec<_>, _>>()?;
                
                // Execute command
                // TODO: Integrate with actual command execution
                crate::serial_println!("[SCRIPT] Command: {}", evaluated.join(" "));
                Ok(0)
            }
            
            Stmt::If { condition, then_body, elif_clauses, else_body } => {
                if Self::is_truthy(condition)? {
                    Self::execute(then_body)?;
                } else {
                    let mut executed = false;
                    for (elif_cond, elif_body) in elif_clauses {
                        if Self::is_truthy(elif_cond)? {
                            Self::execute(elif_body)?;
                            executed = true;
                            break;
                        }
                    }
                    if !executed {
                        if let Some(body) = else_body {
                            Self::execute(body)?;
                        }
                    }
                }
                Ok(0)
            }
            
            Stmt::While { condition, body } => {
                SCRIPT_STATE.clear_flags();
                
                while Self::is_truthy(condition)? && !SCRIPT_STATE.should_break() {
                    Self::execute(body)?;
                    
                    if SCRIPT_STATE.should_continue() {
                        SCRIPT_STATE.clear_flags();
                        continue;
                    }
                }
                
                SCRIPT_STATE.clear_flags();
                Ok(0)
            }
            
            Stmt::For { var, items, body } => {
                SCRIPT_STATE.clear_flags();
                
                for item in items {
                    let val = Self::eval_expr(item)?;
                    SCRIPT_STATE.set_local(var, &val);
                    
                    if SCRIPT_STATE.should_break() {
                        break;
                    }
                    
                    Self::execute(body)?;
                    
                    if SCRIPT_STATE.should_continue() {
                        SCRIPT_STATE.clear_flags();
                        continue;
                    }
                }
                
                SCRIPT_STATE.clear_flags();
                Ok(0)
            }
            
            Stmt::Until { condition, body } => {
                SCRIPT_STATE.clear_flags();
                
                while !Self::is_truthy(condition)? && !SCRIPT_STATE.should_break() {
                    Self::execute(body)?;
                    
                    if SCRIPT_STATE.should_continue() {
                        SCRIPT_STATE.clear_flags();
                        continue;
                    }
                }
                
                SCRIPT_STATE.clear_flags();
                Ok(0)
            }
            
            Stmt::Function { name, params, body } => {
                // Store function definition
                SCRIPT_STATE.functions.lock().insert(
                    name.clone(),
                    Stmt::Function {
                        name: name.clone(),
                        params: params.clone(),
                        body: body.clone(),
                    },
                );
                Ok(0)
            }
            
            Stmt::Return(expr) => {
                let code = if let Some(e) = expr {
                    Self::eval_expr(e)?.parse().unwrap_or(0)
                } else {
                    0
                };
                *SCRIPT_STATE.return_value.lock() = Some(code);
                Ok(code)
            }
            
            Stmt::Break => {
                *SCRIPT_STATE.break_flag.lock() = true;
                Ok(0)
            }
            
            Stmt::Continue => {
                *SCRIPT_STATE.continue_flag.lock() = true;
                Ok(0)
            }
            
            Stmt::Nop => Ok(0),
        }
    }
    
    fn eval_expr(expr: &Expr) -> Result<String, ScriptError> {
        match expr {
            Expr::String(s) => Ok(s.clone()),
            Expr::Number(n) => Ok(n.to_string()),
            Expr::Variable(name) => {
                SCRIPT_STATE.get_var(name).ok_or_else(|| ScriptError::UndefinedVariable(name.clone()))
            }
            Expr::Arithmetic(inner) => {
                let n = Self::eval_arithmetic(inner)?;
                Ok(n.to_string())
            }
            Expr::CommandSub(args) => {
                // TODO: Execute command and capture output
                let cmd: Vec<String> = args.iter()
                    .map(|a| Self::eval_expr(a))
                    .collect::<Result<Vec<_>, _>>()?;
                crate::serial_println!("[SCRIPT] Command substitution: {}", cmd.join(" "));
                Ok(String::new())
            }
            Expr::Binary { op, left, right } => {
                let l = Self::eval_arithmetic(left)?;
                let r = Self::eval_arithmetic(right)?;
                
                let result = match op {
                    BinOp::Add => l + r,
                    BinOp::Sub => l - r,
                    BinOp::Mul => l * r,
                    BinOp::Div => {
                        if r == 0 {
                            return Err(ScriptError::DivisionByZero);
                        }
                        l / r
                    }
                    BinOp::Mod => {
                        if r == 0 {
                            return Err(ScriptError::DivisionByZero);
                        }
                        l % r
                    }
                    BinOp::Eq => (l == r) as i64,
                    BinOp::Ne => (l != r) as i64,
                    BinOp::Lt => (l < r) as i64,
                    BinOp::Gt => (l > r) as i64,
                    BinOp::Le => (l <= r) as i64,
                    BinOp::Ge => (l >= r) as i64,
                    BinOp::And => (l != 0 && r != 0) as i64,
                    BinOp::Or => (l != 0 || r != 0) as i64,
                };
                
                Ok(result.to_string())
            }
            Expr::Unary { op, operand } => {
                let n = Self::eval_arithmetic(operand)?;
                
                let result = match op {
                    UnaryOp::Not => (n == 0) as i64,
                    UnaryOp::Neg => -n,
                };
                
                Ok(result.to_string())
            }
            Expr::Test(inner) => {
                let truthy = Self::is_truthy(inner)?;
                Ok((truthy as i64).to_string())
            }
            Expr::StrCompare { op, left, right } => {
                let l = Self::eval_expr(left)?;
                let r = Self::eval_expr(right)?;
                
                let result = match op {
                    StrCompareOp::Eq => l == r,
                    StrCompareOp::Ne => l != r,
                    StrCompareOp::Lt => l < r,
                    StrCompareOp::Gt => l > r,
                    StrCompareOp::Le => l <= r,
                    StrCompareOp::Ge => l >= r,
                    StrCompareOp::Match => l.contains(&r), // Simplified
                    StrCompareOp::Nmatch => !l.contains(&r),
                };
                
                Ok((result as i64).to_string())
            }
        }
    }
    
    fn eval_arithmetic(expr: &Expr) -> Result<i64, ScriptError> {
        let s = Self::eval_expr(expr)?;
        s.parse().map_err(|_| ScriptError::RuntimeError(format!("Not a number: {}", s)))
    }
    
    fn is_truthy(expr: &Expr) -> Result<bool, ScriptError> {
        let val = Self::eval_expr(expr)?;
        Ok(!val.is_empty() && val != "0")
    }
}

// ============================================================================
// PUBLIC API
// ============================================================================

/// Parse and execute a script
pub fn run_script(source: &str) -> Result<i64, ScriptError> {
    let tokens = ScriptLexer::tokenize(source);
    let mut parser = ScriptParser::new(tokens);
    let stmts = parser.parse()?;
    Interpreter::execute(&stmts)
}

/// Parse a single command line
pub fn parse_line(line: &str) -> Result<Vec<Stmt>, ScriptError> {
    let tokens = ScriptLexer::tokenize(line);
    let mut parser = ScriptParser::new(tokens);
    parser.parse()
}

/// Evaluate an expression
pub fn eval_expression(expr_str: &str) -> Result<String, ScriptError> {
    let tokens = ScriptLexer::tokenize(expr_str);
    let mut parser = ScriptParser::new(tokens);
    let expr = parser.parse_expr()?;
    Interpreter::eval_expr(&expr)
}
