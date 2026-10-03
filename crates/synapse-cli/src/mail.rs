// SPDX-License-Identifier: MIT OR Apache-2.0
//! The mail commands' client for `synapsed` (M5b): find the daemon through its owner-only announce
//! file, start it if nothing answers, and keep one session per role.
//!
//! Design: `docs/superpowers/specs/2026-10-02-m5-synapsed-design.md` (Q2, Q4) and the M5b plan.
//! Session tokens are cached owner-only in `<home>/sessions/<role>.json`, so commands after a claim
//! reuse it instead of re-claiming (which would supersede the role's other holder). An implicit
//! claim happens only when there is no cached session for the running daemon instance, under a
//! per-role lock so concurrent first commands claim once. A 409 `superseded` or a 401 is reported,
//! never answered by re-claiming: taking a role back is always an explicit `synapse claim`.
//!
//! **One holder per (home, role).** The cache is keyed by role, not by holder, so every `synapse`
//! command for a role in a home shares one session: a `synapse claim` re-points all of them, and
//! supersession is only ever reported to holders outside this home's cache. Per-holder sessions are
//! M5c.
//!
//! **The daemon is not yet authenticated (#74).** The announce file is trusted only if it, the
//! session cache and its directory pass the keystore's owner-only check, and only while its `pid`
//! is a live process (on Windows, a live `synapsed`). That stops a stale port, left by a crashed
//! daemon, from receiving tokens, but proof that the listener holds the instance id is M5c.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use synapse::certificate::chain_to_pem;
use synapse::keystore::{Keystore, KeystoreError, check_owner_only};
use synapse::roles::sign_claim_for;

/// The daemon's announce file in the home, written by `synapsed` (M5a).
const ANNOUNCE_FILE: &str = "synapsed.json";
/// How long a started daemon gets to announce itself and answer health.
const START_TIMEOUT: Duration = Duration::from_secs(5);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a command waits for another command's claim of the same role.
const LOCK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub enum MailError {
    Keystore(KeystoreError),
    /// `synapsed` could not be started or never answered.
    NoDaemon(String),
    /// The daemon could not be reached or answered something unreadable.
    Transport(String),
    /// The daemon refused the call.
    Api {
        kind: String,
        message: String,
        current: Option<u64>,
    },
    Io(std::io::Error),
    Usage(String),
}

impl fmt::Display for MailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MailError::Keystore(e) => write!(f, "{e}"),
            MailError::NoDaemon(why) => write!(f, "synapsed is not running: {why}"),
            MailError::Transport(why) => write!(f, "talking to synapsed: {why}"),
            MailError::Api {
                kind,
                current: Some(current),
                ..
            } if kind == "superseded" => write!(
                f,
                "superseded: another holder claimed this role (now epoch {current}); \
                 run `synapse claim` to take it back"
            ),
            MailError::Api { kind, .. } if kind == "unauthorized" => write!(
                f,
                "unauthorized: the daemon no longer knows this session (another holder may have \
                 claimed the role); run `synapse claim` to claim it"
            ),
            MailError::Api { kind, message, .. } => {
                write!(f, "{}: {}", printable(kind), printable(message))
            }
            MailError::Io(e) => write!(f, "{e}"),
            MailError::Usage(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for MailError {}

impl From<KeystoreError> for MailError {
    fn from(e: KeystoreError) -> Self {
        MailError::Keystore(e)
    }
}

impl From<std::io::Error> for MailError {
    fn from(e: std::io::Error) -> Self {
        MailError::Io(e)
    }
}

#[derive(Deserialize)]
struct Announce {
    addr: String,
    instance_id: String,
    pid: u32,
}

/// A session as cached on disk: valid only for the daemon instance that issued it.
#[derive(Serialize, Deserialize)]
struct Cached {
    instance_id: String,
    token: String,
    global_id: String,
    epoch: u64,
}

/// What a successful claim granted.
pub struct Claimed {
    pub global_id: String,
    pub epoch: u64,
}

/// A live connection to this home's daemon.
pub struct Daemon {
    http: reqwest::blocking::Client,
    home: PathBuf,
    addr: String,
    instance_id: String,
    started_at: DateTime<Utc>,
}

impl Daemon {
    /// Find this home's daemon, starting it if nothing answers.
    pub fn connect(home: &Path) -> Result<Daemon, MailError> {
        let http = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(CALL_TIMEOUT)
            .build()
            .map_err(|e| MailError::Transport(e.to_string()))?;
        // Two tries, so one slow answer from a live daemon doesn't start a doomed second one.
        for _ in 0..2 {
            if let Some(found) = answering(&http, home)? {
                return Ok(Daemon::new(http, home, found));
            }
        }
        let mut child = spawn_daemon(home)?;
        eprintln!("synapse: started synapsed (pid {})", child.id());
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            // Whichever daemon answers will do: ours, or, if two CLIs raced, the one holding the
            // store (ours then exits on its lock), or a live one that was only slow to answer.
            if let Some(found) = answering(&http, home)? {
                return Ok(Daemon::new(http, home, found));
            }
            if Instant::now() >= deadline {
                let why = match child.try_wait() {
                    Ok(Some(status)) => {
                        format!("synapsed exited ({status}) and no daemon answered")
                    }
                    _ => "synapsed did not answer within 5 s".to_string(),
                };
                return Err(MailError::NoDaemon(why));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn new(
        http: reqwest::blocking::Client,
        home: &Path,
        (announce, started_at): (Announce, DateTime<Utc>),
    ) -> Daemon {
        Daemon {
            http,
            home: home.to_path_buf(),
            addr: announce.addr,
            instance_id: announce.instance_id,
            started_at,
        }
    }

    /// Claim `role` afresh (a takeover if someone holds it) and cache the session.
    pub fn claim(&self, role: &str) -> Result<Claimed, MailError> {
        let _lock = RoleLock::acquire(&self.home, role)?;
        let cached = self.claim_session(role)?;
        Ok(Claimed {
            global_id: cached.global_id,
            epoch: cached.epoch,
        })
    }

    fn claim_session(&self, role: &str) -> Result<Cached, MailError> {
        let store = Keystore::open(&self.home)?;
        let identity = store.role(role, Utc::now())?;
        let nonce = random_bytes::<16>()?;
        // M3 refuses claims signed at or before the daemon's start.
        let signed_at = Utc::now().max(self.started_at + chrono::Duration::seconds(1));
        let req = sign_claim_for(&identity, &self.instance_id, nonce, signed_at)
            .map_err(|e| MailError::Usage(format!("cannot sign the claim: {e}")))?;
        let body = json!({
            "audience": req.audience,
            "chain_pem": chain_to_pem(&req.chain),
            "nonce_hex": hex(&req.nonce),
            "signed_at": req.signed_at.to_rfc3339(),
            "signature_b64": B64.encode(&req.signature),
        });
        let reply = self.request("/v1/claim", None, Some(&body))?;
        let field = |name: &str| MailError::Transport(format!("the claim reply has no `{name}`"));
        let cached = Cached {
            instance_id: self.instance_id.clone(),
            token: reply["session"]
                .as_str()
                .ok_or_else(|| field("session"))?
                .to_string(),
            global_id: reply["global_id"]
                .as_str()
                .ok_or_else(|| field("global_id"))?
                .to_string(),
            epoch: reply["epoch"].as_u64().ok_or_else(|| field("epoch"))?,
        };
        save_session(&self.home, role, &cached)?;
        Ok(cached)
    }

    /// An authenticated call as `role`: the cached session if it belongs to this daemon instance,
    /// else a fresh claim. Any refusal, 401 and 409 included, is returned as an error: a 401 for
    /// this instance's session means the daemon dropped it, which happens after enough takeovers,
    /// so re-claiming then would silently steal the role back (review I1).
    pub fn call(&self, role: &str, path: &str, body: Option<&Value>) -> Result<Value, MailError> {
        let session = self.session(role)?;
        self.request(path, Some(&session.token), body)
    }

    /// The cached session for this daemon instance, else a claim. Under the role lock, so two
    /// first commands for one role claim once rather than superseding each other (review I4).
    fn session(&self, role: &str) -> Result<Cached, MailError> {
        let current = |c: &Cached| c.instance_id == self.instance_id;
        if let Some(c) = load_session(&self.home, role)?.filter(current) {
            return Ok(c);
        }
        let _lock = RoleLock::acquire(&self.home, role)?;
        match load_session(&self.home, role)?.filter(current) {
            Some(c) => Ok(c),
            None => self.claim_session(role),
        }
    }

    /// POST with a JSON body, or GET without one.
    fn request(
        &self,
        path: &str,
        token: Option<&str>,
        body: Option<&Value>,
    ) -> Result<Value, MailError> {
        let url = format!("http://{}{path}", self.addr);
        let mut req = match body {
            Some(b) => self.http.post(url).json(b),
            None => self.http.get(url),
        };
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        let resp = req
            .send()
            .map_err(|e| MailError::Transport(e.without_url().to_string()))?;
        let status = resp.status();
        let text = resp
            .text()
            .map_err(|e| MailError::Transport(e.without_url().to_string()))?;
        let value: Value = serde_json::from_str(&text).map_err(|_| {
            MailError::Transport(format!("HTTP {status}, and the reply is not JSON"))
        })?;
        let ok = status.is_success();
        if ok {
            return Ok(value);
        }
        Err(MailError::Api {
            kind: value["error"].as_str().unwrap_or("error").to_string(),
            message: value["message"].as_str().unwrap_or("").to_string(),
            current: value["current"].as_u64(),
        })
    }
}

/// The announced daemon and its start time, if it is alive and answers health.
fn answering(
    http: &reqwest::blocking::Client,
    home: &Path,
) -> Result<Option<(Announce, DateTime<Utc>)>, MailError> {
    let Some(announce) = read_announce(home)? else {
        return Ok(None);
    };
    Ok(health(http, &announce.addr).map(|started_at| (announce, started_at)))
}

/// The announce file, if present, owner-only, readable, and naming a live daemon. A file left by a
/// dead daemon is ignored, so a process that took its port gets no token (#74); a file others can
/// reach is an error, since whoever can write it chooses where the token goes.
fn read_announce(home: &Path) -> Result<Option<Announce>, MailError> {
    let path = home.join(ANNOUNCE_FILE);
    if !path.try_exists()? {
        return Ok(None);
    }
    check_owner_only(&path, "daemon announce file")?;
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    Ok(serde_json::from_str::<Announce>(&text)
        .ok()
        .filter(|a| daemon_alive(a.pid)))
}

/// Whether `pid` is a live process of this user (on Windows, a live `synapsed`). This is the
/// interim stand-in for #74's proof of the instance id: it catches a stale announce file, not a
/// reused pid on Unix.
#[cfg(unix)]
fn daemon_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only checks that `pid` exists and may be signalled by us; it sends nothing.
    // EPERM (another user's process) is a failure here, which is what we want.
    pid > 0 && unsafe { libc::kill(pid, 0) } == 0
}

#[cfg(windows)]
fn daemon_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    // SAFETY: OpenProcess takes no pointers; a null result is handled.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let mut code = 0u32;
    let mut name = [0u16; 1024];
    let mut len = name.len() as u32;
    // SAFETY: `handle` is open until the CloseHandle below; `code`, `name` and `len` are valid for
    // writes, and `len` is `name`'s capacity in u16s.
    let (running, named) = unsafe {
        (
            GetExitCodeProcess(handle, &mut code) != 0 && code == STILL_ACTIVE as u32,
            QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, name.as_mut_ptr(), &mut len)
                != 0,
        )
    };
    // SAFETY: opened above and closed exactly once.
    unsafe { CloseHandle(handle) };
    let image = String::from_utf16_lossy(&name[..len as usize]);
    let file = Path::new(&image)
        .file_name()
        .map(|f| f.to_string_lossy().to_ascii_lowercase());
    running && named && file.as_deref() == Some("synapsed.exe")
}

#[cfg(not(any(unix, windows)))]
fn daemon_alive(_pid: u32) -> bool {
    true
}

/// The daemon's start time if `/v1/health` at `addr` answers ok.
fn health(http: &reqwest::blocking::Client, addr: &str) -> Option<DateTime<Utc>> {
    let reply: Value = http
        .get(format!("http://{addr}/v1/health"))
        .timeout(HEALTH_TIMEOUT)
        .send()
        .ok()?
        .json()
        .ok()?;
    if reply["ok"] != true {
        return None;
    }
    reply["started_at"].as_str()?.parse().ok()
}

/// `synapsed` next to this executable, else from PATH.
fn synapsed_path() -> PathBuf {
    let name = format!("synapsed{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(&name)))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

/// Start `synapsed` detached from this process: no console, no stdio, its own process group, and
/// the home as its working directory (so it pins no directory the caller may want to delete). The
/// environment is inherited, so `SYNAPSE_HOME` and `SYNAPSE_ADDR` reach it.
fn spawn_daemon(home: &Path) -> Result<std::process::Child, MailError> {
    let mut cmd = Command::new(synapsed_path());
    cmd.current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        stop_stdio_inheritance();
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: runs in the forked child before exec; `setsid` is async-signal-safe and touches
        // no memory of ours. A new session takes the daemon out of the terminal's job control, so
        // neither Ctrl-C nor the terminal closing (SIGHUP) reaches it.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    cmd.spawn()
        .map_err(|e| MailError::NoDaemon(format!("cannot start synapsed: {e}")))
}

/// A Windows child inherits every inheritable handle of its parent, not only the stdio it is given.
/// This process's stdio may be a caller's pipe: a daemon holding it would keep that pipe open, and
/// a caller reading to EOF (`Command::output`, a shell's `$(...)`) would wait for the daemon's
/// whole life. So this process's own stdio is made non-inheritable before the spawn.
#[cfg(windows)]
fn stop_stdio_inheritance() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
    for handle in [
        std::io::stdin().as_raw_handle(),
        std::io::stdout().as_raw_handle(),
        std::io::stderr().as_raw_handle(),
    ] {
        if !handle.is_null() {
            // SAFETY: the handle is this process's own stdio, open for the process's life;
            // clearing its inherit flag changes no ownership and frees nothing.
            unsafe {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

fn session_path(home: &Path, role: &str) -> PathBuf {
    home.join("sessions").join(format!("{role}.json"))
}

/// `<home>/sessions`, created owner-only if missing.
fn sessions_dir(home: &Path) -> Result<PathBuf, MailError> {
    let dir = home.join("sessions");
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&dir)?;
    // `mode` applies only to a directory created now, so an existing one is checked too.
    check_owner_only(&dir, "session directory")?;
    Ok(dir)
}

/// An exclusive per-role OS lock on `<home>/sessions/<role>.lock`, held while this value lives.
/// The OS releases it when the holder exits, crashed or not, so a leftover file blocks nobody and
/// no lock is ever broken out from under a live holder. The file itself is left in place.
struct RoleLock(#[allow(dead_code)] std::fs::File);

impl RoleLock {
    fn acquire(home: &Path, role: &str) -> Result<RoleLock, MailError> {
        let path = sessions_dir(home)?.join(format!("{role}.lock"));
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        let deadline = Instant::now() + LOCK_TIMEOUT;
        loop {
            if try_lock(&file)? {
                return Ok(RoleLock(file));
            }
            if Instant::now() >= deadline {
                return Err(MailError::Usage(format!(
                    "another synapse command is claiming `{role}`; try again"
                )));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Take an exclusive lock on `file` without waiting: `Ok(false)` if another process holds it.
/// (`File::try_lock` would do, but it needs Rust 1.89 and this crate's MSRV is 1.88.)
#[cfg(unix)]
fn try_lock(file: &std::fs::File) -> Result<bool, MailError> {
    use std::os::unix::io::AsRawFd;
    // SAFETY: the descriptor is open for `file`'s lifetime; flock takes no pointers.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(true);
    }
    let e = std::io::Error::last_os_error();
    if e.kind() == std::io::ErrorKind::WouldBlock {
        Ok(false)
    } else {
        Err(e.into())
    }
}

#[cfg(windows)]
fn try_lock(file: &std::fs::File) -> Result<bool, MailError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::ERROR_LOCK_VIOLATION;
    use windows_sys::Win32::Storage::FileSystem::{
        LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;
    // SAFETY: an all-zero OVERLAPPED (offset 0, no event) is valid for a synchronous handle.
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    // SAFETY: the handle is open for `file`'s lifetime and `overlapped` outlives the call, which
    // does not wait (FAIL_IMMEDIATELY). The lock is released when the handle closes.
    let locked = unsafe {
        LockFileEx(
            file.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            &mut overlapped,
        )
    } != 0;
    if locked {
        return Ok(true);
    }
    let e = std::io::Error::last_os_error();
    if e.raw_os_error() == Some(ERROR_LOCK_VIOLATION as i32) {
        Ok(false)
    } else {
        Err(e.into())
    }
}

#[cfg(not(any(unix, windows)))]
fn try_lock(_file: &std::fs::File) -> Result<bool, MailError> {
    Ok(true)
}

/// The cached session, if any. A cache others can reach is an error: anyone who can read it holds
/// the role, and anyone who can write it chooses the token.
fn load_session(home: &Path, role: &str) -> Result<Option<Cached>, MailError> {
    sessions_dir(home)?;
    let path = session_path(home, role);
    if !path.try_exists()? {
        return Ok(None);
    }
    check_owner_only(&path, "session file")?;
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    Ok(serde_json::from_str(&text).ok())
}

/// Write the session atomically and owner-only: a fresh temp file (`create_new`, so nothing planted
/// at its name is followed) created `0600` on Unix (on Windows it inherits the checked session
/// directory's ACL), then renamed.
fn save_session(home: &Path, role: &str, cached: &Cached) -> Result<(), MailError> {
    let dir = sessions_dir(home)?;
    let temp = dir.join(format!(".tmp-{role}-{}", hex(&random_bytes::<8>()?)));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    {
        use std::io::Write;
        let mut file = options.open(&temp)?;
        file.write_all(
            serde_json::to_string(cached)
                .map_err(|e| MailError::Usage(e.to_string()))?
                .as_bytes(),
        )?;
        file.sync_all()?;
    }
    std::fs::rename(&temp, session_path(home, role))?;
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn random_bytes<const N: usize>() -> Result<[u8; N], MailError> {
    use ring::rand::SecureRandom;
    let mut out = [0u8; N];
    ring::rand::SystemRandom::new()
        .fill(&mut out)
        .map_err(|_| MailError::Usage("the system random source failed".to_string()))?;
    Ok(out)
}

/// A fetched message as one JSON line: the daemon's fields, plus `body` when the body is UTF-8.
pub fn inbox_line(message: &Value) -> Value {
    let mut out = message.clone();
    if let Some(text) = message["body_b64"]
        .as_str()
        .and_then(|b| B64.decode(b).ok())
        .and_then(|bytes| String::from_utf8(bytes).ok())
    {
        out["body"] = Value::String(text);
    }
    out
}

/// Encode a message body for the wire.
pub fn encode_body(body: &[u8]) -> String {
    B64.encode(body)
}

/// Daemon text made safe for a terminal: control characters (escape sequences included) are shown
/// escaped, not sent raw. Only JSON-escaped output (`inbox`, `list`) can skip this.
pub fn printable(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() {
                c.escape_default().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_text_reaches_the_terminal_without_control_characters() {
        assert_eq!(printable("queued"), "queued");
        assert_eq!(printable("a\u{1b}[2Jb\r\n"), "a\\u{1b}[2Jb\\r\\n");
        let refused = MailError::Api {
            kind: "malformed\u{7}".into(),
            message: "\u{1b}]0;owned\u{7}".into(),
            current: None,
        }
        .to_string();
        assert!(!refused.chars().any(char::is_control), "{refused:?}");
    }
}
