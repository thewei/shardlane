//! Platform-local socket transport for the Herdr client boundary.
//!
//! Unix: filesystem Unix domain sockets via std. Windows: named pipes via
//! windows-sys (CreateFileW client + overlapped I/O so the RPC read timeout
//! keeps its "a hung call must fail" invariant). The pipe name mirrors
//! Herdr's config-dir layout (`\\.\pipe\herdr\` + the relative socket path)
//! and `HERDR_SOCKET_PATH` stays authoritative (Herdr socket-api resolution
//! order). The exact default pipe convention is verified against live herdr
//! by the Windows release job; a mismatch is a one-constant fix here.
//!
//! [INPUT]: std UnixStream (unix), windows-sys pipe/file APIs (windows), and
//! the session naming helpers shared with herdr.rs
//! [OUTPUT]: exposes LocalSocketPath, LocalStream (Read/Write +
//! set_read_timeout), connect, wait, exists_ready
//! [POS]: shardlane-host 的传输层底座，herdr.rs 的唯一本地 socket 出口；
//!        平台差异收敛在此模块，上层只见 LocalSocketPath/LocalStream
//! [PROTOCOL]: Update this header on change, then check CLAUDE.md.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A platform-local socket address. On unix this is a filesystem path; on
/// Windows it is a named-pipe path (`\\.\pipe\...`) held in a PathBuf so
/// callers keep one type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalSocketPath {
    raw: PathBuf,
}

impl LocalSocketPath {
    pub fn new(raw: PathBuf) -> Self {
        Self { raw }
    }

    pub fn raw(&self) -> &Path {
        &self.raw
    }

    pub fn display(&self) -> std::path::Display<'_> {
        self.raw.display()
    }

    /// Whether a server is listening. Unix: the socket file exists. Windows:
    /// a pipe probe opens and closes the client end.
    pub fn exists_ready(&self) -> bool {
        #[cfg(unix)]
        {
            self.raw.exists()
        }
        #[cfg(windows)]
        {
            match open_pipe(&self.raw) {
                Ok(handle) => {
                    unsafe {
                        windows_sys::Win32::Foundation::CloseHandle(handle);
                    }
                    true
                }
                Err(_) => false,
            }
        }
    }
}

/// Wait for a server to bind the address: bounded retries at 100ms.
pub fn wait(path: &LocalSocketPath, attempts: u32) -> Result<(), String> {
    for _ in 0..attempts {
        if path.exists_ready() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "timed out waiting for herdr server at {}",
        path.display()
    ))
}

/// Connect to the platform-local socket.
pub fn connect(path: &LocalSocketPath) -> io::Result<LocalStream> {
    #[cfg(unix)]
    {
        Ok(LocalStream::Unix(std::os::unix::net::UnixStream::connect(
            path.raw(),
        )?))
    }
    #[cfg(windows)]
    {
        Ok(LocalStream::Windows(NamedPipeClient {
            handle: open_pipe(path.raw())?,
            read_timeout: Duration::from_secs(10),
        }))
    }
}

pub enum LocalStream {
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixStream),
    #[cfg(windows)]
    Windows(NamedPipeClient),
}

impl LocalStream {
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        match self {
            #[cfg(unix)]
            LocalStream::Unix(stream) => stream.set_read_timeout(timeout),
            #[cfg(windows)]
            LocalStream::Windows(client) => {
                if let Some(timeout) = timeout {
                    // The client-side mutex is single-threaded per stream in
                    // practice; interior mutability keeps Read(&self) honest.
                    // Safe: we never leak the lock across await points.
                    if let Ok(mut slot) = client.read_timeout.lock() {
                        *slot = timeout;
                    }
                }
                Ok(())
            }
        }
    }
}

impl std::io::Read for LocalStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            #[cfg(unix)]
            LocalStream::Unix(stream) => std::io::Read::read(stream, buf),
            #[cfg(windows)]
            LocalStream::Windows(client) => client.read(buf),
        }
    }
}

impl std::io::Write for LocalStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            #[cfg(unix)]
            LocalStream::Unix(stream) => std::io::Write::write(stream, buf),
            #[cfg(windows)]
            LocalStream::Windows(client) => client.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            #[cfg(unix)]
            LocalStream::Unix(stream) => std::io::Write::flush(stream),
            #[cfg(windows)]
            LocalStream::Windows(client) => client.flush(),
        }
    }
}

// --- Windows named-pipe client (overlapped I/O with bounded reads) ---

#[cfg(windows)]
mod pipe {
    use super::{io, Duration};
    use std::sync::Mutex;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE, HANDLE,
        INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};
    use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

    pub struct NamedPipeClient {
        pub(super) handle: HANDLE,
        pub(super) read_timeout: Mutex<Duration>,
    }

    unsafe impl Send for NamedPipeClient {}
    unsafe impl Sync for NamedPipeClient {}

    impl Drop for NamedPipeClient {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }

    /// Open the pipe client end with overlapped I/O enabled.
    pub(super) fn open_pipe(path: &std::path::Path) -> io::Result<HANDLE> {
        let wide: Vec<u16> = path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(handle)
    }

    fn last_error() -> io::Error {
        io::Error::last_os_error()
    }

    struct Overlapped {
        inner: OVERLAPPED,
        event: HANDLE,
    }

    impl Overlapped {
        fn new() -> io::Result<Self> {
            let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
            if event == std::ptr::null_mut() {
                return Err(last_error());
            }
            let mut inner: OVERLAPPED = unsafe { std::mem::zeroed() };
            inner.hEvent = event;
            Ok(Self { inner, event })
        }
    }

    impl Drop for Overlapped {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.event);
            }
        }
    }

    impl NamedPipeClient {
        pub(super) fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
            let timeout = self
                .read_timeout
                .lock()
                .map(|slot| *slot)
                .unwrap_or(Duration::from_secs(10));
            let handle = self.handle;
            let mut overlapped = Overlapped::new()?;
            let mut transferred: u32 = 0;
            let slice = buf.as_mut_ptr();
            let len = buf.len().try_into().unwrap_or(u32::MAX);
            let result = unsafe {
                ReadFile(
                    handle,
                    slice,
                    len,
                    std::ptr::null_mut(),
                    &mut overlapped.inner,
                )
            };
            if result == 0 {
                let error = last_error();
                if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
                    if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
                        return Ok(0);
                    }
                    return Err(error);
                }
            }
            let millis = timeout.as_millis().try_into().unwrap_or(INFINITE);
            let waited = unsafe { WaitForSingleObject(overlapped.event, millis) };
            if waited == WAIT_TIMEOUT {
                unsafe {
                    CancelIoEx(handle, &mut overlapped.inner);
                    let mut ignored: u32 = 0;
                    let _ = GetOverlappedResult(handle, &mut overlapped.inner, &mut ignored, 1);
                }
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "named pipe read timed out",
                ));
            }
            if waited != WAIT_OBJECT_0 {
                return Err(last_error());
            }
            let ok =
                unsafe { GetOverlappedResult(handle, &mut overlapped.inner, &mut transferred, 1) };
            if ok == 0 {
                let error = last_error();
                if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
                    return Ok(0);
                }
                return Err(error);
            }
            Ok(transferred as usize)
        }

        pub(super) fn write(&self, buf: &[u8]) -> io::Result<usize> {
            let handle = self.handle;
            let mut overlapped = Overlapped::new()?;
            let mut transferred: u32 = 0;
            let slice = buf.as_ptr();
            let len = buf.len().try_into().unwrap_or(u32::MAX);
            let result = unsafe {
                WriteFile(
                    handle,
                    slice,
                    len,
                    std::ptr::null_mut(),
                    &mut overlapped.inner,
                )
            };
            if result == 0 {
                let error = last_error();
                if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
                    return Err(error);
                }
            }
            let waited = unsafe { WaitForSingleObject(overlapped.event, INFINITE) };
            if waited != WAIT_OBJECT_0 {
                return Err(last_error());
            }
            let ok =
                unsafe { GetOverlappedResult(handle, &mut overlapped.inner, &mut transferred, 1) };
            if ok == 0 {
                return Err(last_error());
            }
            Ok(transferred as usize)
        }

        pub(super) fn flush(&self) -> io::Result<()> {
            Ok(())
        }
    }
}

#[cfg(windows)]
use pipe::{open_pipe, NamedPipeClient};
