//! Agent Terminal — runs agent commands in real IDE terminals (PTY).
//!
//! When the AI agent calls `execute_command` or `execute_background`, this
//! module creates a real PTY-based terminal that:
//!
//! 1. **Appears in the IDE terminal panel** — user can see the output live
//! 2. **Uses a login shell** — sources `.zshrc`/`.bashrc`, so `nvm`/PATH work
//! 3. **Captures output** for the agent to read as a tool result
//! 4. **Has proper signal handling** — no orphan processes
//!
//! This replaces the old approach of `Command::new("sh").stdout(Stdio::piped())`
//! which was invisible to the user and didn't source shell profiles.

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::io::{self, ErrorKind, Read, Write};
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use alacritty_terminal::{
    event::{OnResize, WindowSize},
    event_loop::Msg,
    tty::{self, EventedPty, EventedReadWrite, Options, Shell, setup_env},
};
use anyhow::Result;
use crossbeam_channel::Receiver;
use lapce_rpc::{
    core::{CoreNotification, CoreRpcHandler},
    terminal::{TermId, TerminalProfile},
};
use polling::PollMode;

use crate::terminal::TerminalSender;

const READ_BUFFER_SIZE: usize = 0x10_0000;
/// Max bytes to capture for agent output.
const MAX_CAPTURE_BYTES: usize = 200_000;

#[cfg(any(target_os = "linux", target_os = "macos"))]
const PTY_READ_WRITE_TOKEN: usize = 0;
#[cfg(any(target_os = "linux", target_os = "macos"))]
const PTY_CHILD_EVENT_TOKEN: usize = 1;

#[cfg(target_os = "windows")]
const PTY_READ_WRITE_TOKEN: usize = 2;
#[cfg(target_os = "windows")]
const PTY_CHILD_EVENT_TOKEN: usize = 1;

// ─── Internal write state (mirrors terminal.rs State/Writing) ────────────────

struct Writing {
    source: Cow<'static, [u8]>,
    written: usize,
}

impl Writing {
    fn new(c: Cow<'static, [u8]>) -> Self {
        Self { source: c, written: 0 }
    }
    fn advance(&mut self, n: usize) {
        self.written += n;
    }
    fn remaining_bytes(&self) -> &[u8] {
        &self.source[self.written..]
    }
    fn finished(&self) -> bool {
        self.written >= self.source.len()
    }
}

#[derive(Default)]
struct WriteState {
    write_list: VecDeque<Cow<'static, [u8]>>,
    writing: Option<Writing>,
}

impl WriteState {
    fn push_input(&mut self, data: Cow<'static, [u8]>) {
        self.write_list.push_back(data);
    }
    fn ensure_next(&mut self) {
        if self.writing.is_none() {
            self.goto_next();
        }
    }
    fn goto_next(&mut self) {
        self.writing = self.write_list.pop_front().map(Writing::new);
    }
    fn take_current(&mut self) -> Option<Writing> {
        self.writing.take()
    }
    fn needs_write(&self) -> bool {
        self.writing.is_some() || !self.write_list.is_empty()
    }
    fn set_current(&mut self, new: Option<Writing>) {
        self.writing = new;
    }
}

// ─── Agent Terminal Manager ──────────────────────────────────────────────────

/// Manages all terminals created by the AI agent.
/// Shared across tool calls via Arc.
pub struct AgentTerminalManager {
    /// Active agent terminals, keyed by PID (so existing tools can look up by PID).
    terminals: Mutex<HashMap<u32, AgentTermHandle>>,
}

/// Handle to a running agent terminal.
struct AgentTermHandle {
    pub term_id: TermId,
    pub pid: u32,
    #[allow(dead_code)]
    pub command: String,
    #[allow(dead_code)]
    pub started_at: std::time::Instant,
    /// Captured output bytes (raw terminal data, stripped of ANSI for agent).
    pub capture: Arc<Mutex<Vec<u8>>>,
    /// Set to Some when the process exits.
    pub exit_code: Arc<Mutex<Option<Option<i32>>>>,
    /// Condvar signaled when the process exits.
    exit_notify: Arc<Condvar>,
    exit_flag: Arc<Mutex<bool>>,
    /// Sender to write input to the terminal (for future use).
    #[allow(dead_code)]
    pub sender: TerminalSender,
}

impl AgentTerminalManager {
    pub fn new() -> Self {
        Self {
            terminals: Mutex::new(HashMap::new()),
        }
    }

    /// Execute a command in a real IDE terminal (foreground, waits for completion).
    ///
    /// The terminal appears in the IDE's terminal panel so the user can see it.
    /// Output is captured and returned as the tool result.
    pub fn execute_command(
        &self,
        command: &str,
        workdir: &Path,
        timeout_secs: u64,
        core_rpc: &CoreRpcHandler,
        ide_terminals: &Arc<std::sync::Mutex<HashMap<TermId, TerminalSender>>>,
        tool_call_id: &str,
        tool_name: &str,
    ) -> forge_agent::tools::ToolResult {
        let (handle, sender) = match self.spawn_terminal(command, workdir, core_rpc) {
            Ok(h) => h,
            Err(e) => return forge_agent::tools::ToolResult::err(format!("Failed to create terminal: {e}")),
        };

        let pid = handle.pid;
        let term_id = handle.term_id;

        // Register in IDE terminals (brief lock, then release)
        ide_terminals.lock().unwrap().insert(term_id, sender);

        // Store the handle for waiting
        let capture = handle.capture.clone();
        let exit_code_ref = handle.exit_code.clone();
        let exit_notify = handle.exit_notify.clone();
        let exit_flag = handle.exit_flag.clone();
        self.terminals.lock().unwrap().insert(pid, handle);

        // Wait for the process to complete (with timeout) — lock is NOT held.
        // Sends periodic AgentToolCallUpdate notifications so the chat panel
        // streams output live instead of showing it all at the end.
        let timeout = Duration::from_secs(timeout_secs);
        let (output, exit_code) = Self::wait_with_refs(
            &capture, &exit_code_ref, &exit_notify, &exit_flag, timeout,
            core_rpc, tool_call_id, tool_name,
        );

        // Strip ANSI escape sequences for clean agent output
        let clean_output = strip_ansi_escapes(&output);
        let truncated = clean_output.len() > 30_000;
        let display_output = if truncated {
            format!(
                "{}...\n(output truncated at 30000 chars)",
                &clean_output[..30_000]
            )
        } else {
            clean_output
        };

        match exit_code {
            Some(0) => forge_agent::tools::ToolResult::ok(
                format!("Exit code: 0\n{display_output}")
            ),
            Some(code) => forge_agent::tools::ToolResult::err(
                format!("Exit code: {code}\n{display_output}")
            ),
            None => forge_agent::tools::ToolResult::err(
                format!("Command timed out after {timeout_secs}s. The terminal is still running.\n{display_output}")
            ),
        }
    }

    /// Execute a command in a real IDE terminal (background, returns immediately).
    ///
    /// The terminal appears in the IDE's terminal panel.
    /// Returns the PID and initial output after waiting briefly.
    pub fn execute_background(
        &self,
        command: &str,
        workdir: &Path,
        wait_seconds: u64,
        core_rpc: &CoreRpcHandler,
        ide_terminals: &Arc<std::sync::Mutex<HashMap<TermId, TerminalSender>>>,
    ) -> forge_agent::tools::ToolResult {
        let (handle, sender) = match self.spawn_terminal(command, workdir, core_rpc) {
            Ok(h) => h,
            Err(e) => return forge_agent::tools::ToolResult::err(format!("Failed to create terminal: {e}")),
        };

        let pid = handle.pid;
        let term_id = handle.term_id;

        // Register in IDE terminals (brief lock, then release)
        ide_terminals.lock().unwrap().insert(term_id, sender);

        // Store in our registry for later read_process_output calls
        self.terminals.lock().unwrap().insert(pid, handle);

        // Wait briefly for initial output — no locks held
        std::thread::sleep(Duration::from_secs(wait_seconds));

        // Check if still running
        let is_running = self.is_running(pid);
        let initial_output = self.get_output(pid);
        let clean_output = strip_ansi_escapes(&initial_output);
        let len = clean_output.len().min(20_000);

        forge_agent::tools::ToolResult::ok(format!(
            "Process started in background (visible in terminal panel).\n\
             PID: {pid}\n\
             Terminal: {term_id:?}\n\
             Running: {is_running}\n\
             --- Initial output ({wait_seconds}s) ---\n\
             {}", &clean_output[..len]
        ))
    }

    /// Read output from an agent terminal by PID.
    pub fn read_output(&self, pid: u32, tail_lines: usize) -> Option<String> {
        let terminals = self.terminals.lock().unwrap();
        let handle = terminals.get(&pid)?;
        let raw = handle.capture.lock().unwrap();
        let text = strip_ansi_escapes(&String::from_utf8_lossy(&raw));

        if tail_lines == 0 || text.is_empty() {
            return Some(text);
        }

        let lines: Vec<&str> = text.lines().collect();
        if lines.len() <= tail_lines {
            Some(text)
        } else {
            Some(lines[lines.len() - tail_lines..].join("\n"))
        }
    }

    /// Check if an agent terminal process is still running.
    pub fn is_running(&self, pid: u32) -> bool {
        let terminals = self.terminals.lock().unwrap();
        if let Some(handle) = terminals.get(&pid) {
            handle.exit_code.lock().unwrap().is_none()
        } else {
            false
        }
    }

    /// Check if this PID belongs to an agent terminal.
    pub fn has_terminal(&self, pid: u32) -> bool {
        self.terminals.lock().unwrap().contains_key(&pid)
    }

    /// Get captured output for a PID.
    fn get_output(&self, pid: u32) -> String {
        let terminals = self.terminals.lock().unwrap();
        if let Some(handle) = terminals.get(&pid) {
            String::from_utf8_lossy(&handle.capture.lock().unwrap()).to_string()
        } else {
            String::new()
        }
    }

    /// Wait for a terminal to exit using pre-extracted Arc references.
    /// Waits in 500ms chunks and sends AgentToolCallUpdate notifications each
    /// time new output arrives so the chat panel streams live output.
    fn wait_with_refs(
        capture: &Arc<Mutex<Vec<u8>>>,
        exit_code: &Arc<Mutex<Option<Option<i32>>>>,
        exit_notify: &Arc<Condvar>,
        exit_flag: &Arc<Mutex<bool>>,
        timeout: Duration,
        core_rpc: &CoreRpcHandler,
        tool_call_id: &str,
        tool_name: &str,
    ) -> (String, Option<i32>) {
        const CHUNK: Duration = Duration::from_millis(500);
        let start = std::time::Instant::now();
        let mut last_output_len = 0;

        loop {
            let elapsed = start.elapsed();
            if elapsed >= timeout {
                break;
            }

            // Wait for exit signal or up to CHUNK duration
            let remaining = timeout - elapsed;
            let wait_for = CHUNK.min(remaining);
            let done = {
                let flag = exit_flag.lock().unwrap();
                if *flag {
                    true
                } else {
                    let (guard, _) = exit_notify.wait_timeout(flag, wait_for).unwrap();
                    *guard
                }
            };

            // Send partial output to the chat panel if anything new arrived
            let current_raw = capture.lock().unwrap().clone();
            let clean = strip_ansi_escapes(&String::from_utf8_lossy(&current_raw));
            if clean.len() != last_output_len {
                last_output_len = clean.len();
                let display = if clean.len() > 30_000 {
                    format!("{}...(truncated)", &clean[..30_000])
                } else {
                    clean
                };
                core_rpc.notification(CoreNotification::AgentToolCallUpdate {
                    tool_call_id: tool_call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    arguments: String::new(),
                    status: "running".to_string(),
                    output: Some(display),
                });
            }

            if done {
                break;
            }
        }

        let output = String::from_utf8_lossy(&capture.lock().unwrap()).to_string();
        let done = *exit_flag.lock().unwrap();
        let code = exit_code.lock().unwrap().flatten();
        (output, if done { code.or(Some(0)) } else { None })
    }

    /// Spawn a new PTY terminal for an agent command.
    fn spawn_terminal(
        &self,
        command: &str,
        workdir: &Path,
        core_rpc: &CoreRpcHandler,
    ) -> Result<(AgentTermHandle, TerminalSender)> {
        let term_id = TermId::next();

        // Use user's login shell
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

        let profile = TerminalProfile {
            name: format!("Agent: {}", &command[..command.len().min(60)]),
            command: Some(shell.clone()),
            arguments: Some(vec!["-l".into(), "-c".into(), command.into()]),
            workdir: Some(
                url::Url::from_file_path(workdir)
                    .map_err(|_| anyhow::anyhow!("Invalid workdir path"))?,
            ),
            environment: None,
        };

        let mut env = profile.environment.clone().unwrap_or_default();
        
        // Inject proto paths into PATH so agent commands can find installed SDKs
        if let Some(home) = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf()) {
            let proto_shims = home.join(".proto").join("shims");
            let proto_bin = home.join(".proto").join("bin");
            let current_path = std::env::var("PATH").unwrap_or_default();
            
            let mut new_path = String::new();
            if proto_shims.exists() {
                new_path.push_str(&proto_shims.to_string_lossy());
                new_path.push(':');
            }
            if proto_bin.exists() {
                new_path.push_str(&proto_bin.to_string_lossy());
                new_path.push(':');
            }
            new_path.push_str(&current_path);
            
            env.insert("PATH".to_string(), new_path);
        }

        let options = Options {
            shell: Some(Shell::new(
                shell,
                vec!["-l".into(), "-c".into(), command.into()],
            )),
            working_directory: Some(workdir.to_path_buf()),
            hold: false,
            env,
        };

        setup_env();

        #[cfg(target_os = "macos")]
        {
            let locale = locale_config::Locale::global_default()
                .to_string()
                .replace('-', "_");
            unsafe {
                std::env::set_var("LC_ALL", format!("{locale}.UTF-8"));
            }
        }

        let size = WindowSize {
            num_lines: 40,
            num_cols: 120,
            cell_width: 1,
            cell_height: 1,
        };

        let pty = alacritty_terminal::tty::new(&options, size, 0)?;

        #[cfg(not(target_os = "windows"))]
        let child_pid = pty.child().id();
        #[cfg(target_os = "windows")]
        let child_pid = pty.child_watcher().pid().map(|x| x.get()).unwrap_or(0);

        let poller: Arc<polling::Poller> = polling::Poller::new()?.into();
        let (tx, rx) = crossbeam_channel::unbounded();

        // Notify frontend about the new terminal
        core_rpc.terminal_process_id(term_id, Some(child_pid));

        let capture = Arc::new(Mutex::new(Vec::new()));
        let exit_code = Arc::new(Mutex::new(None::<Option<i32>>));
        let exit_notify = Arc::new(Condvar::new());
        let exit_flag = Arc::new(Mutex::new(false));

        // Create two senders: one for the handle (agent use), one to return (IDE use)
        let ide_sender = TerminalSender::new(tx.clone(), poller.clone());
        let handle_sender = TerminalSender::new(tx, poller.clone());

        let handle = AgentTermHandle {
            term_id,
            pid: child_pid,
            command: command.to_string(),
            started_at: std::time::Instant::now(),
            capture: capture.clone(),
            exit_code: exit_code.clone(),
            exit_notify: exit_notify.clone(),
            exit_flag: exit_flag.clone(),
            sender: handle_sender,
        };

        // Run the PTY event loop in a background thread
        let rpc = core_rpc.clone();
        let cap = capture;
        let ec = exit_code;
        let en = exit_notify;
        let ef = exit_flag;

        std::thread::spawn(move || {
            run_capturing_event_loop(term_id, pty, poller, rx, rpc, cap, ec, en, ef);
        });

        Ok((handle, ide_sender))
    }
}

// ─── PTY event loop with output capture ──────────────────────────────────────

fn run_capturing_event_loop(
    term_id: TermId,
    mut pty: alacritty_terminal::tty::Pty,
    poller: Arc<polling::Poller>,
    rx: Receiver<Msg>,
    core_rpc: CoreRpcHandler,
    capture: Arc<Mutex<Vec<u8>>>,
    exit_code_holder: Arc<Mutex<Option<Option<i32>>>>,
    exit_notify: Arc<Condvar>,
    exit_flag: Arc<Mutex<bool>>,
) {
    let mut state = WriteState::default();
    let mut buf = [0u8; READ_BUFFER_SIZE];

    let poll_opts = PollMode::Level;
    let mut interest = polling::Event::readable(0);

    unsafe {
        if let Err(e) = pty.register(&poller, interest, poll_opts) {
            tracing::error!("Agent terminal: failed to register PTY: {e}");
            return;
        }
    }

    let mut events = polling::Events::with_capacity(NonZeroUsize::new(1024).unwrap());
    let timeout = Some(Duration::from_secs(6));
    let mut final_exit_code: Option<i32> = None;

    'event_loop: loop {
        events.clear();
        if let Err(err) = poller.wait(&mut events, timeout) {
            match err.kind() {
                ErrorKind::Interrupted => continue,
                _ => {
                    tracing::error!("Agent terminal polling error: {err:?}");
                    break;
                }
            }
        }

        // Drain channel messages
        while let Ok(msg) = rx.try_recv() {
            match msg {
                Msg::Input(input) => state.push_input(input),
                Msg::Shutdown => break 'event_loop,
                Msg::Resize(size) => pty.on_resize(size),
            }
        }

        for event in events.iter() {
            match event.key {
                PTY_CHILD_EVENT_TOKEN => {
                    if let Some(tty::ChildEvent::Exited(exited_code)) = pty.next_child_event() {
                        // Read any remaining output
                        let _ = pty_read_capturing(&mut pty, &core_rpc, &capture, term_id, &mut buf);
                        final_exit_code = exited_code;
                        break 'event_loop;
                    }
                }
                PTY_READ_WRITE_TOKEN => {
                    if event.is_interrupt() {
                        continue;
                    }
                    if event.readable {
                        if let Err(err) = pty_read_capturing(&mut pty, &core_rpc, &capture, term_id, &mut buf) {
                            #[cfg(target_os = "linux")]
                            if err.raw_os_error() == Some(libc::EIO) {
                                continue;
                            }
                            tracing::error!("Agent terminal read error: {err}");
                            break 'event_loop;
                        }
                    }
                    if event.writable {
                        if pty_write(&mut pty, &mut state).is_err() {
                            break 'event_loop;
                        }
                    }
                }
                _ => {}
            }
        }

        // Update write interest
        let needs_write = state.needs_write();
        if needs_write != interest.writable {
            interest.writable = needs_write;
            pty.reregister(&poller, interest, poll_opts).unwrap();
        }
    }

    // Signal completion
    core_rpc.terminal_process_stopped(term_id, final_exit_code);

    *exit_code_holder.lock().unwrap() = Some(final_exit_code);
    let mut done = exit_flag.lock().unwrap();
    *done = true;
    exit_notify.notify_all();

    let _ = pty.deregister(&poller);
}

/// Read from PTY: send to frontend (visible in terminal) AND capture for agent.
fn pty_read_capturing(
    pty: &mut alacritty_terminal::tty::Pty,
    core_rpc: &CoreRpcHandler,
    capture: &Arc<Mutex<Vec<u8>>>,
    term_id: TermId,
    buf: &mut [u8],
) -> io::Result<()> {
    loop {
        match pty.reader().read(buf) {
            Ok(0) => break,
            Ok(n) => {
                let data = buf[..n].to_vec();
                // Send to frontend (appears in terminal panel)
                core_rpc.update_terminal(term_id, data.clone());
                // Capture for agent
                let mut cap = capture.lock().unwrap();
                if cap.len() < MAX_CAPTURE_BYTES {
                    let remaining = MAX_CAPTURE_BYTES - cap.len();
                    cap.extend_from_slice(&data[..data.len().min(remaining)]);
                }
            }
            Err(err) => match err.kind() {
                ErrorKind::Interrupted | ErrorKind::WouldBlock => break,
                _ => return Err(err),
            },
        }
    }
    Ok(())
}

/// Write pending data to the PTY.
fn pty_write(
    pty: &mut alacritty_terminal::tty::Pty,
    state: &mut WriteState,
) -> io::Result<()> {
    state.ensure_next();

    'write_many: while let Some(mut current) = state.take_current() {
        'write_one: loop {
            match pty.writer().write(current.remaining_bytes()) {
                Ok(0) => {
                    state.set_current(Some(current));
                    break 'write_many;
                }
                Ok(n) => {
                    current.advance(n);
                    if current.finished() {
                        state.goto_next();
                        break 'write_one;
                    }
                }
                Err(err) => {
                    state.set_current(Some(current));
                    match err.kind() {
                        ErrorKind::Interrupted | ErrorKind::WouldBlock => {
                            break 'write_many;
                        }
                        _ => return Err(err),
                    }
                }
            }
        }
    }

    Ok(())
}

// ─── ANSI escape stripping ──────────────────────────────────────────────────

/// Strip ANSI escape sequences from terminal output so the agent gets clean text.
fn strip_ansi_escapes(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // ESC sequence
            if let Some(&next) = chars.peek() {
                if next == '[' {
                    chars.next(); // consume '['
                    // CSI sequence: skip until we hit a letter (@ through ~)
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c.is_ascii_alphabetic() || c == '~' || c == '@' {
                            break;
                        }
                    }
                } else if next == ']' {
                    chars.next(); // consume ']'
                    // OSC sequence: skip until BEL (\x07) or ST (ESC \)
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' {
                            if let Some(&'\\') = chars.peek() {
                                chars.next();
                            }
                            break;
                        }
                    }
                } else if next == '(' || next == ')' {
                    chars.next(); // consume '(' or ')'
                    chars.next(); // consume charset designator
                } else {
                    chars.next(); // consume single char after ESC
                }
            }
        } else if ch == '\r' {
            // Skip carriage returns (terminal artifacts)
            continue;
        } else {
            result.push(ch);
        }
    }

    result
}
