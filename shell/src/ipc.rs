//! The launcher's IPC socket: `qsflow-shell {open|close|toggle|status}` is the
//! client (and what the `Alt+Space` keybind spawns), the daemon owns the
//! listener.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::Duration;

use futures::channel::mpsc;
use futures::stream::{self, Stream, StreamExt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcCommand {
    Open,
    Close,
    Toggle,
}

/// Both sides give up after this: the daemon so a silent client cannot wedge the
/// accept thread, the client so a wedged daemon cannot hang the keybind.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(2);

/// Whether a surface is up, answered by the accept thread for `status`.
pub static VISIBLE: AtomicBool = AtomicBool::new(false);

static LISTENER: LazyLock<Mutex<Option<UnixListener>>> = LazyLock::new(Default::default);
/// `(sender, receiver-slot)`: the accept thread shares the sender, the app's
/// subscription takes the receiver exactly once.
type Mailbox<T> = (
    mpsc::UnboundedSender<T>,
    Mutex<Option<mpsc::UnboundedReceiver<T>>>,
);

static INBOX: LazyLock<Mailbox<IpcCommand>> = LazyLock::new(|| {
    let (tx, rx) = mpsc::unbounded();
    (tx, Mutex::new(Some(rx)))
});

/// `$XDG_RUNTIME_DIR/qsflow-shell.sock`, falling back to a per-user `/tmp` path
/// when the session has no runtime dir.
pub fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("qsflow-shell.sock"),
        None => PathBuf::from(format!("/tmp/qsflow-shell-{}.sock", uid())),
    }
}

fn uid() -> u32 {
    // `/proc/self` is owned by this process's user.
    std::fs::metadata("/proc/self")
        .map(|meta| meta.uid())
        .unwrap_or(0)
}

/// Take the IPC socket. Binds *before* Wayland is touched, so a second instance
/// exits without ever creating a surface.
pub fn bind() -> Result<(), String> {
    let path = socket_path();
    if UnixStream::connect(&path).is_ok() {
        return Err("another qsflow-shell is running".to_string());
    }

    // no live listener answered: a leftover socket file
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)
        .map_err(|error| format!("cannot bind {}: {error}", path.display()))?;
    // `bind` honours the umask; the launcher's IPC is owner-only
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    *LISTENER.lock().unwrap_or_else(PoisonError::into_inner) = Some(listener);

    Ok(())
}

/// Incoming commands. Called once per process by the subscription recipe.
pub fn stream() -> impl Stream<Item = IpcCommand> {
    let taken = INBOX
        .1
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    match taken {
        Some(receiver) => {
            spawn_accept_thread();
            receiver.boxed()
        }
        None => stream::empty().boxed(),
    }
}

fn spawn_accept_thread() {
    let listener = LISTENER
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    let Some(listener) = listener else {
        eprintln!("qsflow-shell: IPC socket was never bound");
        return;
    };

    std::thread::spawn(move || {
        loop {
            match listener.accept() {
                // One thread per connection: a client that connects and never
                // writes (or is killed mid-request) must not stall a later
                // `toggle` from the keybind.
                Ok((stream, _)) => {
                    std::thread::spawn(move || serve(stream));
                }
                Err(error) => {
                    eprintln!("qsflow-shell: IPC accept failed: {error}");
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    });
}

/// One line in, one line back, then the connection closes.
fn serve(mut stream: UnixStream) {
    let _ = stream.set_read_timeout(Some(CLIENT_TIMEOUT));

    let Ok(reader) = stream.try_clone() else {
        return;
    };
    let mut line = String::new();
    if BufReader::new(reader).read_line(&mut line).is_err() {
        return;
    }

    let reply = match line.trim() {
        "open" => {
            command(IpcCommand::Open);
            "ok"
        }
        "close" => {
            command(IpcCommand::Close);
            "ok"
        }
        "toggle" => {
            command(IpcCommand::Toggle);
            "ok"
        }
        "status" => visible(),
        _ => "error",
    };

    let _ = stream.write_all(format!("{reply}\n").as_bytes());
}

fn command(command: IpcCommand) {
    let _ = INBOX.0.unbounded_send(command);
}

/// `status` is answered by the accept thread, so it reads the flag the app
/// maintains rather than the app's state.
pub fn visible() -> &'static str {
    if VISIBLE.load(Ordering::Relaxed) {
        "visible"
    } else {
        "hidden"
    }
}

/// The client half: one request line, one reply line, printed. Returns the
/// process exit code.
pub fn client(command: &str) -> i32 {
    let path = socket_path();
    let Ok(mut stream) = UnixStream::connect(&path) else {
        eprintln!(
            "qsflow-shell: no running instance at {} (start qsflow-launcher.service)",
            path.display()
        );
        return 1;
    };

    if stream
        .write_all(format!("{command}\n").as_bytes())
        .and_then(|()| stream.flush())
        .is_err()
    {
        eprintln!("qsflow-shell: cannot talk to the running instance");
        return 1;
    }

    let mut reply = String::new();
    let _ = stream.set_read_timeout(Some(CLIENT_TIMEOUT));
    match BufReader::new(stream).read_line(&mut reply) {
        Ok(0) | Err(_) => {
            eprintln!("qsflow-shell: no reply from the running instance");
            1
        }
        Ok(_) => {
            let reply = reply.trim();
            if reply == "error" {
                eprintln!(
                    "qsflow-shell: unknown command ({}); expected open, close, toggle or status",
                    command
                );
                return 1;
            }
            println!("{reply}");
            0
        }
    }
}
