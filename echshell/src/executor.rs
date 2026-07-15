use crate::builtins;
use crate::scripting;
use crate::shell_syscall as sc;
use crate::tokenizer::{self, ParsedCommand, Token};
use crate::{eprintln_fn, ShellState};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub fn execute_line(state: &mut ShellState, input: &str) {
    let expanded_cmd = expand_command_substitutions(state, input);

    let raw_trimmed = expanded_cmd.trim();
    if raw_trimmed.starts_with("for ") || raw_trimmed.starts_with("for\t") {
        scripting::run_script(state, &expanded_cmd);
        return;
    }

    let mut expanded = state.env.expand(&expanded_cmd);
    expanded = tokenizer::expand_braces(&expanded);

    let trimmed = expanded.trim();
    if trimmed.starts_with("[[ ") && trimmed.ends_with(" ]]") {
        let expr = &trimmed[3..trimmed.len() - 3];
        state.exit_code = scripting::eval_extended_test(state, expr);
        return;
    }
    if trimmed.starts_with("((") && trimmed.ends_with(r"))") {
        let expr = &trimmed[2..trimmed.len() - 2];
        let result = scripting::eval_arithmetic(expr);
        state.exit_code = if result == 0 { 1 } else { 0 };
        return;
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
            return;
        }
    }
    if trimmed.starts_with("if ")
        || trimmed.starts_with("if\t")
        || trimmed.starts_with("while ")
        || trimmed.starts_with("until ")
        || trimmed.starts_with("for ")
        || trimmed.starts_with("case ")
        || trimmed.starts_with("function ")
        || trimmed.contains("() {")
        || trimmed.contains("()\n{")
        || trimmed.starts_with("local ")
        || trimmed.starts_with("return ")
        || trimmed.starts_with("break")
        || trimmed.starts_with("continue")
        || trimmed.starts_with("eval ")
        || trimmed.starts_with("select ")
        || trimmed.starts_with("declare ")
        || trimmed.starts_with("readonly ")
        || trimmed.starts_with("trap ")
        || trimmed.starts_with("trap\n")
        || trimmed == "trap"
    {
        scripting::run_script(state, &expanded);
        return;
    }

    if trimmed.starts_with("alias ") {
        let rest = trimmed[6..].trim();
        if let Some(pos) = rest.find('=') {
            let name = rest[..pos].trim();
            let value = rest[pos + 1..].trim().trim_matches('\'').trim_matches('"');
            state.env.set(&format!("_alias_{}", name), value);
            return;
        }
        for (k, v) in state.env.list() {
            if k.starts_with("_alias_") {
                crate::println(&format!("{}='{}'", &k[7..], v));
            }
        }
        return;
    }
    if trimmed.starts_with("unalias ") {
        let name = trimmed[8..].trim();
        state.env.unset(&format!("_alias_{}", name));
        return;
    }

    // Variable assignment: VAR=value (no spaces around '=')
    // Must check BEFORE tokenize/execute_tokens to avoid "command not found"
    if let Some(eq_pos) = trimmed.find('=') {
        let var_name = &trimmed[..eq_pos];
        // Valid shell variable: starts with alpha/underscore, contains only alnum/underscore
        if !var_name.is_empty()
            && var_name
                .chars()
                .next()
                .map_or(false, |c| c.is_alphabetic() || c == '_')
            && var_name.chars().all(|c| c.is_alphanumeric() || c == '_')
            && !trimmed[eq_pos..].starts_with("() ")
        {
            let value = &trimmed[eq_pos + 1..];
            let expanded_value = state.env.expand(value);
            state.env.set(var_name, &expanded_value);
            return;
        }
    }

    let tokens = tokenizer::tokenize(&expanded);
    execute_tokens(state, &tokens);
}

fn resolve_alias(state: &ShellState, cmd: &str) -> String {
    state
        .env
        .get(&format!("_alias_{}", cmd))
        .unwrap_or_else(|| cmd.to_string())
}

fn execute_tokens(state: &mut ShellState, tokens: &[Token]) {
    let mut i = 0;
    let mut last_result = true;
    while i < tokens.len() {
        let mut segment = Vec::new();
        let mut op = Token::Semi;
        while i < tokens.len() {
            match &tokens[i] {
                Token::And | Token::Or | Token::Semi | Token::Newline => {
                    op = tokens[i].clone();
                    i += 1;
                    break;
                }
                t => {
                    segment.push(t.clone());
                    i += 1;
                }
            }
        }
        if segment.is_empty() {
            continue;
        }
        match &op {
            Token::And if !last_result => {
                continue;
            }
            Token::Or if last_result => {
                continue;
            }
            _ => {}
        }
        let cmds = tokenizer::parse_simple(&segment);
        last_result = execute_cmds(state, &cmds);
    }
}

fn execute_cmds(state: &mut ShellState, cmds: &[ParsedCommand]) -> bool {
    if cmds.is_empty() {
        return true;
    }
    if cmds.len() == 1 {
        return execute_single(state, &cmds[0]);
    }
    execute_pipeline(state, cmds)
}

fn execute_single(state: &mut ShellState, cmd: &ParsedCommand) -> bool {
    if cmd.argv.is_empty() {
        return true;
    }

    if cmd.argv[0] == "(subshell)" || cmd.argv[0] == "{group}" {
        if let Some(ref code) = cmd.here_doc {
            for line in code.lines() {
                let l = line.trim();
                if !l.is_empty() {
                    execute_line(state, l);
                }
            }
        }
        return true;
    }

    let mut argv: Vec<String> = cmd.argv.clone();
    argv[0] = resolve_alias(state, &argv[0]);
    let mut fd_ops: Vec<(String, String)> = Vec::new();
    let mut filtered_argv: Vec<String> = Vec::new();
    for arg in &argv {
        if let Some(target) = arg.strip_prefix("2>&") {
            let target = target.to_string();
            fd_ops.push(("2>&".to_string(), target));
        } else if let Some(target) = arg.strip_prefix("2>") {
            fd_ops.push(("2>".to_string(), target.to_string()));
        } else if let Some(target) = arg.strip_prefix("1>&") {
            fd_ops.push(("1>&".to_string(), target.to_string()));
        } else if let Some(target) = arg.strip_prefix("3>") {
            fd_ops.push(("3>".to_string(), target.to_string()));
        } else if let Some(target) = arg.strip_prefix("4>") {
            fd_ops.push(("4>".to_string(), target.to_string()));
        } else if let Some(target) = arg.strip_prefix("5>") {
            fd_ops.push(("5>".to_string(), target.to_string()));
        } else if arg.starts_with(">&-") || arg.starts_with("<&-") {
            let fd_num: usize = arg[..1].parse().unwrap_or(0);
            fd_ops.push(("close".to_string(), fd_num.to_string()));
        } else {
            filtered_argv.push(arg.clone());
        }
    }
    let argv_refs: Vec<&str> = filtered_argv.iter().map(|s| s.as_str()).collect();

    let expanded_args = tokenizer::expand_globs(&filtered_argv[1..]);
    let mut final_argv: Vec<&str> = if !argv_refs.is_empty() {
        vec![argv_refs[0]]
    } else {
        Vec::new()
    };
    for a in &expanded_args {
        final_argv.push(a);
    }

    let mut stdin_fd: i32 = -1;
    let mut stdout_fd: i32 = -1;
    let mut stderr_fd: i32 = -1;

    if let Some(ref path) = cmd.redirect_out {
        stdout_fd = sc::sys_open(path, 1 | 0x200 | 0x240).unwrap_or(0) as i32;
        if stdout_fd >= 0 {
            let _ = sc::sys_dup2(stdout_fd as usize, 1);
            let _ = sc::sys_close(stdout_fd as usize);
        }
    }
    if let Some(ref path) = cmd.redirect_append {
        stdout_fd = sc::sys_open(path, 1 | 0x200 | 0x400).unwrap_or(0) as i32;
        if stdout_fd >= 0 {
            let _ = sc::sys_dup2(stdout_fd as usize, 1);
            let _ = sc::sys_close(stdout_fd as usize);
        }
    }
    if let Some(ref path) = cmd.redirect_in {
        stdin_fd = sc::sys_open(path, 0).unwrap_or(0) as i32;
        if stdin_fd >= 0 {
            let _ = sc::sys_dup2(stdin_fd as usize, 0);
            let _ = sc::sys_close(stdin_fd as usize);
        }
    }
    if let Some(ref path) = cmd.redirect_err {
        stderr_fd = sc::sys_open(path, 1 | 0x200 | 0x240).unwrap_or(0) as i32;
        if stderr_fd >= 0 {
            let _ = sc::sys_dup2(stderr_fd as usize, 2);
            let _ = sc::sys_close(stderr_fd as usize);
        }
    }
    // <> read-write redirect — dosyayı okuma/yazma modunda aç, stdin ve stdout'a bağla
    if let Some(ref path) = cmd.redirect_readwrite {
        if let Ok(fd) = sc::sys_open(path, 2 /* O_RDWR */) {
            let _ = sc::sys_dup2(fd, 0);
            let _ = sc::sys_dup2(fd, 1);
            let _ = sc::sys_close(fd);
        }
    }
    if let Some(ref path) = cmd.redirect_err_append {
        stderr_fd = sc::sys_open(path, 1 | 0x200 | 0x400).unwrap_or(0) as i32;
        if stderr_fd >= 0 {
            let _ = sc::sys_dup2(stderr_fd as usize, 2);
            let _ = sc::sys_close(stderr_fd as usize);
        }
    }
    if let Some(ref content) = cmd.here_doc {
        if cmd.argv[0] != "(subshell)" && cmd.argv[0] != "{group}" {
            let tmp = "/tmp/.heredoc";
            executor_write_file(tmp, content.as_bytes());
            stdin_fd = sc::sys_open(tmp, 0).unwrap_or(0) as i32;
            if stdin_fd >= 0 {
                let _ = sc::sys_dup2(stdin_fd as usize, 0);
                let _ = sc::sys_close(stdin_fd as usize);
            }
        }
    }
    if let Some(ref content) = cmd.here_string {
        let tmp = "/tmp/.heredoc";
        let mut data = content.as_bytes().to_vec();
        data.push(b'\n');
        executor_write_file(tmp, &data);
        stdin_fd = sc::sys_open(tmp, 0).unwrap_or(0) as i32;
        if stdin_fd >= 0 {
            let _ = sc::sys_dup2(stdin_fd as usize, 0);
            let _ = sc::sys_close(stdin_fd as usize);
        }
    }
    for (op, target) in &fd_ops {
        if op == "2>&" {
            if let Ok(fd) = target.parse::<usize>() {
                let _ = sc::sys_dup2(fd, 2);
            }
        } else if op == "2>" {
            if let Ok(fd) = sc::sys_open(target, 1 | 0x200 | 0x240) {
                let _ = sc::sys_dup2(fd, 2);
                let _ = sc::sys_close(fd);
            }
        } else if op == "1>&" {
            if let Ok(fd) = target.parse::<usize>() {
                let _ = sc::sys_dup2(fd, 1);
            }
        } else if op == "close" {
            if let Ok(fd) = target.parse::<usize>() {
                let _ = sc::sys_close(fd);
            }
        } else if op.ends_with('>') {
            if let Ok(fd_num) = op.trim_end_matches('>').parse::<usize>() {
                if let Ok(fd) = sc::sys_open(target, 1 | 0x200 | 0x240) {
                    let _ = sc::sys_dup2(fd, fd_num);
                    let _ = sc::sys_close(fd);
                }
            }
        }
    }

    let result = if cmd.background {
        match sc::sys_fork() {
            Ok(0) => {
                let _ = builtins::dispatch(state, &final_argv);
                sc::sys_exit(0);
            }
            Ok(pid) => {
                state.job_id_counter += 1;
                state.jobs.push(crate::Job {
                    id: state.job_id_counter,
                    pid,
                    cmd: final_argv.join(" "),
                    running: true,
                    background: true,
                });
                crate::println(&format!("[{}] {}", state.job_id_counter, pid));
                true
            }
            Err(_) => {
                eprintln_fn("fork basarisiz");
                false
            }
        }
    } else {
        builtins::dispatch(state, &final_argv)
    };

    if stdin_fd >= 0 {
        let _ = sc::sys_dup2(0, 0);
    }
    if stdout_fd >= 0 {
        let _ = sc::sys_dup2(1, 1);
    }
    if stderr_fd >= 0 {
        let _ = sc::sys_dup2(2, 2);
    }

    result
}

fn execute_pipeline(state: &mut ShellState, cmds: &[ParsedCommand]) -> bool {
    let mut prev_read: i32 = -1;
    let n = cmds.len();
    let mut pipe_statuses: Vec<i32> = Vec::new();
    for (i, cmd) in cmds.iter().enumerate() {
        let mut pipe_fds: [usize; 2] = [0, 0];
        if i < n - 1 {
            if sc::sys_pipe(&mut pipe_fds).is_err() {
                eprintln_fn("pipe olusturulamadi");
                return false;
            }
        }
        let argv: Vec<String> = cmd.argv.clone();
        let argv_refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        let final_argv: Vec<&str> = argv_refs.clone();

        let pid = match sc::sys_fork() {
            Ok(0) => {
                if prev_read >= 0 {
                    let _ = sc::sys_dup2(prev_read as usize, 0);
                    let _ = sc::sys_close(prev_read as usize);
                }
                if i < n - 1 {
                    let _ = sc::sys_close(pipe_fds[0]);
                    let _ = sc::sys_dup2(pipe_fds[1], 1);
                    let _ = sc::sys_close(pipe_fds[1]);
                }

                if let Some(ref path) = cmd.redirect_out {
                    if let Ok(fd) = sc::sys_open(path, 1 | 0x200 | 0x240) {
                        let _ = sc::sys_dup2(fd, 1);
                        let _ = sc::sys_close(fd);
                    }
                }
                if let Some(ref path) = cmd.redirect_append {
                    if let Ok(fd) = sc::sys_open(path, 1 | 0x200 | 0x400) {
                        let _ = sc::sys_dup2(fd, 1);
                        let _ = sc::sys_close(fd);
                    }
                }
                if let Some(ref path) = cmd.redirect_in {
                    if let Ok(fd) = sc::sys_open(path, 0) {
                        let _ = sc::sys_dup2(fd, 0);
                        let _ = sc::sys_close(fd);
                    }
                }
                if let Some(ref path) = cmd.redirect_err {
                    if let Ok(fd) = sc::sys_open(path, 1 | 0x200 | 0x240) {
                        let _ = sc::sys_dup2(fd, 2);
                        let _ = sc::sys_close(fd);
                    }
                }
                if let Some(ref path) = cmd.redirect_err_append {
                    if let Ok(fd) = sc::sys_open(path, 1 | 0x200 | 0x400) {
                        let _ = sc::sys_dup2(fd, 2);
                        let _ = sc::sys_close(fd);
                    }
                }
                if let Some(ref content) = cmd.here_doc {
                    let tmp = "/tmp/.heredoc";
                    executor_write_file(tmp, content.as_bytes());
                    if let Ok(fd) = sc::sys_open(tmp, 0) {
                        let _ = sc::sys_dup2(fd, 0);
                        let _ = sc::sys_close(fd);
                    }
                }

                if !final_argv.is_empty() {
                    if builtins::is_builtin(final_argv[0]) {
                        builtins::dispatch(state, &final_argv);
                        sc::sys_exit(0);
                    } else {
                        let envp: Vec<&str> = Vec::new();
                        let _ = sc::sys_execve(final_argv[0], &final_argv, &envp);
                        eprintln_fn(&format!("echshell: {} not found", final_argv[0]));
                        sc::sys_exit(127);
                    }
                }
                sc::sys_exit(0);
            }
            Ok(pid) => pid,
            Err(_) => {
                eprintln_fn("fork basarisiz");
                return false;
            }
        };

        if prev_read >= 0 {
            let _ = sc::sys_close(prev_read as usize);
        }
        if i < n - 1 {
            let _ = sc::sys_close(pipe_fds[1]);
            prev_read = pipe_fds[0] as i32;
        }
        if !cmd.background {
            let mut status: i32 = 0;
            let _ = sc::sys_wait4(pid as isize, &mut status, 0);
            pipe_statuses.push(status);
            state.exit_code = status;
        } else {
            pipe_statuses.push(0);
        }
    }
    // Set PIPESTATUS array for ${PIPESTATUS[@]} access
    state.env.set_pipestatus(&pipe_statuses);
    true
}

pub fn expand_command_substitutions(state: &mut ShellState, input: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        if chars[i] == '$' && i + 1 < len && chars[i + 1] == '(' {
            if i + 2 < len && chars[i + 2] == '(' {
                result.push(chars[i]);
                i += 1;
            } else {
                i += 2;
                let mut depth = 1;
                let mut cmd = String::new();
                while i < len && depth > 0 {
                    if chars[i] == '(' {
                        depth += 1;
                        cmd.push('(');
                    } else if chars[i] == ')' {
                        depth -= 1;
                        if depth > 0 {
                            cmd.push(')');
                        }
                    } else {
                        cmd.push(chars[i]);
                    }
                    i += 1;
                }
                let output = execute_and_capture(state, &cmd);
                result.push_str(&output);
            }
        } else if chars[i] == '`' {
            i += 1;
            let mut cmd = String::new();
            while i < len && chars[i] != '`' {
                if chars[i] == '\\' && i + 1 < len {
                    i += 1;
                    cmd.push(chars[i]);
                } else {
                    cmd.push(chars[i]);
                }
                i += 1;
            }
            if i < len {
                i += 1;
            }
            let output = execute_and_capture(state, &cmd);
            result.push_str(&output);
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

fn execute_and_capture(state: &mut ShellState, cmd: &str) -> String {
    let mut pipe_fds = [0usize; 2];
    if sc::sys_pipe(&mut pipe_fds).is_err() {
        return String::new();
    }
    match sc::sys_fork() {
        Ok(0) => {
            let _ = sc::sys_close(pipe_fds[0]);
            let _ = sc::sys_dup2(pipe_fds[1], 1);
            let _ = sc::sys_close(pipe_fds[1]);
            let args: Vec<&str> = cmd.split_whitespace().filter(|s| !s.is_empty()).collect();
            if !args.is_empty() {
                let envp: Vec<&str> = Vec::new();
                let _ = sc::sys_execve(args[0], &args, &envp);
            }
            sc::sys_exit(1);
        }
        Ok(pid) => {
            let _ = sc::sys_close(pipe_fds[1]);
            let mut output = String::new();
            let mut buf = [0u8; 4096];
            loop {
                match sc::sys_read(pipe_fds[0], &mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                            output.push_str(s);
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = sc::sys_close(pipe_fds[0]);
            let mut status: i32 = 0;
            let _ = sc::sys_wait4(pid as isize, &mut status, 0);
            state.exit_code = status;
            output.trim_end_matches('\n').to_string()
        }
        Err(_) => String::new(),
    }
}

pub fn load_file(path: &str) -> Option<Vec<u8>> {
    let fd = sc::sys_open(path, 0).ok()?;
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match sc::sys_read(fd, &mut buf) {
            Ok(0) => break,
            Ok(n) => data.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
    }
    let _ = sc::sys_close(fd);
    Some(data)
}

pub fn write_file(path: &str, data: &[u8]) -> bool {
    if let Ok(fd) = sc::sys_open(path, 1 | 0x200 | 0x240) {
        let _ = sc::sys_write(fd, data);
        let _ = sc::sys_close(fd);
        true
    } else {
        false
    }
}

fn executor_write_file(path: &str, data: &[u8]) -> bool {
    write_file(path, data)
}

pub fn append_file(path: &str, data: &[u8]) -> bool {
    if let Ok(fd) = sc::sys_open(path, 1 | 0x200 | 0x400) {
        let _ = sc::sys_write(fd, data);
        let _ = sc::sys_close(fd);
        true
    } else {
        false
    }
}
