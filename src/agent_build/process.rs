use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
};

pub(super) const MAX_LOG_TAIL: usize = 4000;
const READ_BUFFER_SIZE: usize = 4096;

#[derive(Default)]
pub(super) struct LogTail {
    text: String,
    truncated: bool,
}

impl LogTail {
    pub(super) fn push(&mut self, message: &str) {
        self.text.push_str(message);
        if self.text.len() <= MAX_LOG_TAIL {
            return;
        }
        self.truncated = true;
        let mut start = self.text.len() - MAX_LOG_TAIL;
        while !self.text.is_char_boundary(start) {
            start += 1;
        }
        self.text.drain(..start);
    }

    pub(super) fn finish(self) -> (String, bool) {
        (self.text, self.truncated)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AnsiState {
    Ground,
    Escape,
    Csi,
    Osc,
    OscEscape,
}

#[derive(Default)]
struct AnsiFilter {
    state: Option<AnsiState>,
}

impl AnsiFilter {
    fn push(&mut self, input: &[u8], output: &mut Vec<u8>) {
        for &byte in input {
            self.filter_byte(byte, output);
        }
    }

    fn filter_byte(&mut self, byte: u8, output: &mut Vec<u8>) {
        let state = self.state.take().unwrap_or(AnsiState::Ground);
        let next = match state {
            AnsiState::Ground => return self.ground_byte(byte, output),
            AnsiState::Escape => match byte {
                b'[' => AnsiState::Csi,
                b']' => AnsiState::Osc,
                _ => AnsiState::Ground,
            },
            AnsiState::Csi => match byte {
                0x40..=0x7e => AnsiState::Ground,
                _ => AnsiState::Csi,
            },
            AnsiState::Osc => match byte {
                0x07 => AnsiState::Ground,
                0x1b => AnsiState::OscEscape,
                _ => AnsiState::Osc,
            },
            AnsiState::OscEscape => match byte {
                b'\\' => AnsiState::Ground,
                _ => AnsiState::Osc,
            },
        };
        self.state = (next != AnsiState::Ground).then_some(next);
    }

    fn ground_byte(&mut self, byte: u8, output: &mut Vec<u8>) {
        match byte {
            0x1b => self.state = Some(AnsiState::Escape),
            b'\n' | b'\t' => output.push(byte),
            b'\r' => output.push(b'\n'),
            0x00..=0x1f | 0x7f => {}
            _ => output.push(byte),
        }
    }
}

#[derive(Default)]
struct Utf8Filter {
    pending: Vec<u8>,
}

impl Utf8Filter {
    fn push(&mut self, input: &[u8]) -> String {
        self.pending.extend_from_slice(input);
        let mut output = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(valid) => {
                    output.push_str(valid);
                    self.pending.clear();
                    return output;
                }
                Err(error) => self.push_valid_prefix(&mut output, error),
            }
            if self.pending.is_empty() || self.has_incomplete_suffix() {
                return output;
            }
        }
    }

    fn push_valid_prefix(&mut self, output: &mut String, error: std::str::Utf8Error) {
        let valid = error.valid_up_to();
        let prefix = std::str::from_utf8(&self.pending[..valid]).unwrap_or_default();
        output.push_str(prefix);
        match error.error_len() {
            Some(length) => {
                output.push('\u{fffd}');
                self.pending.drain(..valid + length);
            }
            None => {
                self.pending.drain(..valid);
            }
        }
    }

    fn has_incomplete_suffix(&self) -> bool {
        std::str::from_utf8(&self.pending)
            .err()
            .is_some_and(|error| error.error_len().is_none())
    }

    fn finish(&mut self) -> String {
        String::from_utf8_lossy(&std::mem::take(&mut self.pending)).into_owned()
    }
}

struct Redactor {
    token: Option<String>,
    pending: String,
}

impl Redactor {
    fn new(token: Option<&str>) -> Self {
        Self {
            token: token.filter(|value| !value.is_empty()).map(str::to_owned),
            pending: String::new(),
        }
    }

    fn push(&mut self, fragment: &str) -> String {
        self.pending.push_str(fragment);
        let cutoff = self.safe_cutoff();
        let remaining = self.pending.split_off(cutoff);
        let ready = std::mem::replace(&mut self.pending, remaining);
        redact_complete(&ready, self.token.as_deref())
    }

    fn finish(&mut self) -> String {
        redact_complete(&std::mem::take(&mut self.pending), self.token.as_deref())
    }

    fn safe_cutoff(&self) -> usize {
        let token_start = self
            .token
            .as_deref()
            .and_then(|token| partial_token_start(&self.pending, token));
        let url_start = unfinished_url_start(&self.pending);
        match (token_start, url_start) {
            (Some(left), Some(right)) => left.min(right),
            (Some(start), None) | (None, Some(start)) => start,
            (None, None) => self.pending.len(),
        }
    }
}

pub(super) struct OutputFilter {
    ansi: AnsiFilter,
    utf8: Utf8Filter,
    redactor: Redactor,
}

impl OutputFilter {
    pub(super) fn new(token: Option<&str>) -> Self {
        Self {
            ansi: AnsiFilter::default(),
            utf8: Utf8Filter::default(),
            redactor: Redactor::new(token),
        }
    }

    pub(super) fn push(&mut self, input: &[u8]) -> String {
        let mut clean = Vec::with_capacity(input.len());
        self.ansi.push(input, &mut clean);
        let decoded = self.utf8.push(&clean);
        let decoded = strip_unicode_controls(&decoded);
        self.redactor.push(&decoded)
    }

    pub(super) fn finish(&mut self) -> String {
        let decoded = strip_unicode_controls(&self.utf8.finish());
        let ready = self.redactor.push(&decoded);
        let final_text = self.redactor.finish();
        format!("{ready}{final_text}")
    }
}

fn strip_unicode_controls(input: &str) -> String {
    input
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .collect()
}

fn partial_token_start(input: &str, token: &str) -> Option<usize> {
    input.char_indices().map(|(start, _)| start).find(|&start| {
        let suffix = &input[start..];
        suffix.len() < token.len() && token.starts_with(suffix)
    })
}

fn unfinished_url_start(input: &str) -> Option<usize> {
    let mut search = 0;
    while let Some(relative) = input[search..].find("://") {
        let separator = search + relative;
        let start = scheme_start(input, separator);
        let authority = separator + 3;
        let end = authority_end(input, authority);
        if let Some(start) = start {
            if end == input.len() && !input[authority..end].contains('@') {
                return Some(start);
            }
        }
        search = separator + 3;
    }
    None
}

fn scheme_start(input: &str, separator: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut start = separator;
    while start > 0 && is_scheme_byte(bytes[start - 1]) {
        start -= 1;
    }
    (start < separator && bytes[start].is_ascii_alphabetic()).then_some(start)
}

fn is_scheme_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.')
}

fn authority_end(input: &str, start: usize) -> usize {
    input[start..]
        .find(|character: char| {
            character.is_whitespace()
                || character.is_control()
                || matches!(character, '/' | '?' | '#' | '\'' | '"' | '<' | '>')
        })
        .map(|offset| start + offset)
        .unwrap_or(input.len())
}

fn redact_complete(input: &str, token: Option<&str>) -> String {
    let redacted = redact_url_userinfo(input);
    match token {
        Some(token) => redacted.replace(token, "<redacted>"),
        None => redacted,
    }
}

fn redact_url_userinfo(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(relative) = input[cursor..].find("://") {
        let separator = cursor + relative;
        let authority = separator + 3;
        let end = authority_end(input, authority);
        let credentials = scheme_start(input, separator)
            .and_then(|_| input[authority..end].rfind('@').map(|at| authority + at));
        match credentials {
            Some(at) => {
                output.push_str(&input[cursor..authority]);
                output.push_str("<redacted>@");
                cursor = at + 1;
            }
            None => {
                output.push_str(&input[cursor..authority]);
                cursor = authority;
            }
        }
    }
    output.push_str(&input[cursor..]);
    output
}

enum ProcessChunk {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
}

pub(super) fn run_capture(
    cwd: &Path,
    program: &str,
    args: &[&str],
    token: Option<&str>,
    emit: &mut dyn FnMut(&str),
) -> i32 {
    let mut child = match Command::new(program)
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            emit(&format!("{program} failed to start: {error}"));
            return 1;
        }
    };
    let (sender, receiver) = mpsc::channel();
    let stdout = spawn_reader(
        child.stdout.take().expect("piped stdout"),
        sender.clone(),
        true,
    );
    let stderr = spawn_reader(child.stderr.take().expect("piped stderr"), sender, false);
    forward_chunks(&receiver, token, emit);
    stdout.join().expect("stdout reader panicked");
    stderr.join().expect("stderr reader panicked");
    match child.wait() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            emit(&format!("{program} failed while waiting: {error}"));
            1
        }
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    mut reader: R,
    sender: mpsc::Sender<ProcessChunk>,
    stdout: bool,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buffer = [0; READ_BUFFER_SIZE];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                break;
            }
            let chunk = buffer[..count].to_vec();
            let chunk = if stdout {
                ProcessChunk::Stdout(chunk)
            } else {
                ProcessChunk::Stderr(chunk)
            };
            if sender.send(chunk).is_err() {
                break;
            }
        }
    })
}

fn forward_chunks(
    receiver: &mpsc::Receiver<ProcessChunk>,
    token: Option<&str>,
    emit: &mut dyn FnMut(&str),
) {
    let mut stdout = OutputFilter::new(token);
    let mut stderr = OutputFilter::new(token);
    while let Ok(chunk) = receiver.recv() {
        let message = match chunk {
            ProcessChunk::Stdout(bytes) => stdout.push(&bytes),
            ProcessChunk::Stderr(bytes) => stderr.push(&bytes),
        };
        emit(&message);
    }
    emit(&stdout.finish());
    emit(&stderr.finish());
}

pub(super) fn sanitize(message: &str, token: Option<&str>) -> String {
    let mut filter = OutputFilter::new(token);
    let mut clean = filter.push(message.as_bytes());
    clean.push_str(&filter.finish());
    clean
}
