use super::*;

pub(super) fn decode_terminal_output(raw: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut index = 0;

    while index < raw.len() {
        match raw[index] {
            0x1B => {
                if index + 1 < raw.len() && raw[index + 1] == b'[' {
                    index += 2;
                    let mut final_byte = 0;
                    while index < raw.len() {
                        let byte = raw[index];
                        if (byte as char).is_ascii_alphabetic() {
                            final_byte = byte;
                            break;
                        }
                        index += 1;
                    }
                    if final_byte == b'J' {
                        lines.push(String::from("__CLEAR__"));
                        current.clear();
                    }
                }
            }
            b'\r' => {}
            b'\n' => {
                if !current.trim_end().is_empty() {
                    lines.push(current.trim_end().to_string());
                }
                current.clear();
            }
            0x08 => {
                current.pop();
            }
            byte if byte.is_ascii_graphic() || byte == b' ' => current.push(byte as char),
            _ => {}
        }
        index += 1;
    }

    if !current.trim_end().is_empty() {
        lines.push(current.trim_end().to_string());
    }

    lines
}

pub(super) fn parent_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "/" {
        return String::from("/");
    }
    let mut parts = trimmed.rsplitn(2, '/');
    let _name = parts.next();
    let parent = parts.next().unwrap_or("");
    if parent.is_empty() {
        String::from("/")
    } else {
        format!("/{}", parent.trim_start_matches('/'))
    }
}

pub(super) fn join_path(base: &str, name: &str) -> String {
    if base == "/" {
        format!("/{}", name.trim_start_matches('/'))
    } else {
        format!(
            "{}/{}",
            base.trim_end_matches('/'),
            name.trim_start_matches('/')
        )
    }
}

pub(super) fn entry_launch_kind(path: &str) -> Option<AppKind> {
    match path {
        "/proc" | "/sys" | "/dev" => Some(AppKind::Files),
        "/settings" => Some(AppKind::Settings),
        _ => None,
    }
}

pub(super) fn file_association_kind(path: &str) -> Option<AppKind> {
    if let Some(kind) = entry_launch_kind(path) {
        return Some(kind);
    }
    match path.rsplit('.').next() {
        Some("html" | "htm") => Some(AppKind::Browser),
        Some("txt" | "rs" | "md" | "cfg" | "json" | "toml" | "log") => Some(AppKind::Editor),
        Some("png" | "jpg" | "jpeg" | "bmp") => Some(AppKind::Files),
        _ => Some(AppKind::Editor),
    }
}

pub(super) fn file_association_label(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html" | "htm") => "browser",
        Some("txt" | "rs" | "md" | "cfg" | "json" | "toml" | "log") => "editor",
        Some("png" | "jpg" | "jpeg" | "bmp") => "preview",
        _ => "open",
    }
}

pub(super) fn browser_http_error_label(err: crate::net::http::HttpError) -> String {
    format!("browser request failed: {:?}", err)
}

pub(super) fn normalize_browser_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::from("https://example.com/");
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }
    format!("https://{}", trimmed)
}

pub(super) fn browser_window_title(url: &str) -> String {
    HttpUrl::parse(url)
        .ok()
        .map(|parsed| parsed.host)
        .unwrap_or_else(|| String::from("Web"))
}

pub(super) fn browser_response_is_html(
    url: &str,
    response: &crate::net::http::HttpResponse,
) -> bool {
    response
        .headers
        .get("content-type")
        .map(|content_type| {
            content_type.contains("text/html") || content_type.contains("application/xhtml")
        })
        .unwrap_or_else(|| {
            url.ends_with(".html")
                || url.ends_with(".htm")
                || response.body.starts_with(b"<!DOCTYPE html")
                || response.body.starts_with(b"<html")
        })
}

pub(super) fn browser_plain_preview(body: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for line in body.lines() {
        let normalized = collapse_browser_whitespace(line);
        if !normalized.is_empty() {
            lines.push(normalized);
        }
        if lines.len() >= 14 {
            break;
        }
    }
    lines
}

pub(super) fn browser_binary_preview(
    url: &str,
    content_type: &str,
    body_len: usize,
) -> Vec<String> {
    let file_name = browser_download_path(url, content_type);
    vec![
        format!("Binary response: {}", content_type),
        format!("Size: {} bytes", body_len),
        format!("Download target: {}", file_name),
        String::from("Use Download to store this payload under /downloads."),
    ]
}

#[derive(Default)]
pub(super) struct BrowserDocument {
    pub(super) lines: Vec<String>,
    pub(super) links: Vec<BrowserLink>,
}

pub(super) fn parse_browser_document(base_url: &str, html: &str) -> BrowserDocument {
    let mut document = BrowserDocument::default();
    let mut current_anchor_href: Option<String> = None;
    let mut current_anchor_text = String::new();
    let mut in_title = false;

    for token in HtmlTokenizer::from(html) {
        let Ok(token) = token else {
            continue;
        };
        match token {
            HtmlToken::ElementStart { local, .. } => {
                let tag = local.as_str();
                if tag.eq_ignore_ascii_case("a") {
                    current_anchor_href = Some(String::new());
                    current_anchor_text.clear();
                } else if tag.eq_ignore_ascii_case("title") {
                    in_title = true;
                }
            }
            HtmlToken::Attribute { local, value, .. } => {
                if let Some(href) = current_anchor_href.as_mut() {
                    if local.as_str().eq_ignore_ascii_case("href") {
                        *href = resolve_browser_url(
                            base_url,
                            value.map(|span| span.as_str()).unwrap_or(""),
                        );
                    }
                }
            }
            HtmlToken::Text { text } => {
                let normalized = collapse_browser_whitespace(text.as_str());
                if normalized.is_empty() {
                    continue;
                }
                if in_title && document.lines.is_empty() {
                    document.lines.push(format!("Title: {}", normalized));
                } else if document.lines.len() < 12 {
                    document.lines.push(normalized.clone());
                }
                if current_anchor_href.is_some() {
                    if !current_anchor_text.is_empty() {
                        current_anchor_text.push(' ');
                    }
                    current_anchor_text.push_str(&normalized);
                }
            }
            HtmlToken::ElementEnd { end, .. } => match end {
                HtmlElementEnd::Close(_, local) => {
                    let tag = local.as_str();
                    if tag.eq_ignore_ascii_case("title") {
                        in_title = false;
                    }
                    if tag.eq_ignore_ascii_case("a") {
                        finalize_browser_link(
                            &mut document.links,
                            current_anchor_href.take(),
                            &mut current_anchor_text,
                        );
                    }
                }
                HtmlElementEnd::Empty => {
                    finalize_browser_link(
                        &mut document.links,
                        current_anchor_href.take(),
                        &mut current_anchor_text,
                    );
                }
                HtmlElementEnd::Open => {}
            },
            _ => {}
        }
    }

    finalize_browser_link(
        &mut document.links,
        current_anchor_href.take(),
        &mut current_anchor_text,
    );
    if document.lines.is_empty() {
        document.lines.push(String::from(
            "HTML response fetched. No preview text surfaced.",
        ));
    }
    document.links.truncate(6);
    document
}

pub(super) fn finalize_browser_link(
    links: &mut Vec<BrowserLink>,
    href: Option<String>,
    current_anchor_text: &mut String,
) {
    let Some(url) = href else {
        current_anchor_text.clear();
        return;
    };
    if url.is_empty() {
        current_anchor_text.clear();
        return;
    }
    let label = if current_anchor_text.trim().is_empty() {
        browser_window_title(&url)
    } else {
        current_anchor_text.trim().to_string()
    };
    links.push(BrowserLink { label, url });
    current_anchor_text.clear();
}

pub(super) fn collapse_browser_whitespace(text: &str) -> String {
    let mut normalized = String::new();
    let mut last_was_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                normalized.push(' ');
                last_was_space = true;
            }
        } else {
            normalized.push(ch);
            last_was_space = false;
        }
    }
    normalized.trim().to_string()
}

pub(super) fn resolve_browser_url(base_url: &str, raw_href: &str) -> String {
    let href = raw_href.trim();
    if href.is_empty() {
        return String::new();
    }
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    let Ok(base) = HttpUrl::parse(base_url) else {
        return href.to_string();
    };
    let mut resolved = String::new();
    resolved.push_str(&base.scheme);
    resolved.push_str("://");
    resolved.push_str(&base.host);
    if (base.scheme == "http" && base.port != 80) || (base.scheme == "https" && base.port != 443) {
        resolved.push(':');
        resolved.push_str(&base.port.to_string());
    }
    if href.starts_with('/') {
        resolved.push_str(href);
        return resolved;
    }
    let parent = if let Some((prefix, _)) = base.path.rsplit_once('/') {
        if prefix.is_empty() {
            "/"
        } else {
            prefix
        }
    } else {
        "/"
    };
    resolved.push_str(parent);
    if !resolved.ends_with('/') {
        resolved.push('/');
    }
    resolved.push_str(href);
    resolved
}

pub(super) fn browser_download_path(url: &str, content_type: &str) -> String {
    let parsed = HttpUrl::parse(url).ok();
    let candidate = parsed
        .as_ref()
        .and_then(|parsed| parsed.path.rsplit('/').find(|segment| !segment.is_empty()))
        .unwrap_or("");
    let fallback = match content_type {
        ct if ct.contains("html") => "index.html",
        ct if ct.contains("json") => "payload.json",
        ct if ct.contains("text") => "download.txt",
        _ => "download.bin",
    };
    let chosen = if candidate.is_empty() || !candidate.contains('.') {
        fallback
    } else {
        candidate
    };
    let sanitized: String = candidate
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => ch,
        })
        .collect();
    let sanitized = if sanitized.is_empty() {
        chosen.to_string()
    } else if chosen == fallback && !candidate.contains('.') {
        fallback.to_string()
    } else {
        sanitized
    };
    format!("/downloads/{}", sanitized)
}

pub(super) fn browser_start_page_lines() -> Vec<String> {
    vec![
        String::from("Native browser shell is ready."),
        String::from("Type or paste a URL into the address bar, then press Enter."),
        String::from("Use Open to fetch a page and Download to save the current URL."),
        String::from("Downloaded files are written to /downloads."),
    ]
}

pub(super) fn browser_address_rect(width: i32) -> Rect {
    Rect::new(18, 54, (width - 288).max(180) as u32, 32)
}

pub(super) fn browser_open_button_rect(width: i32) -> Rect {
    Rect::new(width - 252, 54, 70, 32)
}

pub(super) fn browser_refresh_button_rect(width: i32) -> Rect {
    Rect::new(width - 170, 54, 80, 32)
}

pub(super) fn browser_download_button_rect(width: i32) -> Rect {
    Rect::new(width - 78, 54, 60, 32)
}

pub(super) fn browser_link_rect(index: usize, width: i32) -> Rect {
    Rect::new(
        18,
        356 + index as i32 * 52,
        (width - 36).max(120) as u32,
        42,
    )
}

pub(super) fn browser_link_hit(local: Point, width: i32, link_count: usize) -> Option<usize> {
    (0..link_count.min(6)).find(|index| browser_link_rect(*index, width).contains(local))
}

pub(super) fn thumbnail_color_for_path(path: &str) -> u32 {
    match path.rsplit('.').next() {
        Some("png" | "jpg" | "jpeg" | "bmp") => ACCENT_CORAL,
        Some("rs" | "toml") => ACCENT_BLUE,
        Some("cfg" | "json" | "log") => ACCENT_GOLD,
        _ => ACCENT_SOFT,
    }
}

pub(super) fn thumbnail_label_for_path(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("png" | "jpg" | "jpeg" | "bmp") => "I",
        Some("rs") => "R",
        Some("toml" | "cfg" | "json") => "C",
        _ => "F",
    }
}
impl TerminalApp {
    pub(super) fn ensure_window(&mut self, screen: Rect) -> Result<LaunchResult, String> {
        self.ensure_backend()?;
        let result = ensure_window_visible(
            &self.client,
            &mut self.window,
            "Terminal",
            Rect::new(screen.x + 86, screen.y + 126, 720, 420),
            self.workspace_id,
        )?;
        self.sync_winsize();
        self.dirty = true;
        Ok(result)
    }

    pub(super) fn ensure_backend(&mut self) -> Result<(), String> {
        if self.pty.is_some() {
            return Ok(());
        }

        let pair = PTY_MANAGER
            .create_pair()
            .map_err(|err| format!("pty create failed: {:?}", err))?;
        configure_pty_for_shell(&pair);
        write_welcome_message(&pair);
        self.pty = Some(pair);
        self.pull_pty_output();
        Ok(())
    }

    pub(super) fn sync(&mut self, active_workspace: u8) -> bool {
        match sync_window_state(
            &self.client,
            &mut self.window,
            self.workspace_id,
            active_workspace,
        ) {
            WindowSync::Changed => {
                self.sync_winsize();
                self.dirty = true;
                true
            }
            WindowSync::Closed => {
                let _ = self.client.mark_app_exited(true, "window closed");
                self.dirty = true;
                true
            }
            WindowSync::Unchanged => false,
        }
    }

    pub(super) fn handle_events(
        &mut self,
        events: Vec<WindowInputEvent>,
        commands: &mut Vec<SessionCommand>,
    ) {
        let Some(window_id) = self.window.as_ref().map(|window| window.window_id) else {
            return;
        };

        for event in events {
            if event.window_id != window_id {
                continue;
            }
            if is_backspace_key(&event.event) {
                self.input.pop();
                self.dirty = true;
            } else if is_enter_key(&event.event) {
                self.submit_line(commands);
            } else if let Some(ch) = printable_key(&event.event) {
                self.input.push(ch);
                self.dirty = true;
            }
        }
    }

    pub(super) fn submit_line(&mut self, commands: &mut Vec<SessionCommand>) {
        let command = self.input.trim().to_string();
        if command.is_empty() {
            self.input.clear();
            self.dirty = true;
            return;
        }

        match command.as_str() {
            "help" => {
                self.lines.push(String::from(
                    "local: clear | open terminal|files|web|settings|editor | copy <text> | paste | open-file | save-file | pick-folder | screenshot | grants | accessibility",
                ));
                self.lines.push(String::from(
                    "shell: pwd | cd <dir> | ls [path] | tree [path] | find [path] -name <glob> | stat <path> | cp <src> <dst> | mv | rm | mkdir | touch | head | tail | wc | grep | sort | uniq | env | history | alias | which | command",
                ));
            }
            "clear" => {
                self.lines.clear();
                self.lines.push(String::from("screen cleared"));
            }
            "open terminal" => commands.push(SessionCommand::Launch(LaunchIntent::new(
                AppKind::Terminal.descriptor(),
                ExecutionContext::new(
                    LaunchSource::ShellShortcut,
                    self.workspace_id,
                    "terminal-open",
                ),
            ))),
            "open files" => commands.push(SessionCommand::Launch(LaunchIntent::new(
                AppKind::Files.descriptor(),
                ExecutionContext::new(
                    LaunchSource::ShellShortcut,
                    self.workspace_id,
                    "terminal-open",
                ),
            ))),
            "open settings" => commands.push(SessionCommand::Launch(LaunchIntent::new(
                AppKind::Settings.descriptor(),
                ExecutionContext::new(
                    LaunchSource::ShellShortcut,
                    self.workspace_id,
                    "terminal-open",
                ),
            ))),
            "open web" => commands.push(SessionCommand::Launch(LaunchIntent::new(
                AppKind::Browser.descriptor(),
                ExecutionContext::new(
                    LaunchSource::ShellShortcut,
                    self.workspace_id,
                    "terminal-open",
                ),
            ))),
            "open firefox" | "open chromium" | "open chrome" | "open cef" => {
                if let Some(resolution) =
                    crate::runtime_layer::package_registry_contract::RuntimePackageRegistry::new(
                        &desktop_launch_registry(),
                    )
                    .resolve_with_probe(&command, launch::launch_path_exists)
                {
                    if let Some(candidates) = resolution.missing_candidates() {
                        self.lines.push(format!(
                            "{} binary not found; searched {}",
                            resolution.descriptor().title,
                            candidates.join(", ")
                        ));
                    } else {
                        let intent = resolution.launch_intent(ExecutionContext::new(
                            LaunchSource::ShellShortcut,
                            self.workspace_id,
                            "terminal-browser-binary",
                        ));
                        if let Some(path) = resolution.path() {
                            commands.push(SessionCommand::LaunchExternal(intent, path.to_string()));
                        } else {
                            commands.push(SessionCommand::Launch(intent));
                        }
                    }
                }
            }
            "open editor" => commands.push(SessionCommand::Launch(LaunchIntent::new(
                AppKind::Editor.descriptor(),
                ExecutionContext::new(
                    LaunchSource::ShellShortcut,
                    self.workspace_id,
                    "terminal-open",
                ),
            ))),
            "paste" => match self.client.clipboard_get() {
                Ok(ClipboardPayload::Text(text)) => self.lines.push(format!("clipboard: {}", text)),
                Ok(ClipboardPayload::Files(paths)) => {
                    self.lines.push(format!("clipboard files: {}", paths.len()))
                }
                Ok(ClipboardPayload::Empty) => self.lines.push(String::from("clipboard empty")),
                Err(_) => self.lines.push(String::from("clipboard unavailable")),
            },
            "open-file" => {
                if let Ok(dialog_id) = self
                    .client
                    .open_file_dialog("Open File", "/workspace/demo.txt")
                {
                    self.pending_dialogs.push(dialog_id);
                    self.lines.push(String::from("file dialog requested"));
                }
            }
            "save-file" => {
                if let Ok(dialog_id) = self
                    .client
                    .save_file_dialog("Save File", "/workspace/output.txt")
                {
                    self.pending_dialogs.push(dialog_id);
                    self.lines.push(String::from("save dialog requested"));
                }
            }
            "pick-folder" => {
                if let Ok(dialog_id) = self.client.pick_folder_dialog("Pick Folder", "/workspace") {
                    self.pending_dialogs.push(dialog_id);
                    self.lines.push(String::from("folder dialog requested"));
                }
            }
            other if other.starts_with("notify ") => {
                commands.push(SessionCommand::Notify(other[7..].trim().to_string()));
            }
            other if other.starts_with("copy ") => {
                let text = other[5..].trim();
                let _ = self
                    .client
                    .clipboard_set(ClipboardPayload::Text(String::from(text)));
                self.lines.push(String::from("clipboard updated"));
            }
            "screenshot" => match self.client.capture_screen("terminal-request") {
                Ok(entry) => self.lines.push(format!(
                    "capture {} {}x{}",
                    entry.id, entry.width, entry.height
                )),
                Err(err) => self.lines.push(format!("capture failed: {}", err)),
            },
            "screenshot-save" => match self.client.capture_screen("terminal-save") {
                Ok(entry) => {
                    let path = format!("/workspace/capture-{}.ppm", entry.id);
                    match self.client.save_capture_ppm(entry.id, &path) {
                        Ok(()) => self.lines.push(format!("saved {}", path)),
                        Err(err) => self.lines.push(format!("save failed: {}", err)),
                    }
                }
                Err(err) => self.lines.push(format!("capture failed: {}", err)),
            },
            "grants" => match self.client.list_file_grants() {
                Ok(grants) => {
                    for grant in grants {
                        self.lines.push(format!("grant {}", grant.path_prefix));
                    }
                }
                Err(err) => self.lines.push(format!("grant list failed: {}", err)),
            },
            "accessibility" => match self.client.accessibility_tree() {
                Ok(nodes) => self.lines.push(format!("a11y nodes: {}", nodes.len())),
                Err(err) => self.lines.push(format!("a11y failed: {}", err)),
            },
            _ => {
                if self.execute_pty_command(&command).is_err() {
                    self.lines.push(String::from("command execution failed"));
                }
            }
        }

        while self.lines.len() > 22 {
            self.lines.remove(0);
        }
        self.input.clear();
        self.dirty = true;
    }

    pub(super) fn execute_pty_command(&mut self, command: &str) -> Result<(), String> {
        self.ensure_backend()?;
        let Some(pair) = self.pty.as_ref() else {
            return Err(String::from("pty unavailable"));
        };

        let _ = pair.master.write(command.as_bytes());
        let _ = pair.master.write(b"\n");
        let _ = execute_command_on_pty_with_shell(pair, &mut self.shell, command);
        let _ = pair.slave.write(b"$ ");
        self.pull_pty_output();
        Ok(())
    }

    pub(super) fn pull_pty_output(&mut self) {
        let Some(pair) = self.pty.as_ref() else {
            return;
        };
        if !pty_has_output(pair) {
            return;
        }

        let mut raw = Vec::new();
        let mut chunk = [0u8; 512];
        loop {
            let Ok(read) = pair.master.read(&mut chunk) else {
                break;
            };
            if read == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..read]);
            if read < chunk.len() {
                break;
            }
        }

        if raw.is_empty() {
            return;
        }

        for line in decode_terminal_output(&raw) {
            if line == "__CLEAR__" {
                self.lines.clear();
            } else {
                self.lines.push(line);
            }
        }
        while self.lines.len() > 128 {
            self.lines.remove(0);
        }
        self.dirty = true;
    }

    pub(super) fn poll_platform(&mut self) {
        let mut completed = Vec::new();
        for dialog_id in self.pending_dialogs.iter().copied() {
            if let Ok(Some(result)) = self.client.poll_dialog_result(dialog_id) {
                match result.selection {
                    DialogSelection::Accepted(path) => {
                        self.lines.push(format!("dialog accepted: {}", path));
                    }
                    DialogSelection::Cancelled => {
                        self.lines.push(String::from("dialog cancelled"));
                    }
                }
                completed.push(dialog_id);
                self.dirty = true;
            }
        }
        self.pending_dialogs
            .retain(|dialog_id| !completed.iter().any(|done| done == dialog_id));
        self.pull_pty_output();
    }

    pub(super) fn sync_winsize(&mut self) {
        let Some(pair) = self.pty.as_ref() else {
            return;
        };
        let Some(window) = self.window else {
            return;
        };

        let cols = max((window.content_rect.width as i32 - 36) / FONT_WIDTH, 20) as u16;
        let rows = max((window.content_rect.height as i32 - 118) / 18, 8) as u16;
        pair.slave.set_winsize(Winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: window.content_rect.width as u16,
            ws_ypixel: window.content_rect.height as u16,
        });
    }

    pub(super) fn render(&mut self) -> Result<(), String> {
        let Some(window) = self.window else {
            return Ok(());
        };
        if !window.visible || !self.dirty {
            return Ok(());
        }

        let mut canvas = Canvas::new(
            window.content_rect.width as usize,
            window.content_rect.height as usize,
            WINDOW_BG,
        );
        canvas.fill_rect(Rect::new(0, 0, window.content_rect.width, 56), PANEL_BG);
        canvas.fill_rect(Rect::new(0, 56, window.content_rect.width, 1), BORDER);
        canvas.draw_text(18, 18, "Native Terminal", TEXT_PRIMARY);
        canvas.draw_text(18, 72, "Commands", TEXT_MUTED);

        let available_rows = ((window.content_rect.height as i32 - 138).max(0) / 18) as usize;
        let start = self.lines.len().saturating_sub(available_rows);
        let mut y = 96;
        for line in self.lines.iter().skip(start) {
            canvas.draw_text(18, y, line, TEXT_SECONDARY);
            y += 18;
        }

        let footer_y = max(window.content_rect.height as i32 - 34, 0);
        canvas.fill_rect(
            Rect::new(0, footer_y, window.content_rect.width, 34),
            PANEL_BG,
        );
        canvas.fill_rect(Rect::new(0, footer_y, window.content_rect.width, 1), BORDER);
        canvas.draw_text(18, footer_y + 10, ">", ACCENT_MINT);
        canvas.draw_text(34, footer_y + 10, &self.input, TEXT_PRIMARY);

        self.client
            .present(window.window_id, &canvas.into_pixels())?;
        self.dirty = false;
        Ok(())
    }

    pub(super) fn snapshot(&self) -> AppSnapshot {
        snapshot_for_window(
            AppKind::Terminal,
            self.window,
            self.workspace_id,
            format!("pty {} lines", self.lines.len().saturating_sub(1)),
        )
    }

    pub(super) fn publish_accessibility(&self) {
        let Some(window) = self.window else {
            return;
        };
        let nodes = vec![
            AccessibilityNode {
                id: 1,
                app_id: self.client.app_id(),
                role: AccessibilityRole::Window,
                label: String::from("Terminal"),
                description: String::from("pty terminal window"),
                focused: window.focused,
                bounds: window.content_rect,
            },
            AccessibilityNode {
                id: 2,
                app_id: self.client.app_id(),
                role: AccessibilityRole::Input,
                label: String::from("Command Input"),
                description: self.input.clone(),
                focused: window.focused,
                bounds: Rect::new(
                    0,
                    window.content_rect.height as i32 - 34,
                    window.content_rect.width,
                    34,
                ),
            },
        ];
        let _ = self.client.publish_accessibility_tree(nodes);
    }
}

impl FilesApp {
    pub(super) fn ensure_window(&mut self, screen: Rect) -> Result<LaunchResult, String> {
        let result = ensure_window_visible(
            &self.client,
            &mut self.window,
            "Files",
            Rect::new(screen.x + 232, screen.y + 168, 580, 360),
            self.workspace_id,
        )?;
        if crate::boot::appliance::auto_login_requested() && self.entries.is_empty() {
            self.status = format!("Deferred appliance scan for {}", self.current_path);
            self.dirty = true;
            return Ok(result);
        }
        self.refresh()?;
        self.dirty = true;
        Ok(result)
    }

    pub(super) fn sync(&mut self, active_workspace: u8) -> bool {
        match sync_window_state(
            &self.client,
            &mut self.window,
            self.workspace_id,
            active_workspace,
        ) {
            WindowSync::Changed => {
                self.dirty = true;
                true
            }
            WindowSync::Closed => {
                let _ = self.client.mark_app_exited(true, "window closed");
                self.dirty = true;
                true
            }
            WindowSync::Unchanged => false,
        }
    }

    pub(super) fn handle_events(
        &mut self,
        events: Vec<WindowInputEvent>,
        commands: &mut Vec<SessionCommand>,
    ) {
        let Some(window_id) = self.window.as_ref().map(|window| window.window_id) else {
            return;
        };

        for event in events {
            if event.window_id != window_id {
                continue;
            }
            let input = &event.event;
            match input {
                InputEvent::PointerButton {
                    state: KeyState::Pressed,
                    ..
                } => {
                    if let Some(local) = event.local_position {
                        if let Some(index) = files_hit(local, self.entries.len()) {
                            self.selected = index.min(self.entries.len().saturating_sub(1));
                            self.dirty = true;
                            self.activate_selected(commands);
                        }
                    }
                }
                InputEvent::Key { .. } => {
                    if ctrl_scan_pressed(input, 0x13) {
                        let _ = self.refresh();
                        continue;
                    }
                    if key_scan_pressed(input, 0x50) || key_scan_pressed(input, 0x24) {
                        self.selected =
                            (self.selected + 1).min(self.entries.len().saturating_sub(1));
                        self.dirty = true;
                    } else if key_scan_pressed(input, 0x48) || key_scan_pressed(input, 0x25) {
                        self.selected = self.selected.saturating_sub(1);
                        self.dirty = true;
                    } else if is_enter_key(input) {
                        self.activate_selected(commands);
                    } else if key_scan_pressed(input, 0x23) || is_backspace_key(input) {
                        let _ = self.navigate_up();
                    } else if key_scan_pressed(input, 0x20) {
                        let _ = self.delete_selected();
                    } else if key_scan_pressed(input, 0x32) {
                        let _ = self.rename_selected();
                    } else if key_scan_pressed(input, 0x31) {
                        let _ = self.create_directory("new-folder");
                    } else if key_scan_pressed(input, 0x13) {
                        let _ = self.refresh();
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn refresh(&mut self) -> Result<(), String> {
        let entries = self.client.list_directory(&self.current_path)?;
        self.entries = entries;
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
        self.status = format!("{} items in {}", self.entries.len(), self.current_path);
        self.dirty = true;
        Ok(())
    }

    pub(super) fn activate_selected(&mut self, commands: &mut Vec<SessionCommand>) {
        let Some(entry) = self.entries.get(self.selected).cloned() else {
            return;
        };

        if entry.is_directory {
            self.current_path = entry.path;
            if let Err(err) = self.refresh() {
                self.status = err;
                self.dirty = true;
            }
            return;
        }

        if let Some(resolution) = crate::gui::launch_pipeline::resolve_external_image(&entry.path) {
            let intent = resolution.launch_intent(ExecutionContext::new(
                LaunchSource::FileAssociation,
                self.workspace_id,
                file_association_label(&entry.path),
            ));
            if let Some(path) = resolution.path() {
                commands.push(SessionCommand::LaunchExternal(intent, path.to_string()));
            }
            return;
        }

        if let Some(kind) = file_association_kind(&entry.path) {
            commands.push(SessionCommand::Launch(LaunchIntent::new(
                kind.descriptor(),
                ExecutionContext::new(
                    LaunchSource::FileAssociation,
                    self.workspace_id,
                    file_association_label(&entry.path),
                ),
            )));
            return;
        }

        commands.push(SessionCommand::OpenEditorPath(entry.path));
    }

    pub(super) fn navigate_up(&mut self) -> Result<(), String> {
        self.current_path = parent_path(&self.current_path);
        self.refresh()
    }

    pub(super) fn delete_selected(&mut self) -> Result<(), String> {
        let Some(entry) = self.entries.get(self.selected).cloned() else {
            return Ok(());
        };

        if entry.is_directory {
            self.client.delete_directory(&entry.path)?;
        } else {
            self.client.delete_file(&entry.path)?;
        }
        self.status = format!("Removed {}", entry.name);
        self.refresh()
    }

    pub(super) fn create_directory(&mut self, name: &str) -> Result<(), String> {
        let path = join_path(&self.current_path, name);
        self.client.create_directory(&path)?;
        self.status = format!("Created {}", path);
        self.refresh()
    }

    pub(super) fn rename_selected(&mut self) -> Result<(), String> {
        let Some(entry) = self.entries.get(self.selected).cloned() else {
            return Ok(());
        };
        let new_name = if let Some((stem, ext)) = entry.name.rsplit_once('.') {
            format!("{}-renamed.{}", stem, ext)
        } else {
            format!("{}-renamed", entry.name)
        };
        let new_path = join_path(&self.current_path, &new_name);
        self.client.rename_path(&entry.path, &new_path)?;
        self.status = format!("Renamed {} -> {}", entry.name, new_name);
        self.refresh()
    }

    pub(super) fn render(&mut self) -> Result<(), String> {
        let Some(window) = self.window else {
            return Ok(());
        };
        if !window.visible || !self.dirty {
            return Ok(());
        }

        let mut canvas = Canvas::new(
            window.content_rect.width as usize,
            window.content_rect.height as usize,
            WINDOW_BG,
        );
        canvas.fill_rect(Rect::new(0, 0, window.content_rect.width, 60), PANEL_BG);
        canvas.fill_rect(Rect::new(0, 60, window.content_rect.width, 1), BORDER);
        canvas.draw_text(18, 18, "Files", TEXT_PRIMARY);
        canvas.draw_text(18, 38, &self.current_path, TEXT_MUTED);

        let mut y = 86;
        for (index, entry) in self.entries.iter().enumerate() {
            let selected = index == self.selected;
            let rect = Rect::new(18, y - 8, window.content_rect.width.saturating_sub(36), 48);
            canvas.fill_rect(rect, if selected { PANEL_ALT } else { PANEL_BG });
            let accent = if entry.is_directory {
                ACCENT_BLUE
            } else {
                thumbnail_color_for_path(&entry.path)
            };
            canvas.stroke_rect(rect, if selected { accent } else { BORDER });
            canvas.fill_rect(Rect::new(rect.x + 12, rect.y + 12, 18, 18), accent);
            canvas.draw_text(
                rect.x + 16,
                rect.y + 18,
                thumbnail_label_for_path(&entry.path),
                WINDOW_BG,
            );
            canvas.draw_text(34, y + 4, &entry.name, TEXT_PRIMARY);
            let detail = if entry.is_directory {
                String::from("directory")
            } else {
                format!(
                    "{} bytes  {}",
                    entry.size,
                    file_association_label(&entry.path)
                )
            };
            canvas.draw_text(34, y + 22, &detail, TEXT_SECONDARY);
            y += 58;
            if y > window.content_rect.height as i32 - 56 {
                break;
            }
        }

        let footer_y = max(window.content_rect.height as i32 - 34, 0);
        canvas.fill_rect(
            Rect::new(0, footer_y, window.content_rect.width, 34),
            PANEL_BG,
        );
        canvas.fill_rect(Rect::new(0, footer_y, window.content_rect.width, 1), BORDER);
        canvas.draw_text(18, footer_y + 10, &self.status, TEXT_MUTED);

        self.client
            .present(window.window_id, &canvas.into_pixels())?;
        self.dirty = false;
        Ok(())
    }

    pub(super) fn snapshot(&self) -> AppSnapshot {
        snapshot_for_window(
            AppKind::Files,
            self.window,
            self.workspace_id,
            format!("{} [{}]", self.current_path, self.entries.len()),
        )
    }

    pub(super) fn publish_accessibility(&self) {
        let Some(window) = self.window else {
            return;
        };
        let mut nodes = vec![AccessibilityNode {
            id: 1,
            app_id: self.client.app_id(),
            role: AccessibilityRole::Window,
            label: String::from("Files"),
            description: self.current_path.clone(),
            focused: window.focused,
            bounds: window.content_rect,
        }];
        for (index, entry) in self.entries.iter().take(12).enumerate() {
            nodes.push(AccessibilityNode {
                id: (index + 2) as u64,
                app_id: self.client.app_id(),
                role: AccessibilityRole::ListItem,
                label: entry.name.clone(),
                description: entry.path.clone(),
                focused: index == self.selected,
                bounds: Rect::new(
                    18,
                    78 + index as i32 * 58,
                    window.content_rect.width.saturating_sub(36),
                    48,
                ),
            });
        }
        let _ = self.client.publish_accessibility_tree(nodes);
    }
}

impl BrowserApp {
    pub(super) fn show_start_page(&mut self) {
        self.address_input = String::from("https://example.com/");
        self.current_url = Some(String::from("echos://start"));
        self.status = String::from("Start page ready");
        self.preview_lines = browser_start_page_lines();
        self.links.clear();
        self.dirty = true;
    }

    pub(super) fn prime_homepage(&mut self) {
        if self.current_url.is_some() {
            return;
        }
        self.show_start_page();
    }

    pub(super) fn ensure_window(&mut self, screen: Rect) -> Result<LaunchResult, String> {
        let result = ensure_window_visible(
            &self.client,
            &mut self.window,
            "Web",
            Rect::new(screen.x + 196, screen.y + 118, 820, 520),
            self.workspace_id,
        )?;
        if self.current_url.is_none() {
            self.prime_homepage();
        }
        self.dirty = true;
        Ok(result)
    }

    pub(super) fn sync(&mut self, active_workspace: u8) -> bool {
        match sync_window_state(
            &self.client,
            &mut self.window,
            self.workspace_id,
            active_workspace,
        ) {
            WindowSync::Changed => {
                self.dirty = true;
                true
            }
            WindowSync::Closed => {
                let _ = self.client.mark_app_exited(true, "window closed");
                self.dirty = true;
                true
            }
            WindowSync::Unchanged => false,
        }
    }

    pub(super) fn handle_events(
        &mut self,
        events: Vec<WindowInputEvent>,
        _commands: &mut Vec<SessionCommand>,
    ) {
        let Some(window) = self.window else {
            return;
        };

        for event in events {
            if event.window_id != window.window_id {
                continue;
            }

            match &event.event {
                InputEvent::PointerButton {
                    button: crate::gui::protocol::PointerButton::Left,
                    state: KeyState::Pressed,
                    ..
                } => {
                    if let Some(local) = event.local_position {
                        if browser_open_button_rect(window.content_rect.width as i32)
                            .contains(local)
                        {
                            let _ = self.open_current_url();
                        } else if browser_download_button_rect(window.content_rect.width as i32)
                            .contains(local)
                        {
                            let _ = self.download_current_url();
                        } else if browser_refresh_button_rect(window.content_rect.width as i32)
                            .contains(local)
                        {
                            let _ = self.refresh_current_url();
                        } else if let Some(index) = browser_link_hit(
                            local,
                            window.content_rect.width as i32,
                            self.links.len(),
                        ) {
                            if let Some(link) = self.links.get(index) {
                                self.address_input = link.url.clone();
                                let _ = self.open_current_url();
                            }
                        }
                    }
                }
                InputEvent::Key { .. } => {
                    if ctrl_scan_pressed(&event.event, 0x20) {
                        let _ = self.download_current_url();
                    } else if ctrl_scan_pressed(&event.event, 0x13) {
                        let _ = self.refresh_current_url();
                    } else if is_backspace_key(&event.event) {
                        self.address_input.pop();
                        self.dirty = true;
                    } else if is_enter_key(&event.event) {
                        let _ = self.open_current_url();
                    } else if let Some(ch) = printable_key(&event.event) {
                        self.address_input.push(ch);
                        self.dirty = true;
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn poll_platform(&mut self) {}

    pub(super) fn open_current_url(&mut self) -> Result<(), String> {
        if !crate::net::smoltcp_driver::ensure_runtime_network() {
            return Err(String::from("network bootstrap failed"));
        }
        let normalized = normalize_browser_url(&self.address_input);
        self.address_input = normalized.clone();
        let response = HttpClient::new()
            .get(&normalized)
            .map_err(browser_http_error_label)?;
        self.apply_response(&normalized, response)
    }

    pub(super) fn refresh_current_url(&mut self) -> Result<(), String> {
        if self.current_url.is_none() {
            return self.open_current_url();
        }
        self.open_current_url()
    }

    pub(super) fn download_current_url(&mut self) -> Result<(), String> {
        if matches!(self.current_url.as_deref(), Some("echos://start")) {
            return Err(String::from("enter a concrete URL before downloading"));
        }
        if !crate::net::smoltcp_driver::ensure_runtime_network() {
            return Err(String::from("network bootstrap failed"));
        }
        let url = normalize_browser_url(
            self.current_url
                .as_deref()
                .unwrap_or(self.address_input.as_str()),
        );
        let bytes = HttpClient::new()
            .download(&url)
            .map_err(browser_http_error_label)?;
        let _ = self.client.create_directory("/downloads");
        let path = browser_download_path(&url, &self.content_type);
        self.client.write_file(&path, &bytes)?;
        let _ = self.client.notify(
            "Browser",
            &format!("Saved {}", path),
            NotificationLevel::Info,
        );
        self.status = format!("Saved {} ({} bytes)", path, bytes.len());
        self.preview_lines
            .insert(0, format!("Saved download: {}", path));
        self.preview_lines.truncate(18);
        self.dirty = true;
        Ok(())
    }

    pub(super) fn apply_response(
        &mut self,
        url: &str,
        response: crate::net::http::HttpResponse,
    ) -> Result<(), String> {
        self.current_url = Some(url.to_string());
        self.content_type = response
            .headers
            .get("content-type")
            .unwrap_or("application/octet-stream")
            .to_string();
        self.links.clear();
        if browser_response_is_html(url, &response) {
            let document = parse_browser_document(url, &response.body_as_string());
            self.preview_lines = document.lines;
            self.links = document.links;
        } else if self.content_type.starts_with("text/")
            || self.content_type.contains("json")
            || self.content_type.contains("xml")
        {
            self.preview_lines = browser_plain_preview(&response.body_as_string());
        } else {
            self.preview_lines =
                browser_binary_preview(url, &self.content_type, response.body.len());
        }
        if self.preview_lines.is_empty() {
            self.preview_lines
                .push(String::from("Response body is empty or not previewable."));
        }
        self.status = format!(
            "HTTP {}  {}  {} bytes",
            response.status_code,
            self.content_type,
            response.body.len()
        );
        if let Some(window) = self.window {
            let _ = self.client.set_title(
                window.window_id,
                &format!("Web - {}", browser_window_title(url)),
            );
        }
        let _ = self.client.mark_app_launched(&self.status);
        self.dirty = true;
        Ok(())
    }

    pub(super) fn render(&mut self) -> Result<(), String> {
        let Some(window) = self.window else {
            return Ok(());
        };
        if !window.visible || !self.dirty {
            return Ok(());
        }

        let mut canvas = Canvas::new(
            window.content_rect.width as usize,
            window.content_rect.height as usize,
            WINDOW_BG,
        );
        canvas.fill_rect(Rect::new(0, 0, window.content_rect.width, 94), PANEL_BG);
        canvas.fill_rect(Rect::new(0, 94, window.content_rect.width, 1), BORDER);
        canvas.draw_text(18, 16, "Web", TEXT_PRIMARY);
        canvas.draw_text(18, 36, &self.status, TEXT_MUTED);

        let address_rect = browser_address_rect(window.content_rect.width as i32);
        canvas.fill_rect(address_rect, 0xFF0A1420);
        canvas.stroke_rect(address_rect, ACCENT_BLUE);
        canvas.draw_text(
            address_rect.x + 10,
            address_rect.y + 8,
            &self.address_input,
            TEXT_PRIMARY,
        );

        for (rect, label, accent) in [
            (
                browser_open_button_rect(window.content_rect.width as i32),
                "Open",
                ACCENT_MINT,
            ),
            (
                browser_refresh_button_rect(window.content_rect.width as i32),
                "Refresh",
                ACCENT_SOFT,
            ),
            (
                browser_download_button_rect(window.content_rect.width as i32),
                "Download",
                ACCENT_GOLD,
            ),
        ] {
            canvas.fill_rect(rect, PANEL_ALT);
            canvas.stroke_rect(rect, accent);
            canvas.draw_text(rect.x + 12, rect.y + 8, label, TEXT_PRIMARY);
        }

        let mut y = 116;
        if let Some(url) = self.current_url.as_ref() {
            canvas.draw_text(18, y, url, TEXT_SECONDARY);
            y += 24;
        }

        for line in self.preview_lines.iter().take(12) {
            canvas.draw_text(18, y, line, TEXT_SECONDARY);
            y += 18;
            if y > window.content_rect.height as i32 - 120 {
                break;
            }
        }

        if !self.links.is_empty() {
            y += 8;
            canvas.draw_text(18, y, "Links", TEXT_PRIMARY);
            y += 22;
            for (index, link) in self.links.iter().take(6).enumerate() {
                let rect = browser_link_rect(index, window.content_rect.width as i32);
                canvas.fill_rect(rect, PANEL_BG);
                canvas.stroke_rect(rect, ACCENT_BLUE);
                canvas.draw_text(rect.x + 12, rect.y + 6, &link.label, TEXT_PRIMARY);
                canvas.draw_text(rect.x + 12, rect.y + 24, &link.url, TEXT_MUTED);
            }
        }

        let footer_y = max(window.content_rect.height as i32 - 34, 0);
        canvas.fill_rect(
            Rect::new(0, footer_y, window.content_rect.width, 34),
            PANEL_BG,
        );
        canvas.fill_rect(Rect::new(0, footer_y, window.content_rect.width, 1), BORDER);
        canvas.draw_text(
            18,
            footer_y + 10,
            "Enter URL, Enter to open, Ctrl+D to download, Ctrl+R to refresh",
            TEXT_MUTED,
        );

        self.client
            .present(window.window_id, &canvas.into_pixels())?;
        self.dirty = false;
        Ok(())
    }

    pub(super) fn snapshot(&self) -> AppSnapshot {
        let detail = self
            .current_url
            .clone()
            .unwrap_or_else(|| self.status.clone());
        snapshot_for_window(AppKind::Browser, self.window, self.workspace_id, detail)
    }

    pub(super) fn publish_accessibility(&self) {
        let Some(window) = self.window else {
            return;
        };
        let mut nodes = vec![
            AccessibilityNode {
                id: 1,
                app_id: self.client.app_id(),
                role: AccessibilityRole::Window,
                label: String::from("Web"),
                description: self.status.clone(),
                focused: window.focused,
                bounds: window.content_rect,
            },
            AccessibilityNode {
                id: 2,
                app_id: self.client.app_id(),
                role: AccessibilityRole::Input,
                label: String::from("Address"),
                description: self.address_input.clone(),
                focused: window.focused,
                bounds: browser_address_rect(window.content_rect.width as i32),
            },
        ];
        for (index, link) in self.links.iter().take(6).enumerate() {
            nodes.push(AccessibilityNode {
                id: (index + 3) as u64,
                app_id: self.client.app_id(),
                role: AccessibilityRole::Button,
                label: link.label.clone(),
                description: link.url.clone(),
                focused: false,
                bounds: browser_link_rect(index, window.content_rect.width as i32),
            });
        }
        let _ = self.client.publish_accessibility_tree(nodes);
    }
}

impl SettingsApp {
    pub(super) fn ensure_window(&mut self, screen: Rect) -> Result<LaunchResult, String> {
        let result = ensure_window_visible(
            &self.client,
            &mut self.window,
            "Settings",
            Rect::new(screen.x + 314, screen.y + 108, 480, 520),
            self.workspace_id,
        )?;
        self.dirty = true;
        Ok(result)
    }

    pub(super) fn sync(&mut self, active_workspace: u8) -> bool {
        match sync_window_state(
            &self.client,
            &mut self.window,
            self.workspace_id,
            active_workspace,
        ) {
            WindowSync::Changed => {
                self.dirty = true;
                true
            }
            WindowSync::Closed => {
                let _ = self.client.mark_app_exited(true, "window closed");
                self.dirty = true;
                true
            }
            WindowSync::Unchanged => false,
        }
    }

    pub(super) fn handle_events(&mut self, events: Vec<WindowInputEvent>) {
        let Some(window_id) = self.window.as_ref().map(|window| window.window_id) else {
            return;
        };

        for event in events {
            if event.window_id != window_id {
                continue;
            }
            let input = &event.event;
            match input {
                InputEvent::PointerButton {
                    state: KeyState::Pressed,
                    ..
                } => {
                    if let Some(local) = event.local_position {
                        if let Some(index) = settings_hit(local) {
                            self.toggle(index);
                        }
                    }
                }
                InputEvent::Key { .. } => match digit_key_pressed(input) {
                    Some(1) => self.toggle(0),
                    Some(2) => self.toggle(1),
                    Some(3) => self.toggle(2),
                    Some(4) => self.toggle(3),
                    Some(5) => self.toggle(4),
                    Some(6) => self.toggle(5),
                    _ => {}
                },
                _ => {}
            }
        }
    }

    pub(super) fn cycle_motion_profile(&mut self) {
        let current = self
            .client
            .motion_profile()
            .unwrap_or(MotionProfile::Standard);
        let next = next_motion_profile(current);
        let _ = self.client.set_motion_profile(next);
        self.animations = next != MotionProfile::Reduced;
    }

    pub(super) fn cycle_shell_density(&self) {
        let current = self
            .client
            .shell_density()
            .unwrap_or(ShellDensityProfile::Balanced);
        let _ = self
            .client
            .set_shell_density(next_shell_density_profile(current));
    }

    pub(super) fn cycle_display_scale(&self) {
        let Ok(mut profile) = self.client.display_profile() else {
            return;
        };
        let scales = profile.capability.supported_scales_100x.clone();
        if let Some(output) = profile
            .outputs
            .iter_mut()
            .find(|output| output.output_id == profile.primary_output)
        {
            output.scale_100x = next_supported_scale(&scales, output.scale_100x);
            output.text_scale_100x = output.scale_100x;
        }
        let _ = self.client.set_display_profile(profile);
    }

    pub(super) fn toggle_accessibility_core(&self) {
        let mut profile = self.client.accessibility_profile().unwrap_or_default();
        if !profile.screen_reader {
            profile.screen_reader = true;
        } else if !profile.magnifier {
            profile.magnifier = true;
        } else if !profile.captions_enabled {
            profile.captions_enabled = true;
        } else if !profile.reduced_motion {
            profile.reduced_motion = true;
        } else {
            profile = AccessibilityProfile::default();
        }
        let _ = self.client.set_accessibility_profile(profile);
    }

    pub(super) fn cycle_restore_disposition(&self) {
        let current = self
            .client
            .restore_disposition()
            .unwrap_or(RestoreDisposition::RestoreIfClean);
        let _ = self
            .client
            .set_restore_disposition(next_restore_disposition(current));
    }

    pub(super) fn toggle(&mut self, index: usize) {
        match index {
            0 => self.focus_mode = !self.focus_mode,
            1 => self.cycle_motion_profile(),
            2 => self.cycle_shell_density(),
            3 => self.cycle_display_scale(),
            4 => self.toggle_accessibility_core(),
            5 => self.cycle_restore_disposition(),
            _ => {}
        }
        self.dirty = true;
    }

    pub(super) fn render(&mut self) -> Result<(), String> {
        let Some(window) = self.window else {
            return Ok(());
        };
        if !window.visible || !self.dirty {
            return Ok(());
        }

        let mut canvas = Canvas::new(
            window.content_rect.width as usize,
            window.content_rect.height as usize,
            WINDOW_BG,
        );
        canvas.fill_rect(Rect::new(0, 0, window.content_rect.width, 60), PANEL_BG);
        canvas.fill_rect(Rect::new(0, 60, window.content_rect.width, 1), BORDER);
        canvas.draw_text(18, 18, "Settings", TEXT_PRIMARY);
        let session = self.client.session_snapshot().unwrap_or(SessionSnapshot {
            workspace_id: self.workspace_id,
            workspace_layout: WorkspaceLayout::Dwindle,
            power_state: SessionPowerState::Active,
            unread_notifications: 0,
            apps_running: 0,
            apps_crashed: 0,
            overview_active: false,
            scratchpad_visible: false,
            shell_ready: true,
            boot_clean_desktop: true,
            output_scale: 1,
            text_scale: 1,
            clipboard_history_len: 0,
            accessibility_profile: crate::gui::protocol::AccessibilityProfile::default(),
            display_profile: crate::gui::protocol::DisplayProfile::default(),
            shell_density: crate::gui::protocol::ShellDensityProfile::Balanced,
            motion_profile: crate::gui::protocol::MotionProfile::Standard,
            restore_state: crate::gui::protocol::RestoreDisposition::RestoreIfClean,
            stage_set_policy: crate::gui::protocol::StageSetPolicy::default(),
            locale: String::from("en-US"),
            theme_variant: String::from("hybrid-titan"),
            shell_state: ShellState::DesktopReady,
        });
        let motion_profile = self
            .client
            .motion_profile()
            .unwrap_or(session.motion_profile);
        let shell_density = self.client.shell_density().unwrap_or(session.shell_density);
        let restore_state = self
            .client
            .restore_disposition()
            .unwrap_or(session.restore_state);
        let display_profile = self
            .client
            .display_profile()
            .unwrap_or_else(|_| session.display_profile.clone());
        let accessibility_profile = self
            .client
            .accessibility_profile()
            .unwrap_or(session.accessibility_profile);
        self.animations = motion_profile != MotionProfile::Reduced;
        canvas.draw_text(
            18,
            38,
            &format!(
                "{}  {} running  {} faults  {} clips",
                power_state_label(session.power_state),
                session.apps_running,
                session.apps_crashed,
                session.clipboard_history_len
            ),
            TEXT_MUTED,
        );

        let rows = [
            (
                "Focus mode",
                if self.focus_mode {
                    "Enabled".to_string()
                } else {
                    "Disabled".to_string()
                },
                self.focus_mode,
                ACCENT_GOLD,
            ),
            (
                "Motion profile",
                motion_profile_label(motion_profile).to_string(),
                motion_profile != MotionProfile::Reduced,
                ACCENT_BLUE,
            ),
            (
                "Shell density",
                shell_density_label(shell_density).to_string(),
                shell_density != ShellDensityProfile::Balanced,
                ACCENT_SOFT,
            ),
            (
                "Display scale",
                display_scale_label(&display_profile),
                true,
                ACCENT_MINT,
            ),
            (
                "Accessibility core",
                accessibility_profile_label(accessibility_profile),
                accessibility_profile != AccessibilityProfile::default(),
                ACCENT_BLUE,
            ),
            (
                "Restore policy",
                restore_disposition_label(restore_state).to_string(),
                restore_state != RestoreDisposition::RestoreIfClean,
                ACCENT_CORAL,
            ),
        ];

        let mut y = 88;
        for (index, (label, value, enabled, accent)) in rows.iter().enumerate() {
            let rect = Rect::new(20, y - 8, window.content_rect.width.saturating_sub(40), 54);
            canvas.fill_rect(rect, PANEL_BG);
            canvas.stroke_rect(rect, if *enabled { *accent } else { BORDER });
            canvas.draw_text(34, y + 4, label, TEXT_PRIMARY);
            canvas.draw_text(
                34,
                y + 24,
                value,
                if *enabled { *accent } else { TEXT_SECONDARY },
            );
            canvas.draw_text(
                rect.right() - 54,
                y + 14,
                &(index + 1).to_string(),
                TEXT_MUTED,
            );
            y += 64;
        }

        self.client
            .present(window.window_id, &canvas.into_pixels())?;
        self.dirty = false;
        Ok(())
    }

    pub(super) fn snapshot(&self) -> AppSnapshot {
        let enabled = [
            self.focus_mode,
            self.animations,
            self.notifications,
            self.client
                .accessibility_profile()
                .map(|profile| profile != AccessibilityProfile::default())
                .unwrap_or(false),
        ]
        .iter()
        .filter(|flag| **flag)
        .count();
        snapshot_for_window(
            AppKind::Settings,
            self.window,
            self.workspace_id,
            format!("{} live policy lanes active", enabled),
        )
    }

    pub(super) fn publish_accessibility(&self) {
        let Some(window) = self.window else {
            return;
        };
        let nodes = vec![
            AccessibilityNode {
                id: 1,
                app_id: self.client.app_id(),
                role: AccessibilityRole::Window,
                label: String::from("Settings"),
                description: String::from("desktop policy settings"),
                focused: window.focused,
                bounds: window.content_rect,
            },
            AccessibilityNode {
                id: 2,
                app_id: self.client.app_id(),
                role: AccessibilityRole::Button,
                label: String::from("Focus mode"),
                description: self.focus_mode.to_string(),
                focused: false,
                bounds: Rect::new(20, 80, 420, 54),
            },
            AccessibilityNode {
                id: 3,
                app_id: self.client.app_id(),
                role: AccessibilityRole::List,
                label: String::from("Policy controls"),
                description: String::from("motion, density, scale, accessibility, restore"),
                focused: false,
                bounds: Rect::new(20, 144, window.content_rect.width.saturating_sub(40), 320),
            },
        ];
        let _ = self.client.publish_accessibility_tree(nodes);
    }
}

impl EditorApp {
    pub(super) fn ensure_window(&mut self, screen: Rect) -> Result<LaunchResult, String> {
        let result = ensure_window_visible(
            &self.client,
            &mut self.window,
            "Editor",
            Rect::new(screen.x + 162, screen.y + 148, 620, 400),
            self.workspace_id,
        )?;
        self.dirty = true;
        Ok(result)
    }

    pub(super) fn sync(&mut self, active_workspace: u8) -> bool {
        match sync_window_state(
            &self.client,
            &mut self.window,
            self.workspace_id,
            active_workspace,
        ) {
            WindowSync::Changed => {
                self.dirty = true;
                true
            }
            WindowSync::Closed => {
                let _ = self.client.mark_app_exited(true, "window closed");
                self.dirty = true;
                true
            }
            WindowSync::Unchanged => false,
        }
    }

    pub(super) fn handle_events(&mut self, events: Vec<WindowInputEvent>) {
        let Some(window_id) = self.window.as_ref().map(|window| window.window_id) else {
            return;
        };

        for event in events {
            if event.window_id != window_id {
                continue;
            }
            if ctrl_scan_pressed(&event.event, 0x1F) {
                let _ = self.save_document();
                continue;
            }
            if ctrl_scan_pressed(&event.event, 0x18) {
                if let Ok(dialog_id) = self
                    .client
                    .open_file_dialog("Open File", self.path.as_deref().unwrap_or("/"))
                {
                    self.pending_dialogs.push(EditorDialog {
                        id: dialog_id,
                        kind: EditorDialogKind::Open,
                    });
                }
                continue;
            }

            if is_backspace_key(&event.event) {
                self.text.pop();
                self.document_dirty = true;
                self.dirty = true;
            } else if is_enter_key(&event.event) {
                self.text.push('\n');
                self.document_dirty = true;
                self.dirty = true;
            } else if let Some(ch) = printable_key(&event.event) {
                self.text.push(ch);
                self.document_dirty = true;
                self.dirty = true;
            }
        }
    }

    pub(super) fn open_document(&mut self, path: &str) -> Result<(), String> {
        let data = self.client.read_file(path)?;
        let text = String::from_utf8_lossy(&data).to_string();
        self.text = text;
        self.path = Some(String::from(path));
        self.status = format!("Opened {}", path);
        self.document_dirty = false;
        self.dirty = true;
        Ok(())
    }

    pub(super) fn save_document(&mut self) -> Result<(), String> {
        if let Some(path) = self.path.clone() {
            self.client.write_file(&path, self.text.as_bytes())?;
            self.status = format!("Saved {}", path);
            self.document_dirty = false;
            self.dirty = true;
            return Ok(());
        }

        let dialog_id = self.client.save_file_dialog("Save File", "/notes.txt")?;
        self.pending_dialogs.push(EditorDialog {
            id: dialog_id,
            kind: EditorDialogKind::Save,
        });
        self.status = String::from("Waiting for save path");
        self.dirty = true;
        Ok(())
    }

    pub(super) fn poll_platform(&mut self) {
        let mut completed = Vec::new();
        let pending = self.pending_dialogs.clone();
        for dialog in pending.iter() {
            if let Ok(Some(result)) = self.client.poll_dialog_result(dialog.id) {
                match result.selection {
                    DialogSelection::Accepted(path) => match dialog.kind {
                        EditorDialogKind::Open => {
                            let _ = self.open_document(&path);
                        }
                        EditorDialogKind::Save => {
                            self.path = Some(path.clone());
                            let _ = self.save_document();
                        }
                    },
                    DialogSelection::Cancelled => {
                        self.status = String::from("Dialog cancelled");
                    }
                }
                completed.push(dialog.id);
                self.dirty = true;
            }
        }
        self.pending_dialogs
            .retain(|dialog| !completed.iter().any(|done| done == &dialog.id));
    }

    pub(super) fn render(&mut self) -> Result<(), String> {
        let Some(window) = self.window else {
            return Ok(());
        };
        if !window.visible || !self.dirty {
            return Ok(());
        }

        let mut canvas = Canvas::new(
            window.content_rect.width as usize,
            window.content_rect.height as usize,
            WINDOW_BG,
        );
        canvas.fill_rect(Rect::new(0, 0, window.content_rect.width, 54), PANEL_BG);
        canvas.fill_rect(Rect::new(0, 54, window.content_rect.width, 1), BORDER);
        canvas.draw_text(18, 18, "Text Editor", TEXT_PRIMARY);
        let title = self.path.as_deref().unwrap_or("Scratch buffer");
        canvas.draw_text(18, 34, title, TEXT_MUTED);
        canvas.draw_multiline_text(
            20,
            72,
            window.content_rect.width as i32 - 40,
            &self.text,
            TEXT_PRIMARY,
        );
        let footer_y = max(window.content_rect.height as i32 - 30, 0);
        canvas.fill_rect(
            Rect::new(0, footer_y, window.content_rect.width, 30),
            PANEL_BG,
        );
        canvas.fill_rect(Rect::new(0, footer_y, window.content_rect.width, 1), BORDER);
        let status = if self.document_dirty {
            format!("{} (modified)", self.status)
        } else {
            self.status.clone()
        };
        canvas.draw_text(18, footer_y + 8, &status, TEXT_SECONDARY);

        self.client
            .present(window.window_id, &canvas.into_pixels())?;
        self.dirty = false;
        Ok(())
    }

    pub(super) fn snapshot(&self) -> AppSnapshot {
        snapshot_for_window(
            AppKind::Editor,
            self.window,
            self.workspace_id,
            if let Some(path) = self.path.as_ref() {
                format!("{}{}", path, if self.document_dirty { " *" } else { "" })
            } else {
                format!("scratch {} chars", self.text.chars().count())
            },
        )
    }

    pub(super) fn publish_accessibility(&self) {
        let Some(window) = self.window else {
            return;
        };
        let nodes = vec![
            AccessibilityNode {
                id: 1,
                app_id: self.client.app_id(),
                role: AccessibilityRole::Window,
                label: String::from("Editor"),
                description: self.status.clone(),
                focused: window.focused,
                bounds: window.content_rect,
            },
            AccessibilityNode {
                id: 2,
                app_id: self.client.app_id(),
                role: AccessibilityRole::Input,
                label: String::from("Document"),
                description: self.path.clone().unwrap_or_else(|| String::from("scratch")),
                focused: window.focused,
                bounds: Rect::new(
                    0,
                    60,
                    window.content_rect.width,
                    window.content_rect.height.saturating_sub(60),
                ),
            },
        ];
        let _ = self.client.publish_accessibility_tree(nodes);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WindowSync {
    Unchanged,
    Changed,
    Closed,
}
