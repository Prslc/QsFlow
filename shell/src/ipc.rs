//! The unix socket behind `qsflow-shell open|close|toggle|status`, which is also
//! the single-instance guard: a live listener means another shell owns the
//! surface, so a second daemon refuses to start.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug)]
pub enum Command {
    Open,
    Close,
    Toggle,
    /// `status` answers from the event loop, so the reply travels back.
    Status(mpsc::Sender<String>),
}

pub type CommandSender = calloop::channel::Sender<Command>;

pub fn socket_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
    base.join("qsflow-shell.sock")
}

fn send_verb(verb: &str) -> std::io::Result<String> {
    let mut stream = UnixStream::connect(socket_path())?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.write_all(verb.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply)?;
    Ok(reply.trim().to_owned())
}

/// Runs in the client process: `status` prints the daemon's answer, the other
/// verbs are silent unless the daemon refused them.
pub fn client(verb: &str) -> std::io::Result<()> {
    let reply = send_verb(verb)?;
    if verb == "status" {
        println!("{reply}");
    } else if reply != "ok" {
        eprintln!("{reply}");
        std::process::exit(1);
    }
    Ok(())
}

/// Binds the socket and answers every verb with `ok`; `status` waits for the
/// event loop, which is the only place that knows whether a surface is up.
pub fn serve(tx: CommandSender) -> std::io::Result<()> {
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    // `bind` honours the umask, which can leave the launcher toggleable by every
    // local user.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let tx = tx.clone();
            // One thread per connection: a client that connects and never
            // writes must not hold the accept loop.
            std::thread::spawn(move || {
                let _ = answer(stream, tx);
            });
        }
    });
    Ok(())
}

fn answer(mut stream: UnixStream, tx: CommandSender) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut verb = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut verb)?;
    let (command, reply_waiter) = match verb.trim() {
        "open" => (Command::Open, None),
        "close" => (Command::Close, None),
        "toggle" => (Command::Toggle, None),
        "status" => {
            let (reply_tx, reply_rx) = mpsc::channel();
            (Command::Status(reply_tx), Some(reply_rx))
        }
        other => {
            stream.write_all(format!("unknown verb: {other}\n").as_bytes())?;
            stream.flush()?;
            return Ok(());
        }
    };
    let _ = tx.send(command);
    let reply = match reply_waiter {
        Some(rx) => rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or_else(|_| "unknown".into()),
        None => "ok".to_owned(),
    };
    stream.write_all(reply.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()
}
