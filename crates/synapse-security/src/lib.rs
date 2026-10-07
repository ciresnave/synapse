// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sinks for `synapse::security_events` (brute-force hardening P4).
//!
//! The file, stderr and fanout sinks are complete. Alert delivery is only a seam
//! ([`AlertTransport`], no implementation) and awaits board 131. A sink must never take down its
//! caller, so I/O errors are reported once on stderr and then swallowed.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Utc};
use synapse::security_events::{
    Alert, AlertPolicy, SecurityEvent, SecurityEventKind, SecuritySink, sanitize,
};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

struct FileState {
    file: Option<File>,
    size: u64,
}

/// Appends one JSON line per event to an owner-only file, rotating once to `<path>.1`.
pub struct FileSink {
    path: PathBuf,
    max_bytes: u64,
    state: Mutex<FileState>,
}

impl FileSink {
    /// Opens (creating, owner-only) the file at `path`. When a line would take it past `max_bytes`,
    /// the file is renamed to `<path>.1`, replacing any older one, and a fresh file is started, so a
    /// flood can never fill the disk past about twice `max_bytes`.
    pub fn open(path: impl AsRef<Path>, max_bytes: u64) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = open_append(&path)?;
        let size = file.metadata()?.len();
        Ok(Self {
            path,
            max_bytes,
            state: Mutex::new(FileState {
                file: Some(file),
                size,
            }),
        })
    }

    fn rotated_path(&self) -> PathBuf {
        let mut name = self.path.clone().into_os_string();
        name.push(".1");
        PathBuf::from(name)
    }

    fn write_line(&self, line: &[u8]) -> std::io::Result<()> {
        let mut state = lock(&self.state);
        let len = line.len() as u64;
        if state.size > 0 && state.size + len > self.max_bytes {
            // Close before renaming: an open file cannot be renamed on Windows.
            state.file = None;
            let old = self.rotated_path();
            let _ = std::fs::remove_file(&old);
            std::fs::rename(&self.path, &old)?;
            state.size = 0;
        }
        if state.file.is_none() {
            state.file = Some(open_append(&self.path)?);
            state.size = state
                .file
                .as_ref()
                .map_or(0, |f| f.metadata().map_or(0, |m| m.len()));
        }
        if let Some(file) = state.file.as_mut() {
            file.write_all(line)?;
            file.flush()?;
            state.size += len;
        }
        Ok(())
    }
}

/// Opens for append, creating the file owner-only on Unix (the same approach as the daemon's announce
/// file). On Windows the file inherits the home directory's ACL.
fn open_append(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

impl SecuritySink for FileSink {
    fn record(&self, event: &SecurityEvent) {
        let Ok(mut line) = serde_json::to_vec(event) else {
            eprintln!("security: could not serialize an event");
            return;
        };
        line.push(b'\n');
        if let Err(e) = self.write_line(&line) {
            eprintln!("security: could not write {}: {e}", self.path.display());
        }
    }
}

/// Writes `security: <kind> <surface> <subject> <detail>` to stderr, one line per event.
pub struct StderrSink;

impl SecuritySink for StderrSink {
    fn record(&self, event: &SecurityEvent) {
        let kind = serde_json::to_value(event.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        // Fields were sanitized on construction, but the struct's fields are public: do it again.
        eprintln!(
            "security: {kind} {} {} {}",
            sanitize(&event.surface),
            sanitize(&event.subject),
            sanitize(&event.detail)
        );
    }
}

/// Delivers an alert to a person. No implementation ships: delivery waits on board 131.
pub trait AlertTransport: Send + Sync {
    /// Sends the alert, or says why it could not.
    fn send(&self, alert: &Alert) -> Result<(), String>;
}

/// Feeds events to an [`AlertPolicy`] and sends what it emits. A failed send becomes an
/// `AlertFailed` event in the fallback sink and is never retried through the same transport, so a
/// dead transport cannot loop.
pub struct AlertSink<T: AlertTransport> {
    policy: AlertPolicy,
    transport: T,
    fallback: Arc<dyn SecuritySink>,
}

impl<T: AlertTransport> AlertSink<T> {
    /// A sink sending through `transport`, recording delivery failures into `fallback`.
    pub fn new(policy: AlertPolicy, transport: T, fallback: Arc<dyn SecuritySink>) -> Self {
        Self {
            policy,
            transport,
            fallback,
        }
    }

    /// Sends any digests that are due; call periodically.
    pub fn flush(&self, now: DateTime<Utc>) {
        for alert in self.policy.flush(now) {
            self.deliver(&alert, now);
        }
    }

    fn deliver(&self, alert: &Alert, at: DateTime<Utc>) {
        if let Err(reason) = self.transport.send(alert) {
            let failure = SecurityEvent::new(
                at,
                SecurityEventKind::AlertFailed,
                "security/alert",
                &format!("{:?}", alert.kind),
                None,
                &reason,
            );
            self.fallback.record(&failure);
        }
    }
}

impl<T: AlertTransport> SecuritySink for AlertSink<T> {
    fn record(&self, event: &SecurityEvent) {
        if let Some(alert) = self.policy.observe(event) {
            self.deliver(&alert, event.at);
        }
    }
}

/// Records each event into every sink.
pub struct FanoutSink(pub Vec<Arc<dyn SecuritySink>>);

impl SecuritySink for FanoutSink {
    fn record(&self, event: &SecurityEvent) {
        for sink in &self.0 {
            sink.record(event);
        }
    }
}
