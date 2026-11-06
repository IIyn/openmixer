//! Background process holding the tray icon.
//!
//! gpui quits as soon as its last window closes on Linux, so the window
//! cannot outlive itself. Instead this small process owns the tray icon and
//! starts the mixer window (`openmixer --window`) as a child process when
//! asked. It also makes OpenMixer single-instance: a second launch connects
//! to its socket and asks it to show the window.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use crossbeam_channel::{Sender, unbounded};

use crate::tray::{self, TrayAction};
use crate::ui;

const QUIT_GRACE: Duration = Duration::from_secs(3);

fn socket_path() -> PathBuf {
    dirs::runtime_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("openmixer.sock")
}

/// The mixer window, running in a child process.
struct Window {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Window {
    fn spawn(preview: bool) -> Result<Self> {
        let exe = std::env::current_exe().context("locating the openmixer binary")?;
        let mut cmd = Command::new(exe);
        cmd.arg("--window").stdin(Stdio::piped());
        if preview {
            cmd.arg("--preview");
        }
        let mut child = cmd.spawn().context("starting the mixer window")?;
        let stdin = child.stdin.take();
        Ok(Self { child, stdin })
    }

    fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Asks the window to come to the front; false if it is gone.
    fn raise(&mut self) -> bool {
        self.stdin
            .as_mut()
            .is_some_and(|s| writeln!(s, "show").and_then(|()| s.flush()).is_ok())
    }

    /// Closes stdin so the window saves and quits, then waits for it.
    fn quit(mut self) {
        drop(self.stdin.take());
        let deadline = std::time::Instant::now() + QUIT_GRACE;
        while std::time::Instant::now() < deadline {
            if !matches!(self.child.try_wait(), Ok(None)) {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        log::warn!("the mixer window did not quit in time, killing it");
        if let Err(e) = self.child.kill() {
            log::warn!("kill: {e}");
        }
        self.child.wait().ok();
    }
}

/// Forwards `show` requests from later launches to the tray loop.
fn serve_socket(listener: UnixListener, actions: Sender<TrayAction>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            continue;
        };
        let mut line = String::new();
        if BufReader::new(stream).read_line(&mut line).is_ok()
            && line.trim() == "show"
            && actions.send(TrayAction::Show).is_err()
        {
            return;
        }
    }
}

/// Entry point of a normal launch.
pub fn run(preview: bool) -> Result<()> {
    let socket = socket_path();
    // Already running: bring its window up and leave.
    if let Ok(mut stream) = UnixStream::connect(&socket) {
        writeln!(stream, "show").context("talking to the running OpenMixer")?;
        return Ok(());
    }
    // A socket file nobody listens on is left over from a crash.
    if socket.exists() {
        fs::remove_file(&socket).with_context(|| format!("removing {}", socket.display()))?;
    }

    let (tx, rx) = unbounded();
    let Some(_tray) = tray::spawn(tx.clone()) else {
        // No tray on this desktop: behave like a plain application.
        ui::run(preview, false);
        return Ok(());
    };

    match UnixListener::bind(&socket) {
        Ok(listener) => {
            let tx = tx.clone();
            thread::Builder::new()
                .name("openmixer-socket".into())
                .spawn(move || serve_socket(listener, tx))
                .context("starting the socket thread")?;
        }
        Err(e) => log::warn!("single-instance socket unavailable: {e}"),
    }

    let mut window = Some(Window::spawn(preview)?);
    for action in rx {
        match action {
            TrayAction::Show => {
                let raised = match window.as_mut() {
                    Some(w) => w.is_running() && w.raise(),
                    None => false,
                };
                if !raised {
                    window = Some(Window::spawn(false)?);
                }
            }
            TrayAction::Quit => break,
        }
    }
    if let Some(w) = window {
        w.quit();
    }
    if let Err(e) = fs::remove_file(&socket) {
        log::debug!("removing {}: {e}", socket.display());
    }
    Ok(())
}
