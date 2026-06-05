use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;

use crate::shell_syscall as sc;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Word(String),
    Pipe,
    Semi,
    And,    // &&
    Or,     // ||
    RedirectOut,     // >
    RedirectAppend,  // >>
    RedirectIn,      // <
    RedirectHere,    // <<
    RedirectHereStrip, // <<-
    RedirectErr,     // 2>
    RedirectErrAppend, // 2>>
    RedirectAll,     // &>
    Ampersand,       // & (background)
    LParen, RParen,
    LBrace, RBrace,
    DoubleBracket,     // [[
    DoubleBracketClose,// ]]
    DoubleParen,       // ((
    DoubleParenClose,  // ))
    HereString,        // <<<
    Clobber,           // >|
    RedirectReadWrite, // <>
    Newline,
}

pub fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        match chars[i] {
            ' ' | '\t' => { i += 1; }
            '\n' => { tokens.push(Token::Newline); i += 1; }
            '\\' if i + 1 < len && chars[i + 1] == '\n' => { i += 2; }
            '\\' if i + 1 < len => {
                i += 1;
                let mut w = String::new();
                w.push(chars[i]);
                i += 1;
                while i < len && !is_delimiter(chars[i]) {
                    if chars[i] == '\\' && i + 1 < len { i += 1; w.push(chars[i]); }
                    else { w.push(chars[i]); }
                    i += 1;
                }
                tokens.push(Token::Word(w));
            }
            '#' => { while i < len && chars[i] != '\n' { i += 1; } }
            '\'' => {
                i += 1;
                let mut w = String::new();
                while i < len && chars[i] != '\'' { w.push(chars[i]); i += 1; }
                if i < len { i += 1; }
                tokens.push(Token::Word(w));
            }
            '"' => {
                i += 1;
                let mut w = String::new();
                while i < len && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < len { i += 1; w.push(chars[i]); }
                    else { w.push(chars[i]); }
                    i += 1;
                }
                if i < len { i += 1; }
                tokens.push(Token::Word(w));
            }
            '|' => { if i + 1 < len && chars[i + 1] == '|' { tokens.push(Token::Or); i += 2; } else { tokens.push(Token::Pipe); i += 1; } }
            '&' => {
                if i + 1 < len && chars[i + 1] == '>' { tokens.push(Token::RedirectAll); i += 2; }
                else if i + 1 < len && chars[i + 1] == '&' { tokens.push(Token::And); i += 2; }
                else { tokens.push(Token::Ampersand); i += 1; }
            }
            ';' => { tokens.push(Token::Semi); i += 1; }
            '(' => {
                if i + 1 < len && chars[i + 1] == '(' { tokens.push(Token::DoubleParen); i += 2; }
                else { tokens.push(Token::LParen); i += 1; }
            }
            ')' => {
                if i + 1 < len && chars[i + 1] == ')' { tokens.push(Token::DoubleParenClose); i += 2; }
                else { tokens.push(Token::RParen); i += 1; }
            }
            '{' => { tokens.push(Token::LBrace); i += 1; }
            '}' => { tokens.push(Token::RBrace); i += 1; }
            '[' => {
                if i + 1 < len && chars[i + 1] == '[' { tokens.push(Token::DoubleBracket); i += 2; }
                else { let mut w = String::new(); w.push('['); i += 1; tokens.push(Token::Word(w)); }
            }
            ']' => {
                if i + 1 < len && chars[i + 1] == ']' { tokens.push(Token::DoubleBracketClose); i += 2; }
                else { let mut w = String::new(); w.push(']'); i += 1; tokens.push(Token::Word(w)); }
            }
            '>' => {
                if i + 1 < len && chars[i + 1] == '|' { tokens.push(Token::Clobber); i += 2; }
                else if i + 1 < len && chars[i + 1] == '>' { tokens.push(Token::RedirectAppend); i += 2; }
                else { tokens.push(Token::RedirectOut); i += 1; }
            }
            '<' => {
                if i + 2 < len && chars[i + 1] == '<' && chars[i + 2] == '<' { tokens.push(Token::HereString); i += 3; }
                else if i + 1 < len && chars[i + 1] == '<' && i + 2 < len && chars[i + 2] == '-' { tokens.push(Token::RedirectHereStrip); i += 3; }
                else if i + 1 < len && chars[i + 1] == '<' { tokens.push(Token::RedirectHere); i += 2; }
                else if i + 1 < len && chars[i + 1] == '>' { tokens.push(Token::RedirectReadWrite); i += 2; }
                else { tokens.push(Token::RedirectIn); i += 1; }
            }
            '2' if i + 1 < len && chars[i + 1] == '>' => {
                if i + 2 < len && chars[i + 2] == '&' {
                    let mut target = String::from("2>&");
                    i += 3;
                    while i < len && (chars[i].is_ascii_digit() || chars[i] == '-') { target.push(chars[i]); i += 1; }
                    tokens.push(Token::Word(target));
                } else if i + 2 < len && chars[i + 2] == '>' { tokens.push(Token::RedirectErrAppend); i += 3; }
                else { tokens.push(Token::RedirectErr); i += 2; }
            }
            '`' => {
                i += 1;
                let mut cmd = String::new();
                while i < len && chars[i] != '`' {
                    if chars[i] == '\\' && i + 1 < len { i += 1; cmd.push(chars[i]); }
                    else { cmd.push(chars[i]); }
                    i += 1;
                }
                if i < len { i += 1; }
                tokens.push(Token::Word(cmd));
            }
            _ => {
                let mut w = String::new();
                let mut in_dquote = false;
                let mut in_squote = false;
                if !in_squote && !in_dquote && i + 1 < len && (chars[i] == '*' || chars[i] == '+' || chars[i] == '@' || chars[i] == '!')
                    && chars[i + 1] == '(' {
                    let opener = chars[i];
                    i += 2;
                    let mut depth = 1;
                    w.push(opener);
                    w.push('(');
                    while i < len && depth > 0 {
                        let c = chars[i];
                        if c == '(' { depth += 1; w.push(c); i += 1; }
                        else if c == ')' { depth -= 1; if depth > 0 { w.push(c); } i += 1; }
                        else if c == '\\' && i + 1 < len { i += 1; w.push(chars[i]); i += 1; }
                        else { w.push(c); i += 1; }
                    }
                    if !w.is_empty() { tokens.push(Token::Word(w)); }
                } else {
                while i < len {
                    let c = chars[i];
                    if c == '\'' && !in_dquote { in_squote = !in_squote; i += 1; continue; }
                    if c == '"' && !in_squote { in_dquote = !in_dquote; i += 1; continue; }
                    if !in_squote && !in_dquote {
                        if is_delimiter(c) { break; }
                        if c == '\\' && i + 1 < len { i += 1; w.push(chars[i]); i += 1; continue; }
                    }
                    w.push(c);
                    i += 1;
                }
                if !w.is_empty() { tokens.push(Token::Word(w)); }
                }
            }
        }
    }
    tokens
}

fn is_delimiter(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '|' | ';' | '&' | '>' | '<' | '(' | ')' | '{' | '}')
}

// ============================================================================
// GLOB EXPANSION — *, ?, []
// ============================================================================

pub fn expand_globs(args: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    for arg in args {
        if arg.starts_with("@(") || arg.starts_with("+(") || arg.starts_with("*(") || arg.starts_with("!(") {
            let expanded = expand_extglob(arg);
            if expanded.is_empty() {
                result.push(arg.clone());
            } else {
                for m in expanded { result.push(m); }
            }
        } else if arg.contains('*') || arg.contains('?') || arg.contains('[') {
            let matches = glob_expand(arg);
            if matches.is_empty() {
                result.push(arg.clone());
            } else {
                for m in matches { result.push(m); }
            }
        } else {
            result.push(arg.clone());
        }
    }
    result
}

fn parse_extglob(pattern: &str) -> Option<(char, Vec<String>)> {
    let bytes = pattern.as_bytes();
    if bytes.len() < 3 { return None; }
    let op = bytes[0] as char;
    if !matches!(op, '@' | '+' | '*' | '!') || bytes[1] != b'(' { return None; }
    let mut depth = 1;
    let mut i = 2;
    while i < bytes.len() && depth > 0 {
        if bytes[i] == b'(' { depth += 1; }
        else if bytes[i] == b')' { depth -= 1; }
        i += 1;
    }
    if depth != 0 { return None; }
    let content = &pattern[2..i - 1];
    let mut alternatives = Vec::new();
    let mut current = String::new();
    let mut d = 0;
    for c in content.chars() {
        match c {
            '(' => { d += 1; current.push(c); }
            ')' => { d -= 1; current.push(c); }
            '|' if d == 0 => { alternatives.push(current.clone()); current.clear(); }
            _ => { current.push(c); }
        }
    }
    if !current.is_empty() { alternatives.push(current); }
    Some((op, alternatives))
}

fn expand_extglob(pattern: &str) -> Vec<String> {
    let mut result = Vec::new();
    if let Some((op, alternatives)) = parse_extglob(pattern) {
        match op {
            '@' => {
                for alt in &alternatives {
                    if alt.contains('*') || alt.contains('?') || alt.contains('[') {
                        let m = glob_expand(alt);
                        for x in m { result.push(x); }
                    } else {
                        result.push(alt.clone());
                    }
                }
            }
            '+' => {
                for alt in &alternatives {
                    if alt.contains('*') || alt.contains('?') || alt.contains('[') {
                        let m = glob_expand(alt);
                        for x in m { result.push(x); }
                    } else {
                        result.push(alt.clone());
                    }
                }
            }
            '!' => {
                let mut excluded = Vec::new();
                for alt in &alternatives {
                    if alt.contains('*') || alt.contains('?') || alt.contains('[') {
                        let m = glob_expand(alt);
                        for x in m { excluded.push(x); }
                    } else {
                        excluded.push(alt.clone());
                    }
                }
                let mut buf = [0u8; 8192];
                if let Ok(fd) = sc::sys_open(".", 0) {
                    if let Ok(n) = sc::sys_getdents64(fd, &mut buf) {
                        let mut offset = 0;
                        while offset < n {
                            let rec_len = u16::from_le_bytes([buf[offset + 8], buf[offset + 9]]) as usize;
                            let name_len = u16::from_le_bytes([buf[offset + 10], buf[offset + 11]]) as usize;
                            if name_len > 0 {
                                let name_bytes = &buf[offset + 18..offset + 18 + name_len];
                                if let Ok(name) = core::str::from_utf8(name_bytes) {
                                    let name = name.trim_end_matches('\0');
                                    if name != "." && name != ".." && !excluded.contains(&name.to_string()) {
                                        result.push(name.to_string());
                                    }
                                }
                            }
                            if rec_len == 0 { break; }
                            offset += rec_len;
                        }
                    }
                    let _ = sc::sys_close(fd);
                }
            }
            '*' => {
                for alt in &alternatives {
                    if alt.contains('*') || alt.contains('?') || alt.contains('[') {
                        let m = glob_expand(alt);
                        for x in &m { result.push(x.clone()); }
                        if !m.is_empty() {
                            let mut expanded = true;
                            while expanded {
                                expanded = false;
                                let mut new_matches = Vec::new();
                                for existing in &result {
                                    for a in alternatives.iter() {
                                        let candidate = format!("{}/{}", existing, a);
                                        if a.contains('*') || a.contains('?') || a.contains('[') {
                                            let m = glob_expand(&candidate);
                                            if !m.is_empty() {
                                                for x in m {
                                                    if !result.contains(&x) && !new_matches.contains(&x) {
                                                        new_matches.push(x.clone());
                                                        expanded = true;
                                                    }
                                                }
                                            }
                                        } else {
                                            if !result.contains(&candidate) && !new_matches.contains(&candidate) {
                                                new_matches.push(candidate.clone());
                                                expanded = true;
                                            }
                                        }
                                    }
                                }
                                for x in new_matches { result.push(x); }
                            }
                        }
                    } else {
                        result.push(alt.clone());
                        let mut prev_count = 0;
                        let mut current = result.clone();
                        while current.len() > prev_count {
                            prev_count = current.len();
                            let mut new_items = Vec::new();
                            for item in &current {
                                let candidate = format!("{}/{}", item, alt);
                                if !current.contains(&candidate) && !new_items.contains(&candidate) {
                                    new_items.push(candidate);
                                }
                            }
                            for x in new_items { current.push(x.clone()); result.push(x); }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    result.sort();
    result.dedup();
    result
}

fn glob_expand(pattern: &str) -> Vec<String> {
    if pattern.contains("**") {
        return glob_expand_recursive(pattern);
    }
    let (dir, file_pattern) = if let Some(pos) = pattern.rfind('/') {
        (&pattern[..pos + 1], &pattern[pos + 1..])
    } else {
        ("", pattern)
    };
    let prefix = if dir.is_empty() { "./" } else { dir };
    let mut matches = Vec::new();
    let mut buf = [0u8; 8192];
    if let Ok(fd) = sc::sys_open(prefix.trim_end_matches('/'), 0) {
        if let Ok(n) = sc::sys_getdents64(fd, &mut buf) {
            let mut offset = 0;
            while offset < n {
                let rec_len = u16::from_le_bytes([buf[offset + 8], buf[offset + 9]]) as usize;
                let name_len = u16::from_le_bytes([buf[offset + 10], buf[offset + 11]]) as usize;
                if name_len > 0 {
                    let name_bytes = &buf[offset + 18..offset + 18 + name_len];
                    if let Ok(name) = core::str::from_utf8(name_bytes) {
                        let name = name.trim_end_matches('\0');
                        if name != "." && name != ".." && glob_match(name, file_pattern) {
                            matches.push(format!("{}{}", prefix, name));
                        }
                    }
                }
                if rec_len == 0 { break; }
                offset += rec_len;
            }
        }
        let _ = sc::sys_close(fd);
    }
    matches.sort();
    matches
}

fn glob_expand_recursive(pattern: &str) -> Vec<String> {
    let mut matches = Vec::new();
    let (prefix, suffix) = if let Some(pos) = pattern.find("**") {
        let after_stars = if pos + 2 < pattern.len() && pattern.as_bytes()[pos + 2] == b'/' {
            pos + 3
        } else {
            pos + 2
        };
        (&pattern[..pos], &pattern[after_stars..])
    } else {
        return glob_expand(pattern);
    };
    let base = if prefix.is_empty() { "." } else { prefix.trim_end_matches('/') };
    let mut stack = Vec::new();
    stack.push(base.to_string());
    let mut buf = [0u8; 8192];
    while let Some(dir) = stack.pop() {
        if let Ok(fd) = sc::sys_open(&dir, 0) {
            if let Ok(n) = sc::sys_getdents64(fd, &mut buf) {
                let mut offset = 0;
                while offset < n {
                    let rec_len = u16::from_le_bytes([buf[offset + 8], buf[offset + 9]]) as usize;
                    let name_len = u16::from_le_bytes([buf[offset + 10], buf[offset + 11]]) as usize;
                    let d_type = buf[offset + 16];
                    if name_len > 0 {
                        let name_bytes = &buf[offset + 18..offset + 18 + name_len];
                        if let Ok(name) = core::str::from_utf8(name_bytes) {
                            let name = name.trim_end_matches('\0');
                            if name != "." && name != ".." {
                                let full_path = if dir == "." {
                                    name.to_string()
                                } else {
                                    format!("{}/{}", dir, name)
                                };
                                let is_dir = d_type == 4;
                                if !suffix.is_empty() && !is_dir && glob_match(name, suffix) {
                                    matches.push(full_path.clone());
                                }
                                if is_dir {
                                    stack.push(full_path);
                                }
                            }
                        }
                    }
                    if rec_len == 0 { break; }
                    offset += rec_len;
                }
            }
            let _ = sc::sys_close(fd);
        }
    }
    matches.sort();
    matches.dedup();
    matches
}

fn glob_match(name: &str, pattern: &str) -> bool {
    let name_chars: Vec<char> = name.chars().collect();
    let pat_chars: Vec<char> = pattern.chars().collect();
    glob_match_inner(&name_chars, &pat_chars, 0, 0)
}

fn glob_match_inner(name: &[char], pat: &[char], ni: usize, pi: usize) -> bool {
    if pi == pat.len() { return ni == name.len(); }
    if pat[pi] == '*' {
        if pi + 1 < pat.len() && pat[pi + 1] == '*' && (pi + 2 >= pat.len() || pat[pi + 2] == '/') {
            let next_pat = if pi + 2 < pat.len() && pat[pi + 2] == '/' { pi + 3 } else { pi + 2 };
            for skip in 0..=name.len() - ni {
                if glob_match_inner(name, pat, ni + skip, next_pat) { return true; }
            }
            return false;
        }
        for skip in 0..=name.len() - ni {
            if glob_match_inner(name, pat, ni + skip, pi + 1) { return true; }
        }
        return false;
    }
    if pat[pi] == '?' { return ni < name.len() && glob_match_inner(name, pat, ni + 1, pi + 1); }
    if pat[pi] == '[' {
        let mut j = pi + 1;
        let mut negate = false;
        if j < pat.len() && pat[j] == '^' { negate = true; j += 1; }
        if j < pat.len() && pat[j] == ']' { j += 1; }
        let mut found = false;
        while j < pat.len() && pat[j] != ']' {
            if j + 2 < pat.len() && pat[j + 1] == '-' {
                if ni < name.len() && name[ni] >= pat[j] && name[ni] <= pat[j + 2] { found = true; }
                j += 3;
            } else {
                if ni < name.len() && name[ni] == pat[j] { found = true; }
                j += 1;
            }
        }
        if found == negate { return false; }
        return ni < name.len() && glob_match_inner(name, pat, ni + 1, j + 1);
    }
    ni < name.len() && name[ni] == pat[pi] && glob_match_inner(name, pat, ni + 1, pi + 1)
}

// ============================================================================
// BRACE EXPANSION — {a,b,c} ve {1..10}
// ============================================================================

pub fn expand_braces(input: &str) -> String {
    if let Some(start) = input.find('{') {
        if let Some(end) = input[start..].find('}') {
            let brace_content = &input[start + 1..start + end];
            let before = &input[..start];
            let after = &input[start + end + 1..];
            if brace_content.contains(',') {
                let items: Vec<&str> = brace_content.split(',').collect();
                let mut result = String::new();
                for (i, item) in items.iter().enumerate() {
                    if i > 0 { result.push(' '); }
                    result.push_str(before);
                    result.push_str(item);
                    result.push_str(after);
                }
                return expand_braces(&result);
            }
            if let Some(dot_pos) = brace_content.find("..") {
                let start_str = &brace_content[..dot_pos];
                let end_str = &brace_content[dot_pos + 2..];
                if let (Ok(s), Ok(e)) = (start_str.parse::<i64>(), end_str.parse::<i64>()) {
                    let mut result = String::new();
                    let mut first = true;
                    if s <= e {
                        for i in s..=e {
                            if !first { result.push(' '); }
                            result.push_str(before);
                            result.push_str(&format!("{}", i));
                            result.push_str(after);
                            first = false;
                        }
                    } else {
                        for i in (e..=s).rev() {
                            if !first { result.push(' '); }
                            result.push_str(before);
                            result.push_str(&format!("{}", i));
                            result.push_str(after);
                            first = false;
                        }
                    }
                    return expand_braces(&result);
                }
            }
        }
    }
    input.to_string()
}

// ============================================================================
// HERE DOCUMENT
// ============================================================================

pub fn read_heredoc(delimiter: &str, strip_tabs: bool) -> String {
    let mut content = String::new();
    loop {
        let Some(line) = crate::read_line("", None) else { break; };
        let trimmed = if strip_tabs {
            line.trim_start_matches('\t').to_string()
        } else {
            line
        };
        if trimmed.trim() == delimiter { break; }
        content.push_str(&trimmed);
        content.push('\n');
    }
    content
}

// ============================================================================
// ParsedCommand
// ============================================================================

#[derive(Debug, Clone)]
pub struct ParsedCommand {
    pub argv: Vec<String>,
    pub redirect_out: Option<String>,
    pub redirect_append: Option<String>,
    pub redirect_in: Option<String>,
    pub redirect_err: Option<String>,
    pub redirect_err_append: Option<String>,
    pub redirect_readwrite: Option<String>,
    pub here_doc: Option<String>,
    pub here_doc_strip: bool,
    pub here_string: Option<String>,
    pub background: bool,
    pub clobber: bool,
}

pub fn parse_simple(tokens: &[Token]) -> Vec<ParsedCommand> {
    let mut commands = Vec::new();
    let mut current = ParsedCommand {
        argv: Vec::new(),
        redirect_out: None, redirect_append: None,
        redirect_in: None, redirect_err: None,
        redirect_err_append: None, redirect_readwrite: None,
        here_doc: None,
        here_doc_strip: false, here_string: None,
        background: false, clobber: false,
    };
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Word(w) => { current.argv.push(w.clone()); }
            Token::Pipe | Token::Semi | Token::And | Token::Or | Token::Newline => {
                if !current.argv.is_empty() || current.redirect_out.is_some() || current.redirect_in.is_some() || current.here_doc.is_some() || current.here_string.is_some() {
                    commands.push(current.clone());
                }
                current = ParsedCommand {
                    argv: Vec::new(),
                    redirect_out: None, redirect_append: None,
                    redirect_in: None, redirect_err: None,
                    redirect_err_append: None, redirect_readwrite: None,
                    here_doc: None,
                    here_doc_strip: false, here_string: None,
                    background: false, clobber: false,
                };
            }
            Token::RedirectOut => { if let Some(Token::Word(f)) = tokens.get(i + 1) { current.redirect_out = Some(f.clone()); i += 1; } }
            Token::RedirectAppend => { if let Some(Token::Word(f)) = tokens.get(i + 1) { current.redirect_append = Some(f.clone()); i += 1; } }
            Token::RedirectIn => { if let Some(Token::Word(f)) = tokens.get(i + 1) { current.redirect_in = Some(f.clone()); i += 1; } }
            Token::RedirectErr => { if let Some(Token::Word(f)) = tokens.get(i + 1) { current.redirect_err = Some(f.clone()); i += 1; } }
            Token::RedirectErrAppend => { if let Some(Token::Word(f)) = tokens.get(i + 1) { current.redirect_err_append = Some(f.clone()); i += 1; } }
            Token::RedirectAll => { if let Some(Token::Word(f)) = tokens.get(i + 1) { current.redirect_out = Some(f.clone()); current.redirect_err = Some(f.clone()); i += 1; } }
            Token::RedirectHere => {
                if let Some(Token::Word(delimiter)) = tokens.get(i + 1) {
                    let content = read_heredoc(delimiter, false);
                    current.here_doc = Some(content);
                    i += 1;
                }
            }
            Token::RedirectHereStrip => {
                if let Some(Token::Word(delimiter)) = tokens.get(i + 1) {
                    let content = read_heredoc(delimiter, true);
                    current.here_doc = Some(content);
                    current.here_doc_strip = true;
                    i += 1;
                }
            }
            Token::Clobber => {
                current.clobber = true;
                if let Some(Token::Word(f)) = tokens.get(i + 1) {
                    current.redirect_out = Some(f.clone());
                    i += 1;
                }
            }
            Token::RedirectReadWrite => {
                if let Some(Token::Word(f)) = tokens.get(i + 1) {
                    current.redirect_readwrite = Some(f.clone());
                    i += 1;
                }
            }
            Token::HereString => {
                if let Some(Token::Word(content)) = tokens.get(i + 1) {
                    current.here_string = Some(content.clone());
                    i += 1;
                }
            }
            Token::DoubleBracket | Token::DoubleBracketClose | Token::DoubleParen | Token::DoubleParenClose => {
                let cmd_str = collect_compound(tokens, i);
                current.argv.push(cmd_str.0);
                i = cmd_str.1;
            }
            Token::Ampersand => { current.background = true; }
            Token::LParen => {
                let mut depth = 1;
                let mut subshell_cmd = String::new();
                i += 1;
                while i < tokens.len() && depth > 0 {
                    match &tokens[i] {
                        Token::LParen => { depth += 1; subshell_cmd.push('('); }
                        Token::RParen => { depth -= 1; if depth > 0 { subshell_cmd.push(')'); } }
                        Token::Word(w) => { subshell_cmd.push_str(w); subshell_cmd.push(' '); }
                        Token::Pipe => subshell_cmd.push('|'),
                        Token::Semi => subshell_cmd.push_str("; "),
                        Token::And => subshell_cmd.push_str("&& "),
                        Token::Or => subshell_cmd.push_str("|| "),
                        Token::RedirectOut => subshell_cmd.push('>'),
                        Token::RedirectIn => subshell_cmd.push('<'),
                        Token::Newline => subshell_cmd.push('\n'),
                        _ => {}
                    }
                    i += 1;
                }
                current.argv.push("(subshell)".to_string());
                current.here_doc = Some(subshell_cmd);
            }
            Token::LBrace => {
                let mut depth = 1;
                let mut group_cmd = String::new();
                i += 1;
                while i < tokens.len() && depth > 0 {
                    match &tokens[i] {
                        Token::LBrace => { depth += 1; group_cmd.push('{'); }
                        Token::RBrace => { depth -= 1; if depth > 0 { group_cmd.push('}'); } }
                        Token::Word(w) => { group_cmd.push_str(w); group_cmd.push(' '); }
                        Token::Pipe => group_cmd.push('|'),
                        Token::Semi => group_cmd.push_str("; "),
                        Token::And => group_cmd.push_str("&& "),
                        Token::Or => group_cmd.push_str("|| "),
                        Token::RedirectOut => group_cmd.push('>'),
                        Token::RedirectIn => group_cmd.push('<'),
                        Token::Newline => group_cmd.push('\n'),
                        _ => {}
                    }
                    i += 1;
                }
                current.argv.push("{group}".to_string());
                current.here_doc = Some(group_cmd);
            }
            _ => {}
        }
        i += 1;
    }
    if !current.argv.is_empty() || current.redirect_out.is_some() || current.redirect_in.is_some() || current.here_doc.is_some() || current.here_string.is_some() {
        commands.push(current);
    }
    commands
}

fn collect_compound(tokens: &[Token], start: usize) -> (String, usize) {
    let mut depth = 1;
    let mut result = String::new();
    let mut i = start + 1;
    match &tokens[start] {
        Token::DoubleBracket => { result.push_str("[[ "); }
        Token::DoubleParen => { result.push_str("(( "); }
        _ => {}
    }
    while i < tokens.len() && depth > 0 {
        match &tokens[i] {
            Token::DoubleBracket => { depth += 1; result.push_str("[[ "); }
            Token::DoubleBracketClose => { depth -= 1; if depth > 0 { result.push_str("]] "); } }
            Token::DoubleParen => { depth += 1; result.push_str("(( "); }
            Token::DoubleParenClose => { depth -= 1; if depth > 0 { result.push_str(")) "); } }
            Token::Word(w) => { result.push_str(w); result.push(' '); }
            Token::Pipe => result.push('|'),
            Token::Semi => result.push_str("; "),
            Token::And => result.push_str("&& "),
            Token::Or => result.push_str("|| "),
            Token::RedirectOut => result.push('>'),
            Token::RedirectIn => result.push('<'),
            Token::Newline => result.push('\n'),
            _ => {}
        }
        i += 1;
    }
    let closing = match &tokens[start] {
        Token::DoubleBracket => "]]",
        Token::DoubleParen => "))",
        _ => "",
    };
    if !closing.is_empty() { result.push_str(closing); }
    (result, i)
}
