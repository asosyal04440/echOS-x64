//! # Matematiksel İfade Ayrıştırıcı (Expression Evaluator)
//!
//! Shunting-yard algoritması ile infix ifadeleri postfix (RPN) formata çevirir
//! ve yığın tabanlı hesaplama yapar.
//!
//! Desteklenenler:
//! - Temel operatörler: +, -, *, /, %, ^ (üs)
//! - Fonksiyonlar: sin, cos, tan, sqrt, log, abs
//! - Parantezler: ( )
//! - Öncelik sırası ve birleşme kuralları

use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use libm::{powf, sqrtf, sinf, cosf, tanf, log10f, fabsf};

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f32),
    Operator(char),
    Function(String),
    LeftParen,
    RightParen,
}

/// İfadeyi hesaplar ve sonucu döner. Hata durumunda None döner.
pub fn evaluate(expr: &str) -> Option<f32> {
    let tokens = tokenize(expr)?;
    let rpn = shunting_yard(tokens)?;
    eval_rpn(rpn)
}

fn tokenize(expr: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = expr.chars().peekable();

    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c.is_digit(10) || c == '.' {
            // Sayı ayrıştırma
            let mut num_str = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_digit(10) || d == '.' {
                    num_str.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            let num = num_str.parse::<f32>().ok()?;
            tokens.push(Token::Number(num));
        } else if c.is_alphabetic() {
            // Fonksiyon ayrıştırma
            let mut func_str = String::new();
            while let Some(&f) = chars.peek() {
                if f.is_alphabetic() {
                    func_str.push(f);
                    chars.next();
                } else {
                    break;
                }
            }
            tokens.push(Token::Function(func_str));
        } else {
            // Operatörler
            match c {
                '+' | '-' | '*' | '/' | '%' | '^' => tokens.push(Token::Operator(c)),
                '(' => tokens.push(Token::LeftParen),
                ')' => tokens.push(Token::RightParen),
                _ => return None, // Bilinmeyen karakter
            }
            chars.next();
        }
    }
    Some(tokens)
}

fn shunting_yard(tokens: Vec<Token>) -> Option<Vec<Token>> {
    let mut output = Vec::new();
    let mut stack = Vec::new();

    for token in tokens {
        match token {
            Token::Number(_) => output.push(token),
            Token::Function(_) => stack.push(token),
            Token::Operator(op1) => {
                while let Some(top) = stack.last() {
                    match top {
                        Token::Operator(op2) => {
                            if precedence(op1) <= precedence(*op2) {
                                output.push(stack.pop().unwrap());
                            } else {
                                break;
                            }
                        }
                        Token::Function(_) => output.push(stack.pop().unwrap()),
                        _ => break,
                    }
                }
                stack.push(Token::Operator(op1));
            }
            Token::LeftParen => stack.push(token),
            Token::RightParen => {
                while let Some(top) = stack.last() {
                    if *top == Token::LeftParen {
                        break;
                    }
                    output.push(stack.pop().unwrap());
                }
                if stack.pop() != Some(Token::LeftParen) {
                    return None; // Eşleşmeyen parantez
                }
                if let Some(Token::Function(_)) = stack.last() {
                    output.push(stack.pop().unwrap());
                }
            }
        }
    }

    while let Some(token) = stack.pop() {
        if token == Token::LeftParen {
            return None; // Eşleşmeyen parantez
        }
        output.push(token);
    }

    Some(output)
}

fn precedence(op: char) -> u8 {
    match op {
        '+' | '-' => 1,
        '*' | '/' | '%' => 2,
        '^' => 3,
        _ => 0,
    }
}

fn eval_rpn(tokens: Vec<Token>) -> Option<f32> {
    let mut stack = Vec::new();

    for token in tokens {
        match token {
            Token::Number(n) => stack.push(n),
            Token::Operator(op) => {
                let b = stack.pop()?;
                let a = stack.pop()?;
                let res = match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    '/' => a / b,
                    '%' => a % b,
                    '^' => powf(a, b),
                    _ => return None,
                };
                stack.push(res);
            }
            Token::Function(func) => {
                let a = stack.pop()?;
                let res = match func.as_str() {
                    "sin" => sinf(a),
                    "cos" => cosf(a),
                    "tan" => tanf(a),
                    "sqrt" => sqrtf(a),
                    "log" => log10f(a),
                    "abs" => fabsf(a),
                    _ => return None,
                };
                stack.push(res);
            }
            _ => return None,
        }
    }

    if stack.len() == 1 {
        stack.pop()
    } else {
        None
    }
}
