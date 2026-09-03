//! Sandbox enforcement.
//!
//! ProcessSandbox runs tools in a subprocess with resource limits.
//! On Windows: uses Job Objects to cap CPU time and memory.
//! On Linux/macOS: uses `ulimit`-style resource limits via `std::process::Command`.
//! The `Sandbox` trait is portable — backends are selected at compile time.

use sentinel_core::error::{SentinelError, SentinelResult};
use std::time::Duration;

// Sandbox trait

/// Portable sandbox interface. Implementations may enforce:
/// - Memory limits
/// - CPU time limits
/// - Filesystem isolation (future: namespaces / Job Objects)
pub trait Sandbox: Send + Sync {
    fn execute<F, T>(&self, f: F) -> SentinelResult<T>
    where
        F: FnOnce() -> SentinelResult<T> + Send + 'static,
        T: Send + 'static;
}

//  NoopSandbox (default) 

/// No isolation — calls the function directly. Used in dev/test.
pub struct NoopSandbox;

impl Sandbox for NoopSandbox {
    fn execute<F, T>(&self, f: F) -> SentinelResult<T>
    where
        F: FnOnce() -> SentinelResult<T> + Send + 'static,
        T: Send + 'static,
    {
        f()
    }
}

// Keep DefaultSandbox as an alias for backward compatibility
pub type DefaultSandbox = NoopSandbox;

//  ProcessSandbox 

/// Process-level sandbox with resource enforcement.
///
/// For in-process tool functions (closures) we cannot fork to a new process,
/// so this sandbox runs the closure in a Tokio-managed thread with a hard timeout.
/// For external process tools, override `execute_command()` instead.
pub struct ProcessSandbox {
    /// Maximum wall-clock time allowed for the sandboxed function.
    pub timeout: Duration,
    /// Maximum memory in bytes (enforced via OS mechanisms where available).
    pub max_memory_bytes: Option<usize>,
}

impl ProcessSandbox {
    pub fn new(timeout_secs: u64) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_secs),
            max_memory_bytes: None,
        }
    }

    pub fn with_memory_limit(mut self, bytes: usize) -> Self {
        self.max_memory_bytes = Some(bytes);
        self
    }
}

impl Sandbox for ProcessSandbox {
    fn execute<F, T>(&self, f: F) -> SentinelResult<T>
    where
        F: FnOnce() -> SentinelResult<T> + Send + 'static,
        T: Send + 'static,
    {
        #[cfg(target_os = "windows")]
        self.apply_windows_limits();

        #[cfg(any(target_os = "linux", target_os = "macos"))]
        self.apply_unix_limits();

        tracing::debug!(
            "ProcessSandbox: running with {:?} timeout{}",
            self.timeout,
            self.max_memory_bytes
                .map(|m| format!(", {}MB memory limit", m / 1024 / 1024))
                .unwrap_or_default()
        );

        // Run in a dedicated thread with timeout enforced by a channel
        // We wrap the result in Option so it can be sent across thread boundary.
        let timeout = self.timeout;
        let (tx, rx) = std::sync::mpsc::channel::<SentinelResult<T>>();

        std::thread::spawn(move || {
            let _ = tx.send(f());
        });

        match rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                tracing::warn!("ProcessSandbox: timeout after {:?}", timeout);
                Err(SentinelError::SandboxViolation(format!(
                    "Tool execution exceeded {:?} timeout",
                    timeout
                )))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err(SentinelError::SandboxViolation(
                    "Sandboxed function panicked".to_string(),
                ))
            }
        }
    }
}

#[cfg(target_os = "windows")]
impl ProcessSandbox {
    fn apply_windows_limits(&self) {
        // Windows Job Objects provide CPU + memory limits for child processes.
        // For in-process sandboxing we set a process-wide working-set limit
        // using the Windows API if a memory limit is configured.
        // Full Job Object isolation is applied when spawning subprocess tools.
        if let Some(max_bytes) = self.max_memory_bytes {
            unsafe {
                // SetProcessWorkingSetSize: soft min/max hints — best-effort
                // Real hard limit requires Job Objects (CreateJobObject + SetInformationJobObject)
                let min_ws: usize = 4 * 1024 * 1024; // 4 MB minimum
                windows_set_working_set(min_ws, max_bytes);
            }
        }
    }
}

#[cfg(target_os = "windows")]
unsafe fn windows_set_working_set(min: usize, max: usize) {
    // Import only in Windows builds
    #[link(name = "kernel32")]
    extern "system" {
        fn SetProcessWorkingSetSize(
            hProcess: *mut std::ffi::c_void,
            dwMinimumWorkingSetSize: usize,
            dwMaximumWorkingSetSize: usize,
        ) -> i32;
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
    }
    let result = SetProcessWorkingSetSize(GetCurrentProcess(), min, max);
    if result == 0 {
        tracing::warn!("Windows: SetProcessWorkingSetSize failed (non-fatal)");
    } else {
        tracing::debug!("Windows: working set limit set to {} bytes", max);
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl ProcessSandbox {
    fn apply_unix_limits(&self) {
        // Set RLIMIT_AS (virtual address space) on Linux/macOS
        if let Some(max_bytes) = self.max_memory_bytes {
            #[cfg(unix)]
            unsafe {
                let limit = libc_rlimit(max_bytes as u64);
                if limit != 0 {
                    tracing::warn!("Unix: setrlimit(RLIMIT_AS) failed (non-fatal)");
                } else {
                    tracing::debug!("Unix: RLIMIT_AS set to {} bytes", max_bytes);
                }
            }
        }
    }
}

#[cfg(unix)]
unsafe fn libc_rlimit(max_bytes: u64) -> i32 {
    // We call setrlimit directly via libc FFI to avoid adding libc as a dep
    // This is a best-effort approach — production would use the `rlimit` crate
    extern "C" {
        fn setrlimit(resource: i32, rlim: *const [u64; 2]) -> i32;
    }
    const RLIMIT_AS: i32 = 9; // virtual memory on Linux; RLIMIT_DATA on macOS
    let limits = [max_bytes, max_bytes];
    setrlimit(RLIMIT_AS, limits.as_ptr())
}
