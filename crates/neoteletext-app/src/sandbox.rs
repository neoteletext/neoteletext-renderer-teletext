//! Self-sandboxing (REQ-SEC-017, decision E.33 layer 5).
//!
//! After opening its inputs and outputs, `neottx` restricts itself so that it
//! cannot open other files, start programs, or use the network outside web
//! mode. Linux uses Landlock and a seccomp filter (Phase 1). Other systems
//! are Phase 2: `neottx` says so and continues (decision 15 of
//! IMPLEMENTATION-LOG.md).

use std::path::Path;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(not(target_os = "linux"))]
mod unsupported;
#[cfg(not(target_os = "linux"))]
use unsupported as platform;

/// What the sandboxed process may still do.
#[derive(Clone, Copy, Debug)]
pub struct Policy<'a> {
    /// A directory the process may create and write files in.
    pub writable_directory: Option<&'a Path>,
    /// The process may make network connections (web mode fetcher).
    pub network: bool,
    /// Directories the process may read (the fetcher reads `/etc` to
    /// resolve names).
    pub readable: &'a [&'a Path],
}

/// Why the sandbox could not be applied. The program stops: it runs without
/// a sandbox only when told to with `--no-sandbox`, as each message says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SandboxError {
    /// The kernel does not enforce Landlock at all.
    LandlockUnsupported,
    /// Landlock refused the ruleset, one of its rules (a directory that
    /// cannot be opened among them) or the restriction of the process:
    /// what Landlock or the kernel said.
    Landlock(String),
    /// No seccomp filter exists for this architecture, and the kernel's
    /// Landlock cannot restrict the network either, so a process meant to
    /// have no network would keep it (DEC-2351).
    NetworkUnrestricted,
    /// The seccomp filter could not be built or loaded: what seccompiler
    /// or the kernel said.
    Seccomp(String),
}

/// What the user can do about any of them.
const NO_SANDBOX_HINT: &str = "(use --no-sandbox to run without it)";

impl core::fmt::Display for SandboxError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::LandlockUnsupported => write!(
                formatter,
                "cannot apply the Landlock sandbox: Landlock is not supported by this kernel {NO_SANDBOX_HINT}"
            ),
            Self::Landlock(why) => write!(formatter, "cannot apply the Landlock sandbox: {why} {NO_SANDBOX_HINT}"),
            Self::NetworkUnrestricted => write!(
                formatter,
                "cannot apply the seccomp sandbox: no seccomp filter exists for this architecture, and this kernel's Landlock cannot restrict the network {NO_SANDBOX_HINT}"
            ),
            Self::Seccomp(why) => write!(formatter, "cannot apply the seccomp sandbox: {why} {NO_SANDBOX_HINT}"),
        }
    }
}

impl std::error::Error for SandboxError {}

/// Applies the sandbox. `allow_unsandboxed` is `--no-sandbox`.
///
/// # Errors
/// A sandbox that cannot be applied in full, by [`SandboxError`]'s cases.
pub fn apply(policy: Policy<'_>, allow_unsandboxed: bool) -> Result<(), SandboxError> {
    if allow_unsandboxed {
        eprintln!("neottx: running without a sandbox (--no-sandbox)");
        return Ok(());
    }
    platform::apply(policy)
}

/// The REQ-SEC-017 verification: after the sandbox is applied, opening a
/// file, starting a program and connecting to the network must all fail.
///
/// # Errors
/// The exit status and the message for the user (DEC-2451): the
/// sandbox's [`SandboxError`], or a check that was not blocked.
pub fn self_test() -> Result<(), (u8, String)> {
    apply(Policy { writable_directory: None, network: false, readable: &[] }, false)
        .map_err(|error| (crate::exit::INPUT, error.to_string()))?;
    let file = std::fs::File::open("/etc/hostname").is_err();
    let program = std::process::Command::new("/bin/true").status().is_err();
    let network = std::net::TcpStream::connect(("1.1.1.1", 80)).is_err();
    println!("open file blocked: {file}\nstart program blocked: {program}\nnetwork blocked: {network}");
    if file && program && network {
        Ok(())
    } else {
        Err((crate::exit::INPUT, "the sandbox did not block everything".to_string()))
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good"
)]
mod tests {
    use super::{Policy, SandboxError, apply};

    /// REQ-155: a directory the sandbox cannot open is Landlock's error,
    /// an enumeration whose message tells the user the way out. Opening the
    /// directory fails before the process is restricted, so the test
    /// process stays free.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_directory_that_cannot_be_opened_is_landlocks_error() {
        let missing = std::env::temp_dir().join("neoteletext-sandbox-test-no-such-directory");
        assert!(!missing.exists(), "{} must not exist", missing.display());
        let readable = [missing.as_path()];
        let refused = apply(Policy { writable_directory: None, network: false, readable: &readable }, false);
        assert!(matches!(refused, Err(SandboxError::Landlock(_))), "{refused:?}");
        let message = refused.unwrap_err().to_string();
        assert!(message.starts_with("cannot apply the Landlock sandbox: "), "{message}");
        assert!(message.ends_with("(use --no-sandbox to run without it)"), "{message}");
    }

    /// REQ-155: each case keeps the message the program printed before it
    /// was an enumeration.
    #[test]
    fn each_case_says_what_failed_and_the_way_out() {
        assert_eq!(
            SandboxError::LandlockUnsupported.to_string(),
            "cannot apply the Landlock sandbox: Landlock is not supported by this kernel (use --no-sandbox to run without it)"
        );
        assert_eq!(
            SandboxError::NetworkUnrestricted.to_string(),
            "cannot apply the seccomp sandbox: no seccomp filter exists for this architecture, and this kernel's Landlock cannot restrict the network (use --no-sandbox to run without it)"
        );
        assert_eq!(
            SandboxError::Seccomp("refused".into()).to_string(),
            "cannot apply the seccomp sandbox: refused (use --no-sandbox to run without it)"
        );
    }
}
