//! Test-only support for CP6 pipeline tests: a scripted, in-process build
//! runner HTTP server.

use std::sync::Mutex;

/// What the scripted runner answers with on each build call.
#[derive(Clone)]
pub(crate) enum PipelineScript {
    /// Runner succeeds with this image id and exit code.
    Ok { image_id: String, exit_code: i32 },
    /// Runner returns a failure detail with this exit code.
    Error { detail: String, exit_code: i32 },
}

fn response_body(script: &PipelineScript) -> String {
    match script {
        PipelineScript::Ok {
            image_id,
            exit_code,
        } => format!(
            r#"{{"status":"succeeded","image_id":"{image_id}","exit_code":{exit_code},"duration_ms":5,"log_tail":"ok","truncated":false,"detail":null}}"#
        ),
        PipelineScript::Error { detail, exit_code } => format!(
            r#"{{"status":"failed","exit_code":{exit_code},"duration_ms":5,"log_tail":"boom","truncated":false,"detail":"{}"}}"#,
            detail.replace('\\', "\\\\")
        ),
    }
}

/// Installs a scripted runner and returns its base URL.
///
/// The runner binds 127.0.0.1 on an ephemeral port and answers every build
/// request with the scripted response. `delay_ms` holds each response so
/// tests can observe intermediate pipeline stages deterministically.
pub(crate) fn start_scripted_runner(script: PipelineScript, delay_ms: u64) -> String {
    use std::io::{Read, Write};
    use std::time::Duration;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let script = Mutex::new(script);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(stream) => stream,
                Err(_) => return,
            };
            let mut buffer = [0_u8; 16 * 1024];
            let read = stream.read(&mut buffer).unwrap_or(0);
            if read == 0 {
                continue;
            }
            if delay_ms > 0 {
                std::thread::sleep(Duration::from_millis(delay_ms));
            }
            let payload = response_body(&script.lock().unwrap().clone());
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
    });
    format!("http://{address}")
}
