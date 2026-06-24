//! Single-instance enforcement via IPC socket.
//!
//! The socket serves dual purpose:
//! 1. Acts as a lock — if bind fails, another instance is running.
//! 2. IPC channel — second instance sends "focus" to bring the first to front.
//!
//! Platform strategy:
//! - Linux/macOS/FreeBSD: Unix domain socket
//! - Windows: TCP socket on localhost with persistent port file

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
mod unix_impl {
    use std::io::{Read, Write};
    use std::net::Shutdown;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::mpsc::{self, Receiver};
    use std::thread;

    use crate::platform;

    const SOCKET_NAME: &str = "zenix.sock";

    fn socket_path() -> PathBuf {
        platform::zenix_config_dir().join(SOCKET_NAME)
    }

    pub fn try_acquire() -> Result<Receiver<super::Signal>, String> {
        let socket_path = socket_path();

        // Step 1: Try to connect to an existing socket
        if socket_path.exists() {
            match UnixStream::connect(&socket_path) {
                Ok(mut stream) => {
                    if stream.write_all(b"focus").is_ok() {
                        stream.shutdown(Shutdown::Both).ok();
                    }
                    return Err(format!(
                        "another zenix instance is already running (socket: {})",
                        socket_path.display()
                    ));
                }
                Err(_) => {
                    // Stale socket from a crash
                    std::fs::remove_file(&socket_path).ok();
                }
            }
        }

        // Step 2: Bind — this is the lock
        let listener = match UnixListener::bind(&socket_path) {
            Ok(l) => l,
            Err(e) => {
                // Race condition — try one more time to connect
                if let Ok(mut stream) = UnixStream::connect(&socket_path) {
                    if stream.write_all(b"focus").is_ok() {
                        stream.shutdown(Shutdown::Both).ok();
                    }
                }
                return Err(format!(
                    "another zenix instance is already running (socket: {}): {}",
                    socket_path.display(),
                    e
                ));
            }
        };

        let (tx, rx) = mpsc::channel::<super::Signal>();

        thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(mut stream) => {
                        let mut buf = [0u8; 64];
                        match stream.read(&mut buf) {
                            Ok(n) => {
                                let msg = String::from_utf8_lossy(&buf[..n]);
                                if msg.trim() == "focus" {
                                    tx.send(super::Signal::Focus).ok();
                                }
                            }
                            Err(e) => eprintln!("zenix: failed to read from socket: {}", e),
                        }
                    }
                    Err(e) => eprintln!("zenix: failed to accept socket connection: {}", e),
                }
            }
        });

        Ok(rx)
    }

    pub fn release() {
        let socket_path = socket_path();
        std::fs::remove_file(&socket_path).ok();
    }
}

#[cfg(windows)]
mod windows_impl {
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::mpsc::{self, Receiver};
    use std::thread;

    use crate::platform;

    const LOCALHOST: &str = "127.0.0.1";
    const BASE_PORT: u16 = 44238;
    const PORT_FILE: &str = "zenix.port";

    /// Path to the persistent port file.
    fn port_file_path() -> PathBuf {
        platform::zenix_config_dir().join(PORT_FILE)
    }

    /// Read the port from the persistent file, or generate a new one.
    fn read_port() -> Option<u16> {
        let path = port_file_path();
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                return content.trim().parse().ok();
            }
        }
        None
    }

    /// Write the port to the persistent file.
    fn write_port(port: u16) {
        let path = port_file_path();
        std::fs::write(&path, port.to_string()).ok();
    }

    pub fn try_acquire() -> Result<Receiver<super::Signal>, String> {
        // Try the persisted port first
        let port = read_port().unwrap_or(BASE_PORT);
        let addr = format!("{}:{}", LOCALHOST, port);

        // Step 1: Try to connect to an existing listener
        match TcpStream::connect(&addr) {
            Ok(mut stream) => {
                if stream.write_all(b"focus").is_ok() {
                    stream.shutdown(Shutdown::Both).ok();
                }
                return Err(format!(
                    "another zenix instance is already running (port: {})",
                    port
                ));
            }
            Err(_) => {
                // No one listening — proceed to bind
            }
        }

        // Step 2: Bind — this is the lock
        let listener = match TcpListener::bind(&addr) {
            Ok(l) => l,
            Err(e) => {
                // Try one more time to connect (race condition)
                if let Ok(mut stream) = TcpStream::connect(&addr) {
                    if stream.write_all(b"focus").is_ok() {
                        stream.shutdown(Shutdown::Both).ok();
                    }
                }
                return Err(format!(
                    "another zenix instance is already running (port: {}): {}",
                    port, e
                ));
            }
        };

        // Persist the port for future instances
        write_port(port);

        let (tx, rx) = mpsc::channel::<super::Signal>();

        thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(mut stream) => {
                        let mut buf = [0u8; 64];
                        match stream.read(&mut buf) {
                            Ok(n) => {
                                let msg = String::from_utf8_lossy(&buf[..n]);
                                if msg.trim() == "focus" {
                                    tx.send(super::Signal::Focus).ok();
                                }
                            }
                            Err(e) => eprintln!("zenix: failed to read from socket: {}", e),
                        }
                    }
                    Err(e) => eprintln!("zenix: failed to accept socket connection: {}", e),
                }
            }
        });

        Ok(rx)
    }

    pub fn release() {
        // TCP socket is cleaned up when the listener is dropped.
        // Clean up the port file.
        let path = port_file_path();
        std::fs::remove_file(&path).ok();
    }
}

/// Signals received from other zenix instances.
pub enum Signal {
    /// Another instance launched — bring this window to front.
    Focus,
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
pub use unix_impl::{release, try_acquire};

#[cfg(windows)]
pub use windows_impl::{release, try_acquire};
