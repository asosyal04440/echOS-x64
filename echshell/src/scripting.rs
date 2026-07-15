use crate::executor;
use crate::shell_syscall as sc;
use crate::ShellState;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;

pub struct ScriptState {
    pub errexit: bool,
    pub xtrace: bool,
    pub nounset: bool,
    pub local_vars: BTreeMap<String, String>,
    pub functions: BTreeMap<String, String>,
    pub break_flag: bool,
    pub continue_flag: bool,
}

impl ScriptState {
    pub const fn new() -> Self {
        Self {
            errexit: false,
            xtrace: false,
            nounset: false,
            local_vars: BTreeMap::new(),
            functions: BTreeMap::new(),
            break_flag: false,
            continue_flag: false,
        }
    }
}

pub static SCRIPT_STATE: Mutex<ScriptState> = Mutex::new(ScriptState::new());

pub fn run_script(state: &mut ShellState, script: &str) -> i32 {
    let mut last_exit = 0;
    for line in script.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        last_exit = execute_script_line(state, line);
        if SCRIPT_STATE.lock().errexit && last_exit != 0 {
            break;
        }
    }
    last_exit
}

fn execute_script_line(state: &mut ShellState, line: &str) -> i32 {
    let trimmed = line.trim();

    if trimmed.starts_with("for ") || trimmed.starts_with("for\t") {
        return exec_for_raw(state, line);
    }

    let expanded = state.env.expand(line);
    let trimmed = expanded.trim();

    if trimmed.starts_with("[[ ") && trimmed.ends_with(" ]]") {
        let expr = &trimmed[3..trimmed.len() - 3];
        return eval_extended_test(state, expr);
    }
    if trimmed.starts_with("((") && trimmed.ends_with(r"))") {
        let expr = &trimmed[2..trimmed.len() - 2];
        let result = eval_arithmetic(expr);
        state.exit_code = if result == 0 { 1 } else { 0 };
        return state.exit_code;
    }
    if let Some(eq_pos) = trimmed.find("=(") {
        if trimmed.ends_with(')') {
            let var_name = &trimmed[..eq_pos];
            let inner = &trimmed[eq_pos + 2..trimmed.len() - 1];
            let items: Vec<String> = inner
                .split_whitespace()
                .map(|s| state.env.expand(s))
                .collect();
            state.env.set_array(var_name, items);
            return 0;
        }
    }
    if expanded.starts_with("if ") || expanded.starts_with("if\t") {
        return exec_if(state, &expanded);
    }
    if expanded.starts_with("while ") || expanded.starts_with("until ") {
        return exec_loop(state, &expanded);
    }
    if expanded.starts_with("for ") {
        if expanded.starts_with("for ((") {
            return exec_cstyle_for(state, &expanded);
        }
        return exec_for(state, &expanded);
    }
    if expanded.starts_with("case ") {
        return exec_case(state, &expanded);
    }
    if expanded.starts_with("function ") || expanded.contains("() {") || expanded.contains("()\n{")
    {
        return exec_function_def(&expanded);
    }
    if expanded.starts_with("local ") {
        return exec_local(&expanded);
    }
    if expanded.starts_with("return ") {
        return exec_return(&expanded);
    }
    if expanded.starts_with("break") {
        SCRIPT_STATE.lock().break_flag = true;
        return 0;
    }
    if expanded.starts_with("continue") {
        SCRIPT_STATE.lock().continue_flag = true;
        return 0;
    }
    if expanded.starts_with("export ") {
        let rest = expanded[7..].trim();
        if let Some(pos) = rest.find('=') {
            let key = &rest[..pos];
            let val = &rest[pos + 1..];
            state.env.set(key, val);
        } else {
            state.env.set(rest, "");
        }
        return 0;
    }
    if expanded.starts_with("set ") {
        return exec_set(&expanded[4..]);
    }
    if expanded.starts_with("declare ") {
        return exec_declare(state, &expanded[8..]);
    }
    if expanded.starts_with("select ") {
        return exec_select(state, &expanded);
    }
    if expanded.starts_with("trap ") || expanded == "trap" {
        return exec_trap(state, &expanded);
    }
    if expanded.starts_with("readonly ") {
        return exec_readonly(state, &expanded[8..]);
    }
    if expanded == "eval" {
        // eval alone — POSIX no-op, return 0
        return 0;
    }
    if expanded.starts_with("eval ") {
        // POSIX eval: argümanları genişlet, komut olarak çalıştır
        // Birden fazla genişletme seviyesi desteklenir:
        //   eval echo \$X  →  1. expand: "echo \$X"  →  2. eval expand: "echo $X"  →  3. çalıştır
        let expr = expanded[5..].trim();
        let expanded_eval = state.env.expand(expr);
        // eval sonucu birden fazla komut içerebilir (noktalı virgül ile ayrılmış)
        for cmd in split_eval_commands(&expanded_eval) {
            let cmd = cmd.trim();
            if !cmd.is_empty() {
                executor::execute_line(state, cmd);
            }
        }
        return state.exit_code;
    }
    if expanded.starts_with("shift") {
        let n: usize = if expanded.len() > 6 && expanded.as_bytes()[6] == b' ' {
            expanded[7..].trim().parse().unwrap_or(1)
        } else {
            1
        };
        for _ in 0..n {
            for i in 1..=99 {
                let key = format!("{}", i);
                let next = format!("{}", i + 1);
                match state.env.get(&next) {
                    Some(val) => state.env.set(&key, &val),
                    None => state.env.unset(&key),
                }
            }
        }
        return 0;
    }

    executor::execute_line(state, &expanded);
    state.exit_code
}

fn exec_if(state: &mut ShellState, line: &str) -> i32 {
    let rest = &line[3..].trim();
    let cond_end = find_keyword(rest, "then");
    if cond_end == 0 {
        return 1;
    }
    let condition = rest[..cond_end].trim();
    let body_start = rest[cond_end + 4..].trim();
    let (body, elif_parts, else_body) = parse_if_blocks(body_start);

    let cond_result = eval_condition(state, condition);
    if cond_result == 0 {
        for line in body.lines() {
            let l = line.trim();
            if !l.is_empty() {
                execute_script_line(state, l);
            }
            if SCRIPT_STATE.lock().break_flag || SCRIPT_STATE.lock().continue_flag {
                break;
            }
        }
        return state.exit_code;
    }

    for (elif_cond, elif_body) in &elif_parts {
        let elif_result = eval_condition(state, elif_cond);
        if elif_result == 0 {
            for line in elif_body.lines() {
                let l = line.trim();
                if !l.is_empty() {
                    execute_script_line(state, l);
                }
                if SCRIPT_STATE.lock().break_flag || SCRIPT_STATE.lock().continue_flag {
                    break;
                }
            }
            return state.exit_code;
        }
    }

    if let Some(else_code) = else_body {
        for line in else_code.lines() {
            let l = line.trim();
            if !l.is_empty() {
                execute_script_line(state, l);
            }
            if SCRIPT_STATE.lock().break_flag || SCRIPT_STATE.lock().continue_flag {
                break;
            }
        }
    }
    state.exit_code
}

fn eval_condition(state: &mut ShellState, cond: &str) -> i32 {
    let cond = cond.trim();
    if cond == "true" {
        return 0;
    }
    if cond == "false" {
        return 1;
    }
    if let Some(rest) = cond.strip_prefix("-f ") {
        return if sc::sys_open(rest.trim(), 0).is_ok() {
            0
        } else {
            1
        };
    }
    if let Some(_rest) = cond.strip_prefix("-d ") {
        return 1;
    }
    if let Some(rest) = cond.strip_prefix("-e ") {
        return if sc::sys_open(rest.trim(), 0).is_ok() {
            0
        } else {
            1
        };
    }
    if let Some(rest) = cond.strip_prefix("-z ") {
        let val = state.env.expand(rest.trim());
        return if val.is_empty() { 0 } else { 1 };
    }
    if let Some(rest) = cond.strip_prefix("-n ") {
        let val = state.env.expand(rest.trim());
        return if !val.is_empty() { 0 } else { 1 };
    }
    if cond.contains("==") {
        let parts: Vec<&str> = cond.splitn(2, "==").collect();
        let left = state.env.expand(parts[0].trim());
        let right = state.env.expand(parts[1].trim());
        return if left == right { 0 } else { 1 };
    }
    if cond.contains("!=") {
        let parts: Vec<&str> = cond.splitn(2, "!=").collect();
        let left = state.env.expand(parts[0].trim());
        let right = state.env.expand(parts[1].trim());
        return if left != right { 0 } else { 1 };
    }
    if cond.contains("-eq") {
        let parts: Vec<&str> = cond.splitn(2, "-eq").collect();
        let left: i64 = parts[0].trim().parse().unwrap_or(0);
        let right: i64 = parts[1].trim().parse().unwrap_or(0);
        return if left == right { 0 } else { 1 };
    }
    if cond.contains("-ne") {
        let parts: Vec<&str> = cond.splitn(2, "-ne").collect();
        let left: i64 = parts[0].trim().parse().unwrap_or(0);
        let right: i64 = parts[1].trim().parse().unwrap_or(0);
        return if left != right { 0 } else { 1 };
    }
    if cond.contains("-lt") {
        let parts: Vec<&str> = cond.splitn(2, "-lt").collect();
        let left: i64 = parts[0].trim().parse().unwrap_or(0);
        let right: i64 = parts[1].trim().parse().unwrap_or(0);
        return if left < right { 0 } else { 1 };
    }
    if cond.contains("-gt") {
        let parts: Vec<&str> = cond.splitn(2, "-gt").collect();
        let left: i64 = parts[0].trim().parse().unwrap_or(0);
        let right: i64 = parts[1].trim().parse().unwrap_or(0);
        return if left > right { 0 } else { 1 };
    }
    if cond.contains("-le") {
        let parts: Vec<&str> = cond.splitn(2, "-le").collect();
        let left: i64 = parts[0].trim().parse().unwrap_or(0);
        let right: i64 = parts[1].trim().parse().unwrap_or(0);
        return if left <= right { 0 } else { 1 };
    }
    if cond.contains("-ge") {
        let parts: Vec<&str> = cond.splitn(2, "-ge").collect();
        let left: i64 = parts[0].trim().parse().unwrap_or(0);
        let right: i64 = parts[1].trim().parse().unwrap_or(0);
        return if left >= right { 0 } else { 1 };
    }

    executor::execute_line(state, cond);
    state.exit_code
}

fn exec_loop(state: &mut ShellState, line: &str) -> i32 {
    let until = line.starts_with("until");
    let rest = if until {
        &line[6..].trim()
    } else {
        &line[6..].trim()
    };
    let cond_end = find_keyword(rest, "do");
    if cond_end == 0 {
        return 1;
    }
    let condition = rest[..cond_end].trim();
    let body_start = rest[cond_end + 2..].trim();
    let body = extract_block(body_start);

    loop {
        SCRIPT_STATE.lock().break_flag = false;
        SCRIPT_STATE.lock().continue_flag = false;
        let cond_result = eval_condition(state, condition);
        if until {
            if cond_result == 0 {
                break;
            }
        } else {
            if cond_result != 0 {
                break;
            }
        }

        for bline in body.lines() {
            let l = bline.trim();
            if !l.is_empty() {
                execute_script_line(state, l);
            }
            if SCRIPT_STATE.lock().break_flag {
                return state.exit_code;
            }
            if SCRIPT_STATE.lock().continue_flag {
                break;
            }
        }
        if SCRIPT_STATE.lock().continue_flag {
            SCRIPT_STATE.lock().continue_flag = false;
            continue;
        }
    }
    state.exit_code
}

fn exec_for_raw(state: &mut ShellState, line: &str) -> i32 {
    let rest = &line[4..].trim();
    if rest.starts_with("((") {
        return exec_cstyle_for(state, line);
    }
    if let Some(pos) = rest.find(" in ") {
        let var_name = rest[..pos].trim();
        let after_in = &rest[pos + 4..];
        let list_end = find_keyword(after_in, "do");
        let items_str = if list_end > 0 {
            &after_in[..list_end]
        } else {
            after_in
        };
        let body_start = if list_end > 0 {
            after_in[list_end + 2..].trim()
        } else {
            ""
        };
        let body = extract_block(body_start);

        let items: Vec<String> = items_str
            .split_whitespace()
            .map(|s| state.env.expand(s))
            .collect();
        for item in &items {
            SCRIPT_STATE.lock().break_flag = false;
            SCRIPT_STATE.lock().continue_flag = false;
            state.env.set(var_name, item);

            for bline in body.lines() {
                let l = bline.trim();
                if !l.is_empty() {
                    execute_script_line(state, l);
                }
                if SCRIPT_STATE.lock().break_flag {
                    return state.exit_code;
                }
                if SCRIPT_STATE.lock().continue_flag {
                    break;
                }
            }
            if SCRIPT_STATE.lock().continue_flag {
                SCRIPT_STATE.lock().continue_flag = false;
                continue;
            }
        }
    }
    state.exit_code
}

fn exec_for(state: &mut ShellState, line: &str) -> i32 {
    let rest = &line[4..].trim();
    if let Some(pos) = rest.find(" in ") {
        let var_name = &rest[..pos];
        let list_str = &rest[pos + 4..];
        let list_end = find_keyword(list_str, "do");
        let items_str = if list_end > 0 {
            &list_str[..list_end]
        } else {
            list_str
        };
        let body_start = if list_end > 0 {
            list_str[list_end + 2..].trim()
        } else {
            ""
        };
        let body = extract_block(body_start);

        let items: Vec<String> = items_str
            .split_whitespace()
            .map(|s| state.env.expand(s))
            .collect();
        for item in &items {
            SCRIPT_STATE.lock().break_flag = false;
            SCRIPT_STATE.lock().continue_flag = false;
            state.env.set(var_name, item);

            for bline in body.lines() {
                let l = bline.trim();
                if !l.is_empty() {
                    execute_script_line(state, l);
                }
                if SCRIPT_STATE.lock().break_flag {
                    return state.exit_code;
                }
                if SCRIPT_STATE.lock().continue_flag {
                    break;
                }
            }
            if SCRIPT_STATE.lock().continue_flag {
                SCRIPT_STATE.lock().continue_flag = false;
                continue;
            }
        }
    } else if rest.contains(" in ") {
        let parts: Vec<&str> = rest.splitn(2, " in ").collect();
        let var_name = parts[0].trim();
        let seq_expr = parts[1].trim();
        let seq_end = find_keyword(seq_expr, "do");
        let seq_str = if seq_end > 0 {
            &seq_expr[..seq_end]
        } else {
            seq_expr
        };
        let body_start = if seq_end > 0 {
            seq_expr[seq_end + 2..].trim()
        } else {
            ""
        };
        let body = extract_block(body_start);

        let items = eval_seq(seq_str);
        for item in &items {
            SCRIPT_STATE.lock().break_flag = false;
            SCRIPT_STATE.lock().continue_flag = false;
            state.env.set(var_name, item);

            for bline in body.lines() {
                let l = bline.trim();
                if !l.is_empty() {
                    execute_script_line(state, l);
                }
                if SCRIPT_STATE.lock().break_flag {
                    return state.exit_code;
                }
                if SCRIPT_STATE.lock().continue_flag {
                    break;
                }
            }
            if SCRIPT_STATE.lock().continue_flag {
                SCRIPT_STATE.lock().continue_flag = false;
                continue;
            }
        }
    }
    state.exit_code
}

fn exec_cstyle_for(state: &mut ShellState, line: &str) -> i32 {
    let rest = &line[4..].trim();
    let inner = if let Some(s) = rest.strip_prefix("((") {
        s.strip_suffix("))").unwrap_or(s)
    } else {
        rest
    };
    let parts: Vec<&str> = inner.split(';').collect();
    let init_expr = parts.get(0).unwrap_or(&"").trim();
    let cond_expr = parts.get(1).unwrap_or(&"1").trim();
    let update_expr = parts.get(2).unwrap_or(&"").trim();

    if !init_expr.is_empty() {
        eval_cstyle_expr(state, init_expr);
    }

    let body_start_pos = line.find("do").unwrap_or(0);
    let body_start = if body_start_pos > 0 {
        let rest2 = &line[body_start_pos..];
        if rest2.starts_with("do {") {
            &rest2[3..]
        } else if rest2.starts_with("do") {
            &rest2[2..]
        } else {
            rest2
        }
    } else {
        ""
    };
    let body = extract_block(body_start);

    loop {
        SCRIPT_STATE.lock().break_flag = false;
        SCRIPT_STATE.lock().continue_flag = false;

        let cond_val = eval_cstyle_expr(state, cond_expr);
        if cond_val == 0 {
            break;
        }

        for bline in body.lines() {
            let l = bline.trim();
            if !l.is_empty() {
                execute_script_line(state, l);
            }
            if SCRIPT_STATE.lock().break_flag {
                return state.exit_code;
            }
            if SCRIPT_STATE.lock().continue_flag {
                break;
            }
        }
        if SCRIPT_STATE.lock().continue_flag {
            SCRIPT_STATE.lock().continue_flag = false;
            if !update_expr.is_empty() {
                eval_cstyle_expr(state, update_expr);
            }
            continue;
        }

        if !update_expr.is_empty() {
            eval_cstyle_expr(state, update_expr);
        }
    }
    state.exit_code
}

fn eval_cstyle_expr(state: &mut ShellState, expr: &str) -> i64 {
    let expr = expr.trim();
    let mut result = 0i64;

    let assignments: Vec<&str> = expr.split(',').collect();
    for part in &assignments {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part.ends_with("++")
            || part.ends_with("--")
            || part.starts_with("++")
            || part.starts_with("--")
        {
            result = eval_cstyle_arith(state, part);
            continue;
        }
        if let Some((var_name, op, val_str)) = parse_cstyle_assignment(part) {
            let old: i64 = state
                .env
                .get(var_name)
                .unwrap_or(String::from("0"))
                .parse()
                .unwrap_or(0);
            let rhs = eval_cstyle_arith(state, val_str);
            let new_val = match op {
                "=" => rhs,
                "+=" => old + rhs,
                "-=" => old - rhs,
                "*=" => old * rhs,
                "/=" => {
                    if rhs != 0 {
                        old / rhs
                    } else {
                        0
                    }
                }
                "%=" => {
                    if rhs != 0 {
                        old % rhs
                    } else {
                        0
                    }
                }
                _ => rhs,
            };
            state.env.set(var_name, &format!("{}", new_val));
            result = new_val;
        } else {
            result = eval_cstyle_arith(state, part);
        }
    }
    result
}

fn parse_cstyle_assignment<'a>(expr: &'a str) -> Option<(&'a str, &'a str, &'a str)> {
    for op in ["+=", "-=", "*=", "/=", "%=", "="] {
        if let Some(pos) = expr.find(op) {
            if op == "=" {
                if pos > 0 {
                    let prev = expr.as_bytes()[pos - 1];
                    if prev == b'=' || prev == b'!' || prev == b'<' || prev == b'>' {
                        continue;
                    }
                }
                if pos + 1 < expr.len() && expr.as_bytes()[pos + 1] == b'=' {
                    continue;
                }
            }
            let lhs = expr[..pos].trim().trim_start_matches('$');
            let rhs = expr[pos + op.len()..].trim();
            if !lhs.is_empty() {
                return Some((lhs, op, rhs));
            }
        }
    }
    None
}

fn eval_cstyle_arith(state: &mut ShellState, expr: &str) -> i64 {
    let expr = expr.trim();
    if let Some(inner) = expr.strip_prefix("++").map(|s| s.trim()) {
        let var = inner.trim_start_matches('$');
        let val: i64 = state
            .env
            .get(var)
            .unwrap_or(String::from("0"))
            .parse()
            .unwrap_or(0);
        state.env.set(var, &format!("{}", val + 1));
        return val + 1;
    }
    if let Some(inner) = expr.strip_prefix("--").map(|s| s.trim()) {
        let var = inner.trim_start_matches('$');
        let val: i64 = state
            .env
            .get(var)
            .unwrap_or(String::from("0"))
            .parse()
            .unwrap_or(0);
        state.env.set(var, &format!("{}", val - 1));
        return val - 1;
    }
    if let Some(inner) = expr.strip_suffix("++").map(|s| s.trim()) {
        let var = inner.trim_start_matches('$');
        let val: i64 = state
            .env
            .get(var)
            .unwrap_or(String::from("0"))
            .parse()
            .unwrap_or(0);
        state.env.set(var, &format!("{}", val + 1));
        return val;
    }
    if let Some(inner) = expr.strip_suffix("--").map(|s| s.trim()) {
        let var = inner.trim_start_matches('$');
        let val: i64 = state
            .env
            .get(var)
            .unwrap_or(String::from("0"))
            .parse()
            .unwrap_or(0);
        state.env.set(var, &format!("{}", val - 1));
        return val;
    }
    if let Some(pos) = expr.find('+') {
        if pos > 0 && pos + 1 < expr.len() && expr.as_bytes()[pos + 1] != b'+' {
            let l = eval_cstyle_arith(state, &expr[..pos]);
            let r = eval_cstyle_arith(state, &expr[pos + 1..]);
            return l + r;
        }
    }
    if let Some(pos) = expr.find('-') {
        if pos > 0 && pos + 1 < expr.len() && expr.as_bytes()[pos + 1] != b'-' {
            let l = eval_cstyle_arith(state, &expr[..pos]);
            let r = eval_cstyle_arith(state, &expr[pos + 1..]);
            return l - r;
        }
    }
    if let Some(pos) = expr.find('*') {
        if pos > 0 {
            let l = eval_cstyle_arith(state, &expr[..pos]);
            let r = eval_cstyle_arith(state, &expr[pos + 1..]);
            return l * r;
        }
    }
    if let Some(pos) = expr.find('/') {
        if pos > 0 {
            let l = eval_cstyle_arith(state, &expr[..pos]);
            let r = eval_cstyle_arith(state, &expr[pos + 1..]);
            return if r != 0 { l / r } else { 0 };
        }
    }
    if let Some(pos) = expr.find('%') {
        if pos > 0 {
            let l = eval_cstyle_arith(state, &expr[..pos]);
            let r = eval_cstyle_arith(state, &expr[pos + 1..]);
            return if r != 0 { l % r } else { 0 };
        }
    }
    if let Some(pos) = expr.find("==") {
        let l = eval_cstyle_arith(state, &expr[..pos]);
        let r = eval_cstyle_arith(state, &expr[pos + 2..]);
        return (l == r) as i64;
    }
    if let Some(pos) = expr.find("!=") {
        let l = eval_cstyle_arith(state, &expr[..pos]);
        let r = eval_cstyle_arith(state, &expr[pos + 2..]);
        return (l != r) as i64;
    }
    if let Some(pos) = expr.find("<=") {
        let l = eval_cstyle_arith(state, &expr[..pos]);
        let r = eval_cstyle_arith(state, &expr[pos + 2..]);
        return (l <= r) as i64;
    }
    if let Some(pos) = expr.find(">=") {
        let l = eval_cstyle_arith(state, &expr[..pos]);
        let r = eval_cstyle_arith(state, &expr[pos + 2..]);
        return (l >= r) as i64;
    }
    if let Some(pos) = expr.find('<') {
        let l = eval_cstyle_arith(state, &expr[..pos]);
        let r = eval_cstyle_arith(state, &expr[pos + 1..]);
        return (l < r) as i64;
    }
    if let Some(pos) = expr.find('>') {
        if pos > 0 {
            let l = eval_cstyle_arith(state, &expr[..pos]);
            let r = eval_cstyle_arith(state, &expr[pos + 1..]);
            return (l > r) as i64;
        }
    }
    if let Some(pos) = expr.find("&&") {
        let l = eval_cstyle_arith(state, &expr[..pos]);
        let r = eval_cstyle_arith(state, &expr[pos + 2..]);
        return (l != 0 && r != 0) as i64;
    }
    if let Some(pos) = expr.find("||") {
        let l = eval_cstyle_arith(state, &expr[..pos]);
        let r = eval_cstyle_arith(state, &expr[pos + 2..]);
        return (l != 0 || r != 0) as i64;
    }
    if let Some(stripped) = expr.strip_prefix('!') {
        let val = eval_cstyle_arith(state, stripped.trim());
        return if val == 0 { 1 } else { 0 };
    }
    if let Some(stripped) = expr.strip_prefix('-') {
        let val = eval_cstyle_arith(state, stripped.trim());
        return -val;
    }
    if let Some(stripped) = expr.strip_prefix('(') {
        let inner = stripped.trim_end_matches(')');
        return eval_cstyle_arith(state, inner.trim());
    }
    let var = expr.trim_start_matches('$');
    if let Some(val) = state.env.get(var) {
        val.parse().unwrap_or(0)
    } else {
        expr.parse().unwrap_or(0)
    }
}

fn exec_select(state: &mut ShellState, line: &str) -> i32 {
    let rest = &line[7..].trim();
    let var_name = rest.split_whitespace().next().unwrap_or("");
    let in_pos = rest.find(" in ");
    let items_str = if let Some(pos) = in_pos {
        let after_in = &rest[pos + 4..];
        let word_end = find_keyword(after_in, "do");
        if word_end > 0 {
            &after_in[..word_end]
        } else {
            after_in
        }
    } else {
        ""
    };
    let body_start_pos = line.find("do").unwrap_or(0);
    let body_start = if body_start_pos > 0 {
        let rest2 = &line[body_start_pos..];
        if rest2.starts_with("do {") {
            &rest2[3..]
        } else if rest2.starts_with("do") {
            &rest2[2..]
        } else {
            rest2
        }
    } else {
        ""
    };
    let body = extract_block(body_start);

    let items: Vec<String> = items_str
        .split_whitespace()
        .map(|s| state.env.expand(s))
        .collect();

    loop {
        SCRIPT_STATE.lock().break_flag = false;
        SCRIPT_STATE.lock().continue_flag = false;

        let mut counter = 1i32;
        crate::println("");
        for item in &items {
            crate::println(&format!("{}) {}", counter, item));
            counter += 1;
        }
        crate::print("#? ");

        let mut buf = [0u8; 64];
        let input = match sc::sys_read(0, &mut buf) {
            Ok(n) if n > 0 => {
                let s = core::str::from_utf8(&buf[..n]).unwrap_or("");
                s.trim().trim_end_matches('\n').to_string()
            }
            _ => break,
        };

        if input.is_empty() {
            continue;
        }
        if input == "EOF" {
            break;
        }
        if let Ok(idx) = input.parse::<usize>() {
            if idx > 0 && idx <= items.len() {
                state.env.set(var_name, &items[idx - 1]);
            } else {
                crate::eprintln_fn(&format!("select: {} invalid number", idx));
                continue;
            }
        } else {
            state.env.set(var_name, &input);
        }

        for bline in body.lines() {
            let l = bline.trim();
            if !l.is_empty() {
                execute_script_line(state, l);
            }
            if SCRIPT_STATE.lock().break_flag {
                return state.exit_code;
            }
            if SCRIPT_STATE.lock().continue_flag {
                break;
            }
        }
        if SCRIPT_STATE.lock().continue_flag {
            SCRIPT_STATE.lock().continue_flag = false;
            continue;
        }
    }
    state.exit_code
}

pub fn exec_trap(state: &mut ShellState, line: &str) -> i32 {
    let rest = line[4..].trim();
    if rest.is_empty() {
        for (k, v) in state.env.list() {
            if k.starts_with("_trap_") && !v.is_empty() {
                crate::println(&format!("trap -- '{}' {}", v, &k[6..]));
            }
        }
        return 0;
    }

    let (first, remaining) = parse_trap_operand(rest);
    let action_or_cond = strip_outer_quotes(first.trim());
    let remaining = remaining.trim();

    if remaining.is_empty() {
        if is_trap_condition(&action_or_cond) {
            let cond = normalize_trap_condition(&action_or_cond);
            if let Some(action) = state.env.get(&format!("_trap_{}", cond)) {
                if !action.is_empty() {
                    crate::println(&action);
                }
            }
            return 0;
        }
        crate::eprintln_fn("trap: missing condition");
        return 1;
    }

    if action_or_cond.chars().all(|c| c.is_ascii_digit()) {
        let mut all_conditions = Vec::new();
        all_conditions.push(action_or_cond.to_string());
        all_conditions.extend(remaining.split_whitespace().map(|s| s.to_string()));
        for cond in &all_conditions {
            let cond = normalize_trap_condition(cond);
            state.env.set(&format!("_trap_{}", cond), "");
        }
        return 0;
    }

    let action = action_or_cond;
    let conditions: Vec<&str> = remaining.split_whitespace().collect();
    if conditions.is_empty() {
        crate::eprintln_fn("trap: missing condition");
        return 1;
    }

    for cond in &conditions {
        let cond = normalize_trap_condition(cond);
        if action == "-" {
            state.env.set(&format!("_trap_{}", cond), "");
        } else {
            state.env.set(&format!("_trap_{}", cond), &action);
        }
    }
    0
}

pub fn run_trap_action(state: &mut ShellState, condition: &str) {
    let cond = normalize_trap_condition(condition);
    let key = format!("_trap_{}", cond);
    let Some(action) = state.env.get(&key) else {
        return;
    };
    if action.is_empty() {
        return;
    }
    if state.env.get("_in_trap").as_deref() == Some("1") {
        return;
    }

    state.env.set("_in_trap", "1");
    let saved_exit = state.exit_code;
    let action = strip_outer_quotes(action.trim());
    for cmd in action.split(';') {
        let cmd = cmd.trim();
        if !cmd.is_empty() {
            execute_script_line(state, cmd);
        }
    }
    state.exit_code = saved_exit;
    state.env.set("_in_trap", "0");
}

fn parse_trap_operand(input: &str) -> (&str, &str) {
    let s = input.trim_start();
    if s.is_empty() {
        return ("", "");
    }
    let bytes = s.as_bytes();
    if bytes[0] == b'\'' || bytes[0] == b'"' {
        let quote = bytes[0];
        let mut i = 1;
        while i < bytes.len() {
            if bytes[i] == quote {
                return (&s[..=i], &s[i + 1..]);
            }
            i += 1;
        }
        (s, "")
    } else {
        let mut i = 0;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        (&s[..i], &s[i..])
    }
}

fn strip_outer_quotes(input: &str) -> String {
    let s = input.trim();
    if s.len() >= 2 {
        let b = s.as_bytes();
        if (b[0] == b'\'' && b[s.len() - 1] == b'\'') || (b[0] == b'"' && b[s.len() - 1] == b'"') {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

fn normalize_trap_condition(cond: &str) -> String {
    let mut c = cond.trim().to_uppercase();
    if c.starts_with("SIG") {
        c = c[3..].to_string();
    }
    if c == "0" {
        c = "EXIT".to_string();
    }
    c
}

fn is_trap_condition(s: &str) -> bool {
    let c = normalize_trap_condition(s);
    c == "EXIT"
        || c == "ERR"
        || c == "INT"
        || c == "TERM"
        || c == "QUIT"
        || c == "HUP"
        || c == "TSTP"
        || c.chars().all(|ch| ch.is_ascii_digit())
}

fn exec_case(state: &mut ShellState, line: &str) -> i32 {
    let rest = &line[5..].trim();
    let var = state.env.expand(rest);
    let _i = 0;
    let _bytes = rest.as_bytes();

    let body_start = rest.find("esac");
    if body_start == None {
        return 1;
    }
    let body = &rest[rest.find('\n').unwrap_or(0)..];

    for case_line in body.lines() {
        let case_line = case_line.trim();
        if case_line == "esac" {
            break;
        }
        if case_line.starts_with('(') || case_line.ends_with(')') {
            let pattern = case_line
                .trim_start_matches('(')
                .trim_end_matches(')')
                .trim();
            if pattern == &var || pattern == "*" {
                let in_case = true;
                for cl in body.lines() {
                    let cl = cl.trim();
                    if cl == "esac" {
                        break;
                    }
                    if cl == ";;" {
                        break;
                    }
                    if in_case && !cl.is_empty() {
                        execute_script_line(state, cl);
                    }
                }
                return state.exit_code;
            }
        }
    }
    state.exit_code
}

fn exec_function_def(line: &str) -> i32 {
    let name = if let Some(pos) = line.find('(') {
        line[..pos].trim()
    } else {
        line.split_whitespace().nth(1).unwrap_or("")
    };
    let body_start = line.find('{').map(|p| p + 1).unwrap_or(0);
    let body_end = line.rfind('}').unwrap_or(line.len());
    let body = line[body_start..body_end].trim().to_string();

    if !name.is_empty() {
        SCRIPT_STATE.lock().functions.insert(name.to_string(), body);
    }
    0
}

fn exec_local(line: &str) -> i32 {
    let rest = &line[6..].trim();
    if let Some(pos) = rest.find('=') {
        let key = &rest[..pos];
        let val = &rest[pos + 1..];
        SCRIPT_STATE
            .lock()
            .local_vars
            .insert(key.to_string(), val.to_string());
    }
    0
}

fn exec_return(line: &str) -> i32 {
    let code_str = &line[7..].trim();
    code_str.parse().unwrap_or(0)
}

pub fn eval_extended_test(state: &mut ShellState, expr: &str) -> i32 {
    let expr = expr.trim();
    eval_or_expr(state, expr)
}

fn eval_or_expr(state: &mut ShellState, expr: &str) -> i32 {
    let parts: Vec<&str> = split_top_level(expr, "||");
    if parts.len() > 1 {
        for part in &parts {
            if eval_and_expr(state, part.trim()) == 0 {
                return 0;
            }
        }
        return 1;
    }
    eval_and_expr(state, expr)
}

fn eval_and_expr(state: &mut ShellState, expr: &str) -> i32 {
    let parts: Vec<&str> = split_top_level(expr, "&&");
    if parts.len() > 1 {
        for part in &parts {
            if eval_and_expr(state, part.trim()) != 0 {
                return 1;
            }
        }
        return 0;
    }
    eval_not_expr(state, expr)
}

fn eval_not_expr(state: &mut ShellState, expr: &str) -> i32 {
    let expr = expr.trim();
    if let Some(rest) = expr.strip_prefix('!') {
        let rest = rest.trim();
        if eval_not_expr(state, rest) == 0 {
            1
        } else {
            0
        }
    } else {
        eval_primary_test(state, expr)
    }
}

fn eval_primary_test(state: &mut ShellState, expr: &str) -> i32 {
    let expr = expr.trim();
    if expr.starts_with('(') && expr.ends_with(')') {
        let inner = &expr[1..expr.len() - 1];
        return eval_or_expr(state, inner);
    }
    let args: Vec<&str> = expr.split_whitespace().collect();
    if args.is_empty() {
        return 1;
    }
    match args[0] {
        "-f" | "-e" => {
            if args.len() < 2 {
                return 1;
            }
            let path = state.env.expand(args[1]);
            if crate::shell_syscall::sys_open(&path, 0).is_ok() {
                0
            } else {
                1
            }
        }
        "-d" => {
            if args.len() < 2 {
                return 1;
            }
            let path = state.env.expand(args[1]);
            let mut buf = [0u8; 8192];
            if let Ok(fd) = crate::shell_syscall::sys_open(&path, 0) {
                let result = if let Ok(n) = crate::shell_syscall::sys_getdents64(fd, &mut buf) {
                    n > 0
                } else {
                    false
                };
                let _ = crate::shell_syscall::sys_close(fd);
                if result {
                    0
                } else {
                    1
                }
            } else {
                1
            }
        }
        "-r" => {
            if args.len() < 2 {
                return 1;
            }
            let path = state.env.expand(args[1]);
            if crate::shell_syscall::sys_open(&path, 0).is_ok() {
                0
            } else {
                1
            }
        }
        "-w" => 0,
        "-x" => 0,
        "-s" => {
            if args.len() < 2 {
                return 1;
            }
            let path = state.env.expand(args[1]);
            match crate::shell_syscall::sys_open(&path, 0) {
                Ok(fd) => {
                    let _ = crate::shell_syscall::sys_close(fd);
                    0
                }
                Err(_) => 1,
            }
        }
        "-z" => {
            let val = if args.len() > 1 {
                state.env.expand(args[1])
            } else {
                String::new()
            };
            if val.is_empty() {
                0
            } else {
                1
            }
        }
        "-n" => {
            let val = if args.len() > 1 {
                state.env.expand(args[1])
            } else {
                String::new()
            };
            if !val.is_empty() {
                0
            } else {
                1
            }
        }
        "-L" => 0,
        "-N" => 0,
        "-O" => 0,
        "-G" => 0,
        "-t" => 0,
        "-p" => 0,
        "-c" => 0,
        "-u" => 0,
        "-g" => 0,
        "-k" => 0,
        "-S" => 0,
        "=" | "==" | "-eq" => {
            if args.len() < 3 {
                return 1;
            }
            let left = state.env.expand(args[0]);
            let right = state.env.expand(args[2]);
            if left == right {
                0
            } else {
                1
            }
        }
        "!=" | "-ne" => {
            if args.len() < 3 {
                return 1;
            }
            let left = state.env.expand(args[0]);
            let right = state.env.expand(args[2]);
            if left != right {
                0
            } else {
                1
            }
        }
        "<" | "-lt" => {
            if args.len() < 3 {
                return 1;
            }
            let left: i64 = state.env.expand(args[0]).parse().unwrap_or(0);
            let right: i64 = state.env.expand(args[2]).parse().unwrap_or(0);
            if left < right {
                0
            } else {
                1
            }
        }
        ">" | "-gt" => {
            if args.len() < 3 {
                return 1;
            }
            let left: i64 = state.env.expand(args[0]).parse().unwrap_or(0);
            let right: i64 = state.env.expand(args[2]).parse().unwrap_or(0);
            if left > right {
                0
            } else {
                1
            }
        }
        "<=" | "-le" => {
            if args.len() < 3 {
                return 1;
            }
            let left: i64 = state.env.expand(args[0]).parse().unwrap_or(0);
            let right: i64 = state.env.expand(args[2]).parse().unwrap_or(0);
            if left <= right {
                0
            } else {
                1
            }
        }
        ">=" | "-ge" => {
            if args.len() < 3 {
                return 1;
            }
            let left: i64 = state.env.expand(args[0]).parse().unwrap_or(0);
            let right: i64 = state.env.expand(args[2]).parse().unwrap_or(0);
            if left >= right {
                0
            } else {
                1
            }
        }
        "-nt" => {
            if args.len() < 3 {
                return 1;
            }
            0
        }
        "-ot" => {
            if args.len() < 3 {
                return 1;
            }
            1
        }
        "-ef" => {
            if args.len() < 3 {
                return 1;
            }
            0
        }
        _ => {
            if args.len() == 1 {
                let val = state.env.expand(args[0]);
                if !val.is_empty() && val != "0" && val != "false" {
                    0
                } else {
                    1
                }
            } else if args.len() == 3 {
                let left = state.env.expand(args[0]);
                let op = args[1];
                let right = state.env.expand(args[2]);
                match op {
                    "==" | "=" => {
                        if left == right {
                            0
                        } else {
                            1
                        }
                    }
                    "!=" => {
                        if left != right {
                            0
                        } else {
                            1
                        }
                    }
                    "<" => {
                        if left < right {
                            0
                        } else {
                            1
                        }
                    }
                    ">" => {
                        if left > right {
                            0
                        } else {
                            1
                        }
                    }
                    "-eq" => {
                        let l: i64 = left.parse().unwrap_or(0);
                        let r: i64 = right.parse().unwrap_or(0);
                        if l == r {
                            0
                        } else {
                            1
                        }
                    }
                    "-ne" => {
                        let l: i64 = left.parse().unwrap_or(0);
                        let r: i64 = right.parse().unwrap_or(0);
                        if l != r {
                            0
                        } else {
                            1
                        }
                    }
                    "-lt" => {
                        let l: i64 = left.parse().unwrap_or(0);
                        let r: i64 = right.parse().unwrap_or(0);
                        if l < r {
                            0
                        } else {
                            1
                        }
                    }
                    "-gt" => {
                        let l: i64 = left.parse().unwrap_or(0);
                        let r: i64 = right.parse().unwrap_or(0);
                        if l > r {
                            0
                        } else {
                            1
                        }
                    }
                    "-le" => {
                        let l: i64 = left.parse().unwrap_or(0);
                        let r: i64 = right.parse().unwrap_or(0);
                        if l <= r {
                            0
                        } else {
                            1
                        }
                    }
                    "-ge" => {
                        let l: i64 = left.parse().unwrap_or(0);
                        let r: i64 = right.parse().unwrap_or(0);
                        if l >= r {
                            0
                        } else {
                            1
                        }
                    }
                    "=~" => {
                        let pattern = right;
                        let captures = regex_match_captures(&left, &pattern);
                        if let Some(groups) = captures {
                            state.env.set_bash_rematch(groups);
                            0
                        } else {
                            1
                        }
                    }
                    _ => 1,
                }
            } else {
                1
            }
        }
    }
}

fn regex_match(text: &str, pattern: &str) -> bool {
    let t: Vec<char> = text.chars().collect();
    let p: Vec<char> = pattern.chars().collect();
    regex_match_full(&t, &p, 0, 0)
}

fn regex_match_captures(text: &str, pattern: &str) -> Option<Vec<String>> {
    let t: Vec<char> = text.chars().collect();
    let p: Vec<char> = pattern.chars().collect();
    let group_count = p.iter().filter(|&&c| c == '(').count();
    let mut captures: Vec<Option<(usize, usize)>> = Vec::new();
    for _ in 0..group_count {
        captures.push(None);
    }
    let captures_ptr = captures.as_mut_ptr();
    let captures_len = captures.len();
    let matched = regex_match_captures_recursive(&t, &p, 0, 0, captures_ptr, captures_len);
    if matched {
        let mut result = Vec::new();
        result.push(text.to_string());
        for cap in captures.iter() {
            match cap {
                Some((start, end)) => {
                    if *start <= *end && *end <= text.len() {
                        let s: String = text[*start..*end].chars().collect();
                        result.push(s);
                    } else {
                        result.push(String::new());
                    }
                }
                None => result.push(String::new()),
            }
        }
        Some(result)
    } else {
        None
    }
}

fn regex_match_captures_recursive(
    text: &[char],
    pat: &[char],
    ti: usize,
    pi: usize,
    captures: *mut Option<(usize, usize)>,
    caps_len: usize,
) -> bool {
    if pi == pat.len() {
        return ti == text.len();
    }

    if pi < pat.len() && pat[pi] == '(' {
        let group_idx = unsafe {
            let mut count = 0;
            for k in 0..pi {
                if *pat.get_unchecked(k) == '(' {
                    count += 1;
                }
            }
            count
        };
        if group_idx < caps_len {
            unsafe {
                *captures.add(group_idx) = Some((ti, 0));
            }
        }
        let result = regex_match_captures_recursive(text, pat, ti, pi + 1, captures, caps_len);
        if result {
            return true;
        } else {
            if group_idx < caps_len {
                unsafe {
                    *captures.add(group_idx) = None;
                }
            }
            return false;
        }
    }

    if pi < pat.len() && pat[pi] == ')' {
        let mut open_pos = None;
        let mut depth = 0;
        for k in (0..pi).rev() {
            if pat[k] == ')' {
                depth += 1;
            } else if pat[k] == '(' {
                if depth == 0 {
                    open_pos = Some(k);
                    break;
                } else {
                    depth -= 1;
                }
            }
        }
        if let Some(open_k) = open_pos {
            let group_idx = unsafe {
                let mut count = 0;
                for k in 0..open_k {
                    if *pat.get_unchecked(k) == '(' {
                        count += 1;
                    }
                }
                count
            };
            if group_idx < caps_len {
                unsafe {
                    if let Some(ref mut slot) = *captures.add(group_idx) {
                        slot.1 = ti;
                    }
                }
            }
        }
        return regex_match_captures_recursive(text, pat, ti, pi + 1, captures, caps_len);
    }

    if pi + 1 < pat.len() && pat[pi + 1] == '*' {
        let c = pat[pi];
        let mut j = ti;
        while j <= text.len() {
            if regex_match_captures_recursive(text, pat, j, pi + 2, captures, caps_len) {
                return true;
            }
            if j < text.len()
                && (c == '.' || c == text[j] || (c == '[' && char_class_match(text[j], &pat, pi)))
            {
                j += 1;
            } else {
                break;
            }
        }
        return false;
    }
    if pi + 1 < pat.len() && pat[pi + 1] == '+' {
        let c = pat[pi];
        let mut j = ti;
        let mut matched = false;
        while j <= text.len() {
            if regex_match_captures_recursive(text, pat, j, pi + 2, captures, caps_len) {
                return true;
            }
            if j < text.len()
                && (c == '.' || c == text[j] || (c == '[' && char_class_match(text[j], &pat, pi)))
            {
                j += 1;
                matched = true;
            } else {
                break;
            }
        }
        return matched && regex_match_captures_recursive(text, pat, j, pi + 2, captures, caps_len);
    }
    if pi + 1 < pat.len() && pat[pi + 1] == '?' {
        let c = pat[pi];
        if ti < text.len()
            && (c == '.' || c == text[ti] || (c == '[' && char_class_match(text[ti], &pat, pi)))
        {
            return regex_match_captures_recursive(text, pat, ti + 1, pi + 2, captures, caps_len);
        }
        return regex_match_captures_recursive(text, pat, ti, pi + 2, captures, caps_len);
    }
    if pat[pi] == '^' {
        return regex_match_captures_recursive(text, pat, ti, pi + 1, captures, caps_len);
    }
    if pat[pi] == '$' {
        return ti == text.len()
            && regex_match_captures_recursive(text, pat, ti, pi + 1, captures, caps_len);
    }
    if pat[pi] == '.' {
        if ti < text.len() {
            return regex_match_captures_recursive(text, pat, ti + 1, pi + 1, captures, caps_len);
        }
        return false;
    }
    if pat[pi] == '[' {
        let mut j = pi + 1;
        let mut negate = false;
        if j < pat.len() && pat[j] == '^' {
            negate = true;
            j += 1;
        }
        if j < pat.len() && pat[j] == ']' {
            j += 1;
        }
        let mut found = false;
        while j < pat.len() && pat[j] != ']' {
            if j + 2 < pat.len() && pat[j + 1] == '-' {
                if ti < text.len() && text[ti] >= pat[j] && text[ti] <= pat[j + 2] {
                    found = true;
                }
                j += 3;
            } else {
                if ti < text.len() && text[ti] == pat[j] {
                    found = true;
                }
                j += 1;
            }
        }
        let mut end_class = j;
        if end_class < pat.len() {
            end_class += 1;
        }
        if found == negate {
            return ti < text.len()
                && regex_match_captures_recursive(
                    text,
                    pat,
                    ti + 1,
                    end_class,
                    captures,
                    caps_len,
                );
        }
        return false;
    }
    if ti < text.len() && text[ti] == pat[pi] {
        return regex_match_captures_recursive(text, pat, ti + 1, pi + 1, captures, caps_len);
    }
    false
}

fn regex_match_full(text: &[char], pat: &[char], ti: usize, pi: usize) -> bool {
    if pi == pat.len() {
        return ti == text.len();
    }
    if pi + 1 < pat.len() && pat[pi + 1] == '*' {
        let c = pat[pi];
        let mut j = ti;
        while j <= text.len() {
            if regex_match_full(text, pat, j, pi + 2) {
                return true;
            }
            if j < text.len()
                && (c == '.' || c == text[j] || (c == '[' && char_class_match(text[j], &pat, pi)))
            {
                j += 1;
            } else {
                break;
            }
        }
        return false;
    }
    if pi + 1 < pat.len() && pat[pi + 1] == '+' {
        let c = pat[pi];
        let mut j = ti;
        let mut matched = false;
        while j <= text.len() {
            if regex_match_full(text, pat, j, pi + 2) {
                return true;
            }
            if j < text.len()
                && (c == '.' || c == text[j] || (c == '[' && char_class_match(text[j], &pat, pi)))
            {
                j += 1;
                matched = true;
            } else {
                break;
            }
        }
        return matched && regex_match_full(text, pat, j, pi + 2);
    }
    if pi + 1 < pat.len() && pat[pi + 1] == '?' {
        let c = pat[pi];
        if ti < text.len()
            && (c == '.' || c == text[ti] || (c == '[' && char_class_match(text[ti], &pat, pi)))
        {
            return regex_match_full(text, pat, ti + 1, pi + 2);
        }
        return regex_match_full(text, pat, ti, pi + 2);
    }
    if pat[pi] == '^' {
        return regex_match_full(text, pat, ti, pi + 1);
    }
    if pat[pi] == '$' {
        return ti == text.len() && regex_match_full(text, pat, ti, pi + 1);
    }
    if pat[pi] == '.' {
        if ti < text.len() {
            return regex_match_full(text, pat, ti + 1, pi + 1);
        }
        return false;
    }
    if pat[pi] == '[' {
        let mut j = pi + 1;
        let mut negate = false;
        if j < pat.len() && pat[j] == '^' {
            negate = true;
            j += 1;
        }
        if j < pat.len() && pat[j] == ']' {
            j += 1;
        }
        let mut found = false;
        while j < pat.len() && pat[j] != ']' {
            if j + 2 < pat.len() && pat[j + 1] == '-' {
                if ti < text.len() && text[ti] >= pat[j] && text[ti] <= pat[j + 2] {
                    found = true;
                }
                j += 3;
            } else {
                if ti < text.len() && text[ti] == pat[j] {
                    found = true;
                }
                j += 1;
            }
        }
        let mut end_class = j;
        if end_class < pat.len() {
            end_class += 1;
        }
        if found == negate {
            return ti < text.len() && regex_match_full(text, pat, ti + 1, end_class);
        }
        return false;
    }
    if ti < text.len() && text[ti] == pat[pi] {
        return regex_match_full(text, pat, ti + 1, pi + 1);
    }
    false
}

fn char_class_match(c: char, pat: &[char], pi: usize) -> bool {
    if pi >= pat.len() || pat[pi] != '[' {
        return false;
    }
    let mut j = pi + 1;
    let mut negate = false;
    if j < pat.len() && pat[j] == '^' {
        negate = true;
        j += 1;
    }
    if j < pat.len() && pat[j] == ']' {
        j += 1;
    }
    let mut found = false;
    while j < pat.len() && pat[j] != ']' {
        if j + 2 < pat.len() && pat[j + 1] == '-' {
            if c >= pat[j] && c <= pat[j + 2] {
                found = true;
            }
            j += 3;
        } else {
            if c == pat[j] {
                found = true;
            }
            j += 1;
        }
    }
    if found == negate {
        false
    } else {
        true
    }
}

fn split_top_level<'a>(expr: &'a str, op: &str) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut last = 0;
    let chars: Vec<char> = expr.chars().collect();
    let op_chars: Vec<char> = op.chars().collect();
    let len = chars.len();
    let op_len = op_chars.len();
    let mut i = 0;
    while i < len {
        if chars[i] == '(' {
            depth += 1;
        } else if chars[i] == ')' {
            if depth > 0 {
                depth -= 1;
            }
        } else if depth == 0 && i + op_len <= len && &chars[i..i + op_len] == op_chars.as_slice() {
            if i > last {
                parts.push(&expr[last..i]);
            }
            last = i + op_len;
            i += op_len;
            continue;
        }
        i += 1;
    }
    if last < len {
        parts.push(&expr[last..]);
    }
    parts
}

fn exec_declare(state: &mut ShellState, args: &str) -> i32 {
    let args: Vec<&str> = args.split_whitespace().collect();
    if args.is_empty() {
        for (k, v) in state.env.list() {
            crate::println(&format!("declare -- {}=\"{}\"", k, v));
        }
        return 0;
    }
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-a" || args[i] == "-A" {
            i += 1;
            if i < args.len() {
                let name_val = args[i];
                if let Some(pos) = name_val.find('=') {
                    let name = &name_val[..pos];
                    let val_str = &name_val[pos + 1..];
                    let values: Vec<String> =
                        val_str.split_whitespace().map(|s| s.to_string()).collect();
                    state.env.set_array(name, values);
                }
            }
        } else if args[i] == "-i" || args[i] == "-r" || args[i] == "-x" {
            i += 1;
        } else if args[i].contains('=') {
            let parts: Vec<&str> = args[i].splitn(2, '=').collect();
            let val = if parts.len() > 1 { parts[1] } else { "" };
            if val.starts_with('(') && val.ends_with(')') {
                let inner = &val[1..val.len() - 1];
                let values: Vec<String> = inner.split_whitespace().map(|s| s.to_string()).collect();
                state.env.set_array(parts[0], values);
            } else {
                state.env.set(parts[0], val);
            }
        } else {
            state.env.set(args[i], "");
        }
        i += 1;
    }
    0
}

fn exec_readonly(state: &mut ShellState, args: &str) -> i32 {
    let args: Vec<&str> = args.split_whitespace().collect();
    if args.is_empty() {
        for (k, v) in state.env.list() {
            crate::println(&format!("readonly {}=\"{}\"", k, v));
        }
        return 0;
    }
    let mut i = 0;
    while i < args.len() {
        if args[i].contains('=') {
            let parts: Vec<&str> = args[i].splitn(2, '=').collect();
            state.env.set(parts[0], parts[1]);
        } else {
            state.env.set(args[i], "");
        }
        i += 1;
    }
    0
}

fn exec_set(line: &str) -> i32 {
    let flag = line.trim();
    match flag {
        "-e" => {
            SCRIPT_STATE.lock().errexit = true;
        }
        "+e" => {
            SCRIPT_STATE.lock().errexit = false;
        }
        "-x" => {
            SCRIPT_STATE.lock().xtrace = true;
        }
        "+x" => {
            SCRIPT_STATE.lock().xtrace = false;
        }
        "-u" => {
            SCRIPT_STATE.lock().nounset = true;
        }
        "+u" => {
            SCRIPT_STATE.lock().nounset = false;
        }
        _ => {}
    }
    0
}

pub fn eval_arithmetic(expr: &str) -> i64 {
    let expr = expr.trim();
    let expr = expr.trim_start_matches('(').trim_end_matches(')');
    let expr = expr.trim_start_matches('(').trim_end_matches(')');

    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() == 1 {
        if let Ok(v) = parts[0].parse::<i64>() {
            return v;
        }
        if let Some(val) = eval_infix(parts[0]) {
            return val;
        }
        if let Some(state) = try_lock_script_state() {
            if let Some(val) = state.local_vars.get(parts[0]) {
                if let Ok(v) = val.parse::<i64>() {
                    return v;
                }
                if let Some(v) = eval_infix(val) {
                    return v;
                }
                return 0;
            }
        }
        return 0;
    }
    if parts.len() == 3 {
        let left = eval_arithmetic(parts[0]);
        let right = eval_arithmetic(parts[2]);
        return match parts[1] {
            "+" => left + right,
            "-" | "\u{2212}" => left - right,
            "*" => left * right,
            "/" => {
                if right != 0 {
                    left / right
                } else {
                    0
                }
            }
            "%" => {
                if right != 0 {
                    left % right
                } else {
                    0
                }
            }
            "==" | "=" => (left == right) as i64,
            "!=" => (left != right) as i64,
            "-lt" | "<" => (left < right) as i64,
            "-gt" | ">" => (left > right) as i64,
            "-le" | "<=" => (left <= right) as i64,
            "-ge" | ">=" => (left >= right) as i64,
            _ => 0,
        };
    }
    0
}

fn eval_infix(expr: &str) -> Option<i64> {
    let ops: &[(&str, fn(i64, i64) -> i64)] = &[
        ("+", |a, b| a + b),
        ("-", |a, b| a - b),
        ("*", |a, b| a * b),
        ("/", |a, b| if b != 0 { a / b } else { 0 }),
        ("%", |a, b| if b != 0 { a % b } else { 0 }),
    ];
    for &(op, f) in ops {
        if let Some(pos) = expr.rfind(op) {
            if pos > 0 && pos + op.len() < expr.len() {
                let left = eval_arithmetic(&expr[..pos]);
                let right = eval_arithmetic(&expr[pos + op.len()..]);
                return Some(f(left, right));
            }
        }
    }
    None
}

fn try_lock_script_state() -> Option<spin::MutexGuard<'static, ScriptState>> {
    Some(SCRIPT_STATE.lock())
}

fn eval_seq(expr: &str) -> Vec<String> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() == 1 {
        if let Ok(end) = parts[0].parse::<i64>() {
            return (1..=end).map(|i| format!("{}", i)).collect();
        }
    }
    if parts.len() == 3 && parts[1] == ".." {
        if let (Ok(start), Ok(end)) = (parts[0].parse::<i64>(), parts[2].parse::<i64>()) {
            if start <= end {
                (start..=end).map(|i| format!("{}", i)).collect()
            } else {
                (end..=start).rev().map(|i| format!("{}", i)).collect()
            }
        } else {
            Vec::new()
        }
    } else {
        parts.iter().map(|s| s.to_string()).collect()
    }
}

fn find_keyword(text: &str, keyword: &str) -> usize {
    let words = text.split_whitespace();
    let mut offset = 0;
    for word in words {
        let word_start = text[offset..]
            .find(word)
            .map(|p| p + offset)
            .unwrap_or(offset);
        if word == keyword {
            return word_start;
        }
        offset = word_start + word.len();
    }
    0
}

fn extract_block(text: &str) -> &str {
    text.trim()
        .trim_end_matches("done")
        .trim_end_matches("fi")
        .trim_end_matches("esac")
        .trim()
}

/// eval için komutları ayır — noktalı virgül ile ayrılmış, tırnak içindeki noktalı virgülleri koruyarak
fn split_eval_commands(input: &str) -> Vec<&str> {
    let mut commands = Vec::new();
    let mut depth = 0i32;
    let mut last = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        match bytes[i] {
            b'\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            b'"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            b'(' if !in_single_quote && !in_double_quote => {
                depth += 1;
            }
            b')' if !in_single_quote && !in_double_quote => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            b';' if !in_single_quote && !in_double_quote && depth == 0 => {
                if i > last {
                    commands.push(&input[last..i]);
                }
                last = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if last < len {
        commands.push(&input[last..]);
    }
    commands
}

fn parse_if_blocks(text: &str) -> (&str, Vec<(&str, &str)>, Option<String>) {
    let mut body_end = text.len();
    let mut elif_parts = Vec::new();
    let mut else_body: Option<String> = None;

    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    let mut depth = 0;
    let _current_start = 0;
    let _current_type = "then";

    while i < lines.len() {
        let line = lines[i].trim();
        if line.starts_with("if ") {
            depth += 1;
        }
        if line == "fi" {
            if depth == 0 {
                body_end = text.find(lines[i]).unwrap_or(text.len());
                break;
            }
            depth -= 1;
        }
        if depth == 0 {
            if line.starts_with("elif ") {
                let cond = &line[5..];
                elif_parts.push((cond, ""));
            }
            if line == "else" {
                let s = lines[i + 1..].join("\n");
                else_body = Some(s.trim_end_matches("fi").trim().to_string());
            }
        }
        i += 1;
    }

    let body = text[..body_end].trim();
    (body, elif_parts, else_body)
}
