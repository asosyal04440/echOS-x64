use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;
use spin::Mutex;

pub struct ShellEnv {
    vars: Mutex<BTreeMap<String, String>>,
    arrays: Mutex<BTreeMap<String, Vec<String>>>,
    pub opts: Mutex<ShellOpts>,
}

pub struct ShellOpts {
    pub allexport: bool,
    pub braceexpand: bool,
    pub errexit: bool,
    pub exec: bool,
    pub hashall: bool,
    pub histexpand: bool,
    pub ignoreeof: bool,
    pub monitor: bool,
    pub noclobber: bool,
    pub noglob: bool,
    pub nounset: bool,
    pub pipefail: bool,
    pub posix: bool,
    pub verbose: bool,
    pub xtrace: bool,
    pub vi: bool,
    pub emacs: bool,
}

impl ShellOpts {
    pub const fn new() -> Self {
        Self {
            allexport: false, braceexpand: true, errexit: false, exec: true,
            hashall: true, histexpand: true, ignoreeof: false, monitor: true,
            noclobber: false, noglob: false, nounset: false, pipefail: false,
            posix: true, verbose: false, xtrace: false, vi: false, emacs: false,
        }
    }
    pub fn set_by_name(&mut self, name: &str, val: bool) -> bool {
        match name {
            "allexport" => { self.allexport = val; true }
            "braceexpand" => { self.braceexpand = val; true }
            "errexit" | "e" => { self.errexit = val; true }
            "exec" | "n" => { self.exec = val; true }
            "hashall" | "h" => { self.hashall = val; true }
            "histexpand" | "H" => { self.histexpand = val; true }
            "ignoreeof" => { self.ignoreeof = val; true }
            "monitor" | "m" => { self.monitor = val; true }
            "noclobber" | "C" => { self.noclobber = val; true }
            "noglob" | "f" => { self.noglob = val; true }
            "nounset" | "u" => { self.nounset = val; true }
            "pipefail" => { self.pipefail = val; true }
            "posix" => { self.posix = val; true }
            "verbose" | "v" => { self.verbose = val; true }
            "xtrace" | "x" => { self.xtrace = val; true }
            "vi" => { self.vi = val; true }
            "emacs" => { self.emacs = val; true }
            _ => false,
        }
    }
    pub fn get_by_name(&self, name: &str) -> Option<bool> {
        match name {
            "allexport" => Some(self.allexport),
            "braceexpand" => Some(self.braceexpand),
            "errexit" | "e" => Some(self.errexit),
            "exec" | "n" => Some(self.exec),
            "hashall" | "h" => Some(self.hashall),
            "histexpand" | "H" => Some(self.histexpand),
            "ignoreeof" => Some(self.ignoreeof),
            "monitor" | "m" => Some(self.monitor),
            "noclobber" | "C" => Some(self.noclobber),
            "noglob" | "f" => Some(self.noglob),
            "nounset" | "u" => Some(self.nounset),
            "pipefail" => Some(self.pipefail),
            "posix" => Some(self.posix),
            "verbose" | "v" => Some(self.verbose),
            "xtrace" | "x" => Some(self.xtrace),
            "vi" => Some(self.vi),
            "emacs" => Some(self.emacs),
            _ => None,
        }
    }
    pub fn to_flags_string(&self) -> String {
        let mut s = String::new();
        if self.allexport { s.push('a'); }
        if self.errexit { s.push('e'); }
        if self.noglob { s.push('f'); }
        if self.hashall { s.push('h'); }
        if self.histexpand { s.push('H'); }
        if self.monitor { s.push('m'); }
        if self.noclobber { s.push('C'); }
        if self.nounset { s.push('u'); }
        if self.verbose { s.push('v'); }
        if self.xtrace { s.push('x'); }
        if s.is_empty() { s.push_str("himxB"); }
        s
    }
}

unsafe impl Send for ShellEnv {}
unsafe impl Sync for ShellEnv {}

impl ShellEnv {
    pub const fn new() -> Self {
        Self {
            vars: Mutex::new(BTreeMap::new()),
            arrays: Mutex::new(BTreeMap::new()),
            opts: Mutex::new(ShellOpts::new()),
        }
    }
    pub fn set_array(&self, name: &str, values: Vec<String>) {
        self.arrays.lock().insert(name.to_string(), values);
    }
    pub fn get_array(&self, name: &str) -> Option<Vec<String>> {
        self.arrays.lock().get(name).cloned()
    }
    pub fn unset_array(&self, name: &str) {
        self.arrays.lock().remove(name);
    }
    pub fn array_len(&self, name: &str) -> usize {
        self.arrays.lock().get(name).map_or(0, |v| v.len())
    }
    pub fn array_get(&self, name: &str, index: usize) -> Option<String> {
        self.arrays.lock().get(name).and_then(|v| v.get(index).cloned())
    }
    pub fn array_set(&self, name: &str, index: usize, value: &str) {
        let mut arrays = self.arrays.lock();
        let arr = arrays.entry(name.to_string()).or_insert_with(Vec::new);
        if index >= arr.len() { arr.resize(index + 1, String::new()); }
        arr[index] = value.to_string();
    }
    pub fn array_push(&self, name: &str, value: &str) {
        let mut arrays = self.arrays.lock();
        let arr = arrays.entry(name.to_string()).or_insert_with(Vec::new);
        arr.push(value.to_string());
    }
    pub fn set_pipestatus(&self, statuses: &[i32]) {
        let arr: Vec<String> = statuses.iter().map(|s| format!("{}", s)).collect();
        self.arrays.lock().insert(String::from("PIPESTATUS"), arr.clone());
        // Also set scalar to last element for $PIPESTATUS without index
        if let Some(last) = arr.last() {
            self.vars.lock().insert(String::from("PIPESTATUS"), last.clone());
        }
    }

    pub fn set_bash_rematch(&self, matches: Vec<String>) {
        self.arrays.lock().insert(String::from("BASH_REMATCH"), matches.clone());
        // Also set scalar to first match (BASH_REMATCH[0]) for $BASH_REMATCH without index
        if let Some(first) = matches.first() {
            self.vars.lock().insert(String::from("BASH_REMATCH"), first.clone());
        }
    }

    pub fn get_pipestatus(&self) -> Vec<String> {
        self.arrays.lock().get("PIPESTATUS").cloned().unwrap_or_default()
    }

    pub fn array_slice(&self, name: &str, offset: usize, length: Option<usize>) -> Vec<String> {
        let arrays = self.arrays.lock();
        if let Some(arr) = arrays.get(name) {
            let start = core::cmp::min(offset, arr.len());
            let end = match length {
                Some(len) => core::cmp::min(start + len, arr.len()),
                None => arr.len(),
            };
            arr[start..end].to_vec()
        } else {
            Vec::new()
        }
    }
    pub fn list_arrays(&self) -> Vec<(String, Vec<String>)> {
        self.arrays.lock().iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
    pub fn set(&mut self, key: &str, value: &str) {
        self.vars.lock().insert(key.to_string(), value.to_string());
    }
    pub fn get(&self, key: &str) -> Option<String> {
        self.vars.lock().get(key).cloned()
    }
    pub fn unset(&mut self, key: &str) {
        self.vars.lock().remove(key);
    }
    pub fn list(&self) -> Vec<(String, String)> {
        self.vars.lock().iter().map(|(k, v): (&String, &String)| (k.clone(), v.clone())).collect()
    }

    pub fn expand(&self, input: &str) -> String {
        let mut result = String::new();
        let chars: Vec<char> = input.chars().collect();
        let len = chars.len();
        let mut i = 0;

        while i < len {
            if chars[i] == '$' && i + 1 < len {
                i += 1;
                if chars[i] == '{' {
                    i += 1;
                    let exp_result = self.expand_brace_param(&chars, &mut i);
                    result.push_str(&exp_result);
                } else if chars[i] == '(' && i + 1 < len && chars[i + 1] == '(' {
                    i += 2;
                    let (val, new_i) = self.eval_arith_expansion(&chars, i);
                    result.push_str(&val);
                    i = new_i;
                } else if chars[i] == '(' {
                    i += 1;
                    let (val, new_i) = self.eval_cmd_substitution(&chars, i);
                    result.push_str(&val);
                    i = new_i;
                } else if chars[i] == '?' {
                    i += 1;
                    // $? — son çıkış kodu
                    if let Some(code) = self.get("?") {
                        result.push_str(&code);
                    } else {
                        result.push_str("0");
                    }
                } else if chars[i] == '$' {
                    i += 1;
                    result.push_str(&format!("{}", crate::shell_syscall::sys_getpid()));
                } else if chars[i] == '!' {
                    i += 1;
                    if let Some(bg) = self.get("_bg_pid") { result.push_str(&bg); }
                } else if chars[i] == '@' || chars[i] == '*' {
                    i += 1;
                    // $@ ve $* — tüm pozisyonel parametreleri boşlukla birleştirerek genişlet
                    let mut params = Vec::new();
                    let mut idx = 1u32;
                    while let Some(val) = self.get(&format!("{}", idx)) {
                        params.push(val);
                        idx += 1;
                    }
                    result.push_str(&params.join(" "));
                } else if chars[i] == '#' {
                    i += 1;
                    let mut count = 0u32;
                    let mut idx = 1u32;
                    while self.get(&format!("{}", idx)).is_some() { count += 1; idx += 1; }
                    result.push_str(&format!("{}", count));
                } else if chars[i] == '-' {
                    i += 1;
                    let opts = self.opts.lock().to_flags_string();
                    result.push_str(&opts);
                } else if chars[i] == '0' {
                    i += 1;
                    if let Some(name) = self.get("_shell_name") { result.push_str(&name); }
                    else { result.push_str("echshell"); }
                } else if matches_special_var(&chars, i, "EPOCHSECONDS") {
                    i += "EPOCHSECONDS".len();
                    let ts = crate::shell_syscall::sys_time();
                    result.push_str(&format!("{}", ts));
                } else if matches_special_var(&chars, i, "EPOCHREALTIME") {
                    i += "EPOCHREALTIME".len();
                    let ts = crate::shell_syscall::sys_time();
                    result.push_str(&format!("{}.0", ts));
                } else if matches_special_var(&chars, i, "RANDOM") {
                    i += "RANDOM".len();
                    let ts = crate::shell_syscall::sys_time();
                    let rand_val = ((ts as u32).wrapping_mul(1103515245).wrapping_add(12345) >> 16) & 0x7FFF;
                    result.push_str(&format!("{}", rand_val));
                } else if matches_special_var(&chars, i, "LINENO") {
                    i += "LINENO".len();
                    if let Some(lineno) = self.get("LINENO") { result.push_str(&lineno); }
                    else { result.push_str("1"); }
                } else if matches_special_var(&chars, i, "HOSTNAME") {
                    i += "HOSTNAME".len();
                    let mut buf = [0u8; 256];
                    match crate::shell_syscall::sys_eon_get_hostname(&mut buf) {
                        Ok(n) => {
                            if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                                result.push_str(s);
                            } else {
                                result.push_str("echos");
                            }
                        }
                        Err(_) => { result.push_str("echos"); }
                    }
                } else if chars[i].is_ascii_digit() {
                    let mut name = String::new();
                    while i < len && chars[i].is_ascii_digit() { name.push(chars[i]); i += 1; }
                    if let Some(val) = self.get(&name) { result.push_str(&val); }
                } else if chars[i].is_alphabetic() || chars[i] == '_' {
                    let mut name = String::new();
                    while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                        name.push(chars[i]); i += 1;
                    }
                    if name.ends_with('[') {
                        name.pop();
                        let mut index = String::new();
                        while i < len && chars[i] != ']' { index.push(chars[i]); i += 1; }
                        if i < len { i += 1; }
                        if index == "@" || index == "*" {
                            if let Some(arr) = self.get_array(&name) {
                                let expanded: Vec<String> = arr.iter().map(|v| self.expand(v)).collect();
                                result.push_str(&expanded.join(" "));
                            }
                        } else if index == "#" {
                            result.push_str(&format!("{}", self.array_len(&name)));
                        } else if let Some(idx) = index.strip_prefix('@') {
                            if self.get_array(&name).is_some() {
                                let offset: usize = idx.parse().unwrap_or(0);
                                let slice = self.array_slice(&name, offset, None);
                                let expanded: Vec<String> = slice.iter().map(|v| self.expand(v)).collect();
                                result.push_str(&expanded.join(" "));
                            }
                        } else if let Ok(idx) = index.parse::<usize>() {
                            if let Some(val) = self.array_get(&name, idx) {
                                result.push_str(&self.expand(&val));
                            }
                        }
                    } else {
                        if let Some(val) = self.get(&name) { result.push_str(&val); }
                    }
                }
            } else if chars[i] == '~' && (i == 0 || (i > 0 && chars[i-1] == '=' || chars[i-1] == ':')) {
                i += 1;
                let mut user = String::new();
                while i < len && chars[i] != '/' && chars[i] != ' ' && chars[i] != '\t' {
                    user.push(chars[i]); i += 1;
                }
                if user.is_empty() {
                    if let Some(home) = self.get("HOME") { result.push_str(&home); }
                    else { result.push('/'); }
                } else {
                    result.push_str(&format!("/home/{}", user));
                }
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }
        result
    }

    fn expand_brace_param(&self, chars: &[char], i: &mut usize) -> String {
        // ${!var} — indirect expansion (bash extension)
        let mut indirect = false;
        if *i < chars.len() && chars[*i] == '!' {
            indirect = true;
            *i += 1;
        }
        let mut param = String::new();
        while *i < chars.len() && !matches!(chars[*i], '}' | ':' | '-' | '+' | '=' | '?' | '#' | '%' | '/' | '[') {
            param.push(chars[*i]);
            *i += 1;
        }
        if *i >= chars.len() {
            if indirect {
                let ind_name = self.get(&param).unwrap_or_default();
                return self.get(&ind_name).unwrap_or_default();
            }
            return self.get(&param).unwrap_or_default();
        }

        let op = chars[*i];
        if op == '}' {
            *i += 1;
            if indirect {
                let ind_name = self.get(&param).unwrap_or_default();
                return self.get(&ind_name).unwrap_or_default();
            }
            return self.get(&param).unwrap_or_default();
        }

        if *i + 1 < chars.len() && chars[*i] == '}' {
            let next = chars[*i + 1];
            if next == '}' {
                *i += 1;
                if indirect {
                    let ind_name = self.get(&param).unwrap_or_default();
                    return self.get(&ind_name).unwrap_or_default();
                }
                return self.get(&param).unwrap_or_default();
            }
        }

        if *i + 1 < chars.len() {
            let c1 = chars[*i];
            let c2 = chars[*i + 1];
            if c1 == '^' && c2 == '^' {
                *i += 2;
                if *i < chars.len() && chars[*i] == '}' { *i += 1; }
                let val = self.get(&param).unwrap_or_default();
                return val.to_uppercase();
            }
            if c1 == ',' && c2 == ',' {
                *i += 2;
                if *i < chars.len() && chars[*i] == '}' { *i += 1; }
                let val = self.get(&param).unwrap_or_default();
                return val.to_lowercase();
            }
            if c1 == '@' {
                let modifier = c2;
                *i += 2;
                if *i < chars.len() && chars[*i] == '}' { *i += 1; }
                let val = self.get(&param).unwrap_or_default();
                match modifier {
                    'Q' => {
                        let mut escaped = String::from("'");
                        for ch in val.chars() {
                            if ch == '\'' {
                                escaped.push_str("'\\''");
                            } else {
                                escaped.push(ch);
                            }
                        }
                        escaped.push('\'');
                        return escaped;
                    }
                    'P' => {
                        return format!("[{}]", val);
                    }
                    'A' => {
                        let mut result = String::new();
                        for ch in val.chars() {
                            match ch {
                                '\'' => result.push_str("'\\''"),
                                '\\' => result.push_str("\\\\"),
                                '"' => result.push_str("\\\""),
                                '$' => result.push_str("\\$"),
                                '`' => result.push_str("\\`"),
                                '!' => result.push_str("\\!"),
                                _ => result.push(ch),
                            }
                        }
                        return result;
                    }
                    'a' => {
                        let mut result = String::new();
                        for ch in val.chars() {
                            match ch {
                                '\x07' => result.push_str("\\a"),
                                '\x08' => result.push_str("\\b"),
                                '\x1b' => result.push_str("\\e"),
                                '\x0C' => result.push_str("\\f"),
                                '\n' => result.push_str("\\n"),
                                '\r' => result.push_str("\\r"),
                                '\t' => result.push_str("\\t"),
                                '\x0B' => result.push_str("\\v"),
                                '\\' => result.push_str("\\\\"),
                                _ => result.push(ch),
                            }
                        }
                        return result;
                    }
                    _ => {
                        return format!("{}{}", c1, c2);
                    }
                }
            }
        }

        if op == '[' {
            *i += 1;
            let mut index = String::new();
            while *i < chars.len() && chars[*i] != ']' { index.push(chars[*i]); *i += 1; }
            if *i < chars.len() { *i += 1; }
            if *i < chars.len() && chars[*i] == '}' { *i += 1; }
            if index == "@" || index == "*" {
                if let Some(arr) = self.get_array(&param) {
                    let expanded: Vec<String> = arr.iter().map(|v| self.expand(v)).collect();
                    return expanded.join(" ");
                }
            } else if index == "#" {
                return format!("{}", self.array_len(&param));
            } else if let Some(idx) = index.strip_prefix('@') {
                if self.get_array(&param).is_some() {
                    let offset: usize = idx.parse().unwrap_or(0);
                    let slice = self.array_slice(&param, offset, None);
                    let expanded: Vec<String> = slice.iter().map(|v| self.expand(v)).collect();
                    return expanded.join(" ");
                }
            } else if let Ok(idx) = index.parse::<usize>() {
                if let Some(val) = self.array_get(&param, idx) {
                    return self.expand(&val);
                }
            }
            return String::new();
        }

        if op == '/' {
            *i += 1;
            let mut all_matches = false;
            if *i < chars.len() && chars[*i] == '/' { all_matches = true; *i += 1; }
            let mut pattern = String::new();
            while *i < chars.len() && chars[*i] != '/' && chars[*i] != '}' { pattern.push(chars[*i]); *i += 1; }
            let mut replacement = String::new();
            if *i < chars.len() && chars[*i] == '/' { *i += 1; }
            while *i < chars.len() && chars[*i] != '}' { replacement.push(chars[*i]); *i += 1; }
            if *i < chars.len() { *i += 1; }
            let val = self.get(&param).unwrap_or_default();
            if pattern.is_empty() { return val; }
            if all_matches {
                let pattern = self.expand(&pattern);
                let replacement = self.expand(&replacement);
                return replace_all_fn(&val, &pattern, &replacement);
            } else {
                let pattern = self.expand(&pattern);
                let replacement = self.expand(&replacement);
                return replace_first(&val, &pattern, &replacement);
            }
        }

        let has_colon = *i + 1 < chars.len() && chars[*i + 1] == ':';
        if has_colon { *i += 1; }

        let mut greedy = false;
        if *i + 1 < chars.len() && chars[*i + 1] == op {
            greedy = true;
            *i += 1;
        }

        *i += 1;

        let mut word = String::new();
        let mut depth = 1u32;
        while *i < chars.len() && depth > 0 {
            match chars[*i] {
                '{' => { depth += 1; word.push('{'); *i += 1; }
                '}' => { depth -= 1; if depth > 0 { word.push('}'); } *i += 1; }
                _ => { word.push(chars[*i]); *i += 1; }
            }
        }
        let word = self.expand(&word);
        let val = if indirect {
            let ind_name = self.get(&param).unwrap_or_default();
            self.get(&ind_name).unwrap_or_default()
        } else {
            self.get(&param).unwrap_or_default()
        };
        let is_set = if indirect {
            let ind_name = self.get(&param).unwrap_or_default();
            self.get(&ind_name).is_some()
        } else {
            self.get(&param).is_some()
        };
        let is_null = val.is_empty();

        match op {
            '-' => {
                if !is_set || (has_colon && is_null) { word } else { val }
            }
            '=' => {
                if !is_set || (has_colon && is_null) {
                    self.vars.lock().insert(param.clone(), word.clone());
                    word
                } else { val }
            }
            '?' => {
                if !is_set || (has_colon && is_null) {
                    let msg = if word.is_empty() { format!("{}: parameter null or not set", param) }
                    else { format!("{}: {}", param, word) };
                    crate::eprintln_fn(&msg);
                    crate::shell_syscall::sys_exit(1);
                }
                val
            }
            '+' => {
                if !is_set || (has_colon && is_null) { String::new() } else { word }
            }
            '#' => {
                if param.is_empty() {
                    if word.is_empty() {
                        // ${#} — positional parametre sayısı ($#)
                        return format!("{}", self.count_positional());
                    }
                    // ${#var} — değişken değerinin string uzunluğu (POSIX zorunlu)
                    let val = self.get(&word).unwrap_or_default();
                    return format!("{}", val.len());
                }
                if param == "#" || param == "*" {
                    return format!("{}", self.count_positional());
                }
                if word.is_empty() { return val; }
                remove_prefix(&val, &word, greedy)
            }
            ':' => {
                let parts: Vec<&str> = word.splitn(2, ':').collect();
                let offset: usize = parts[0].parse().unwrap_or(0);
                let length: usize = if parts.len() > 1 { parts[1].parse().unwrap_or(val.len()) } else { val.len() };
                let chars_vec: Vec<char> = val.chars().collect();
                if offset >= chars_vec.len() { return String::new(); }
                let end = core::cmp::min(offset + length, chars_vec.len());
                chars_vec[offset..end].iter().collect()
            }
            '%' => {
                if word.is_empty() { return val; }
                remove_suffix(&val, &word, greedy)
            }
            _ => {
                val
            }
        }
    }

    fn eval_arith_expansion(&self, chars: &[char], start: usize) -> (String, usize) {
        let mut depth = 2u32;
        let mut i = start;
        let mut expr = String::new();
        while i < chars.len() && depth > 0 {
            if chars[i] == '(' { depth += 1; expr.push('('); }
            else if chars[i] == ')' { depth -= 1; if depth > 1 { expr.push(')'); } }
            else { expr.push(chars[i]); }
            i += 1;
        }
        let result = crate::scripting::eval_arithmetic(&expr);
        (format!("{}", result), i)
    }

    fn eval_cmd_substitution(&self, chars: &[char], start: usize) -> (String, usize) {
        let mut depth = 1u32;
        let mut i = start;
        let mut cmd = String::new();
        while i < chars.len() && depth > 0 {
            if chars[i] == '(' { depth += 1; cmd.push('('); }
            else if chars[i] == ')' { depth -= 1; if depth > 0 { cmd.push(')'); } }
            else { cmd.push(chars[i]); }
            i += 1;
        }
        (cmd, i)
    }

    fn count_positional(&self) -> u32 {
        let mut count = 0u32;
        let mut idx = 1u32;
        while self.get(&format!("{}", idx)).is_some() { count += 1; idx += 1; }
        count
    }
}

fn matches_special_var(chars: &[char], start: usize, name: &str) -> bool {
    let name_chars: Vec<char> = name.chars().collect();
    if start + name_chars.len() > chars.len() {
        return false;
    }
    for (idx, c) in name_chars.iter().enumerate() {
        if chars[start + idx] != *c {
            return false;
        }
    }
    true
}

fn replace_first(val: &str, pattern: &str, replacement: &str) -> String {
    let val_str = val.to_string();
    if let Some(pos) = val_str.find(pattern) {
        let mut result = String::new();
        result.push_str(&val_str[..pos]);
        result.push_str(replacement);
        result.push_str(&val_str[pos + pattern.len()..]);
        result
    } else {
        val_str
    }
}

fn replace_all_fn(val: &str, pattern: &str, replacement: &str) -> String {
    let val_str = val.to_string();
    if pattern.is_empty() { return val_str; }
    let mut result = String::new();
    let mut last = 0;
    while let Some(pos) = val_str[last..].find(pattern) {
        let abs_pos = last + pos;
        result.push_str(&val_str[last..abs_pos]);
        result.push_str(replacement);
        last = abs_pos + pattern.len();
    }
    result.push_str(&val_str[last..]);
    result
}

fn remove_suffix(val: &str, pattern: &str, greedy: bool) -> String {
    let val_chars: Vec<char> = val.chars().collect();
    let pat_chars: Vec<char> = pattern.chars().collect();
    if greedy {
        for start in (0..=val_chars.len()).rev() {
            if glob_match_slice(&val_chars[start..], &pat_chars) {
                return val_chars[..start].iter().collect();
            }
        }
    } else {
        for start in 0..=val_chars.len() {
            if glob_match_slice(&val_chars[start..], &pat_chars) {
                return val_chars[..start].iter().collect();
            }
        }
    }
    val.to_string()
}

fn remove_prefix(val: &str, pattern: &str, greedy: bool) -> String {
    let val_chars: Vec<char> = val.chars().collect();
    let pat_chars: Vec<char> = pattern.chars().collect();
    if greedy {
        for end in (0..=val_chars.len()).rev() {
            if glob_match_slice(&val_chars[..end], &pat_chars) {
                return val_chars[end..].iter().collect();
            }
        }
    } else {
        for end in 0..=val_chars.len() {
            if glob_match_slice(&val_chars[..end], &pat_chars) {
                return val_chars[end..].iter().collect();
            }
        }
    }
    val.to_string()
}

fn glob_match_slice(name: &[char], pattern: &[char]) -> bool {
    glob_match_inner(name, pattern, 0, 0)
}

fn glob_match_inner(name: &[char], pat: &[char], ni: usize, pi: usize) -> bool {
    if pi == pat.len() { return ni == name.len(); }
    // extglob: ?(p1|p2), *(p1|p2), +(p1|p2), @(p1|p2), !(p1|p2)
    if pi + 1 < pat.len() && pat[pi + 1] == '('
        && matches!(pat[pi], '?' | '*' | '+' | '@' | '!') {
        let ext_op = pat[pi];
        let list_start = pi + 2;
        let mut depth = 1u32;
        let mut j = list_start;
        while j < pat.len() && depth > 0 {
            match pat[j] { '(' => depth += 1, ')' => depth -= 1, _ => {} }
            if depth > 0 { j += 1; }
        }
        let list_end = j;
        let after_paren = if list_end < pat.len() { list_end + 1 } else { list_end };
        // Split alternatives by '|' at depth 0
        let mut alts: Vec<(usize, usize)> = Vec::new();
        let mut seg_start = list_start;
        let mut k = list_start;
        let mut inner_depth = 0i32;
        while k < list_end {
            match pat[k] {
                '(' => { inner_depth += 1; k += 1; }
                ')' => { inner_depth -= 1; k += 1; }
                '|' if inner_depth == 0 => { alts.push((seg_start, k)); k += 1; seg_start = k; }
                _ => { k += 1; }
            }
        }
        alts.push((seg_start, list_end));
        match ext_op {
            '?' => {
                if glob_match_inner(name, pat, ni, after_paren) { return true; }
                for &(s, e) in &alts {
                    let alt = &pat[s..e];
                    let mut combined = Vec::new();
                    combined.extend_from_slice(alt);
                    combined.extend_from_slice(&pat[after_paren..]);
                    if glob_match_inner(name, &combined, ni, 0) { return true; }
                }
                return false;
            }
            '*' => {
                if glob_match_inner(name, pat, ni, after_paren) { return true; }
                for pos in ni..name.len() {
                    for &(s, e) in &alts {
                        let alt = &pat[s..e];
                        if alt.is_empty() { continue; }
                        let mut alt_combined = alt.to_vec();
                        let mut rest: Vec<char> = pat[pi..].to_vec();
                        alt_combined.append(&mut rest);
                        if glob_match_inner(&name[pos..], &alt_combined, 0, 0) {
                            if glob_match_inner(name, pat, pos + alt.len(), after_paren) {
                                return true;
                            }
                        }
                    }
                }
                return false;
            }
            '+' => {
                for &(s, e) in &alts {
                    let alt = &pat[s..e];
                    if alt.is_empty() { continue; }
                    let mut alt_combined = alt.to_vec();
                    alt_combined.extend_from_slice(&pat[after_paren..]);
                    if glob_match_inner(name, &alt_combined, ni, 0) { return true; }
                    let star_pat: Vec<char> = alt.iter()
                        .chain(pat[pi..].iter())
                        .copied().collect();
                    if glob_match_inner(name, &star_pat, ni, 0) { return true; }
                }
                return false;
            }
            '@' => {
                for &(s, e) in &alts {
                    let alt = &pat[s..e];
                    let mut combined = Vec::new();
                    combined.extend_from_slice(alt);
                    combined.extend_from_slice(&pat[after_paren..]);
                    if glob_match_inner(name, &combined, ni, 0) { return true; }
                }
                return false;
            }
            '!' => {
                for &(s, e) in &alts {
                    let alt = &pat[s..e];
                    let mut combined = Vec::new();
                    combined.extend_from_slice(alt);
                    combined.extend_from_slice(&pat[after_paren..]);
                    if glob_match_inner(name, &combined, ni, 0) { return false; }
                }
                return glob_match_inner(name, pat, ni, after_paren);
            }
            _ => {}
        }
    }
    if pat[pi] == '*' {
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
