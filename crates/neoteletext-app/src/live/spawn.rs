//! Starting the fetcher process and reading its frames on a thread of
//! their own (REQ-SEC-018).

use super::Live;
use crate::pipe::{self, Frame, Notice};
use crate::source::{Source, Wake};
use crate::{Failure, exit};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::mpsc;

/// Starts the fetcher process for `address`. Must run before sandboxing:
/// starting a program is blocked afterwards.
///
/// # Errors
/// A [`Failure`]: the exit status and the message for the user (DEC-2451),
/// for a link that wraps no web address, or when the fetcher program cannot
/// be found or started.
pub fn open(
    address: &str,
    no_sandbox: bool,
    allow_local: &[std::net::IpAddr],
    proxy: Option<&str>,
    region_named: bool,
    walk: bool,
) -> Result<Box<dyn Source>, Failure> {
    let address = crate::input::link_address(address).map_err(|error| (exit::USAGE, error.to_string()))?;
    let program = std::env::current_exe().map_err(|error| (exit::INPUT, error.to_string()))?;
    let mut command = Command::new(program);
    command.arg("fetch").arg(address).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit());
    if no_sandbox {
        command.arg("--no-sandbox");
    }
    for address in allow_local {
        command.arg("--allow-local").arg(address.to_string());
    }
    if let Some(proxy) = proxy {
        command.arg("--proxy").arg(proxy);
    }
    if walk {
        command.arg("--walk");
    }
    let mut child = command.spawn().map_err(|error| (exit::INPUT, format!("cannot start the fetcher: {error}")))?;
    let requests = child.stdin.take().ok_or((exit::INPUT, "no pipe to the fetcher".to_string()))?;
    let mut output = child.stdout.take().ok_or((exit::INPUT, "no pipe from the fetcher".to_string()))?;
    let (sender, frames) = mpsc::channel();
    let live = Live::new(Some(child), Box::new(requests), frames, region_named);
    let wake = live.wake.clone();
    std::thread::spawn(move || read_frames(&mut output, &sender, &wake));
    Ok(Box::new(live))
}

/// A font file fetched by the fetcher process from the address the user
/// named (REQ-123, REQ-SEC-018), and whether it came encrypted. Must run
/// before sandboxing, as [`open`] must. The fetcher applies the network rules
/// of a channel's files; the caller checks the font's SHA-256 and reads it.
///
/// # Errors
/// A [`Failure`] when the fetcher cannot be started, when it says why the
/// font could not be fetched, or when it ends without sending one.
pub fn fetch_font(
    address: &str,
    no_sandbox: bool,
    allow_local: &[std::net::IpAddr],
    proxy: Option<&str>,
) -> Result<(Vec<u8>, bool), Failure> {
    let program = std::env::current_exe().map_err(|error| (exit::INPUT, error.to_string()))?;
    let mut command = Command::new(program);
    command
        .arg("fetch")
        .arg("--font")
        .arg(address)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if no_sandbox {
        command.arg("--no-sandbox");
    }
    for address in allow_local {
        command.arg("--allow-local").arg(address.to_string());
    }
    if let Some(proxy) = proxy {
        command.arg("--proxy").arg(proxy);
    }
    let mut child = command.spawn().map_err(|error| (exit::INPUT, format!("cannot start the fetcher: {error}")))?;
    let mut output = child.stdout.take().ok_or((exit::INPUT, "no pipe from the fetcher".to_string()))?;
    let mut message = None;
    let mut secure = true;
    let font = loop {
        // Every frame is validated by pipe::read; a bad one ends the link.
        match pipe::read(&mut output) {
            Ok(Some(Frame::Font(bytes))) => break Some((bytes, secure)),
            Ok(Some(Frame::Notice(Notice::Unencrypted))) => secure = false,
            Ok(Some(Frame::Notice(Notice::Message(text)))) => message = Some(text),
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break None,
        }
    };
    // Closing its input ends the fetcher if it has not ended.
    drop(child.stdin.take());
    let _ = child.wait();
    font.ok_or_else(|| {
        (exit::INPUT, format!("{address}: {}", message.unwrap_or_else(|| "the fetcher sent no font".to_string())))
    })
}

/// Reads the fetcher's frames until its pipe ends, passing each on and
/// waking the viewer, if it asked to be woken, as it comes.
pub(super) fn read_frames(output: &mut dyn std::io::Read, sender: &mpsc::Sender<Frame>, wake: &OnceLock<Wake>) {
    // Every frame is validated by pipe::read; a bad one ends the link.
    while let Ok(Some(frame)) = pipe::read(output) {
        if sender.send(frame).is_err() {
            return;
        }
        if let Some(wake) = wake.get() {
            wake();
        }
    }
    // The end is news too: the viewer sees the fetcher gone.
    if let Some(wake) = wake.get() {
        wake();
    }
}
