//! What the integration tests of `neottx` share: running the program, a
//! folder of a test's own, and a WAVE file to read.
#![allow(dead_code, reason = "each integration test uses only some of the helpers")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Runs the `neottx` this package built with `arguments`.
pub fn neottx(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_neottx")).args(arguments).output().expect("run neottx")
}

/// A fresh folder of the test's own: tests run in parallel.
pub fn folder(name: &str) -> PathBuf {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("make the test folder");
    folder
}

/// A mono 16-bit WAVE file of `samples` at `rate`.
pub fn wave(samples: &[i16], rate: u32) -> Vec<u8> {
    let data: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
    let mut bytes = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0".to_vec();
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&[2, 0, 16, 0]);
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&data);
    bytes
}

/// Serves the files under `root` as a static channel at `/ch/`, on a local
/// port, until the test ends: each request's target is noted, in order, and
/// answered with the file or 404. Only plain names inside `root` are served.
pub fn serve_folder(root: PathBuf) -> (u16, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a local port");
    let port = listener.local_addr().expect("its address").port();
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let noted = std::sync::Arc::clone(&asked);
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().expect("the stream"));
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let target = line.split_whitespace().nth(1).unwrap_or_default().to_owned();
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap_or(0) == 0 || header.trim().is_empty() {
                    break;
                }
            }
            noted.lock().expect("the list").push(target.clone());
            let file = target
                .strip_prefix("/ch/")
                .filter(|name| !name.is_empty() && name.split('/').all(|part| !part.is_empty() && part != ".."))
                .and_then(|name| std::fs::read(root.join(name)).ok());
            let list = Path::new(&target).extension().is_some_and(|extension| extension == "sub");
            let media = if list { "text/plain" } else { "application/octet-stream" };
            let head = match &file {
                Some(body) => format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {media}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                ),
                None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
            };
            let _ = stream.write_all(head.as_bytes());
            if let Some(body) = file {
                let _ = stream.write_all(&body);
            }
        }
    });
    (port, asked)
}
