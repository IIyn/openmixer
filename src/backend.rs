//! USB worker thread: owns the device, coalesces writes, polls meters and
//! the monitor knob, and reconnects when the interface is replugged.

use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};

use crate::device::protocol::{self, Request};
use crate::device::{DeviceError, IdDevice, Profile};

const POLL_INTERVAL: Duration = Duration::from_millis(33);
const MONITOR_INTERVAL: Duration = Duration::from_millis(100);
const RECONNECT_INTERVAL: Duration = Duration::from_secs(2);

pub enum Command {
    Write(Vec<Request>),
}

pub enum Event {
    Connected {
        profile: &'static Profile,
        interface: u8,
    },
    Disconnected(String),
    Meters(Vec<f32>),
    /// Monitor level read back from the device (0..1).
    MonitorLevel(f32),
    MonitorDim(bool),
    WriteError(String),
}

pub struct Backend {
    pub commands: Sender<Command>,
    pub events: Receiver<Event>,
}

impl Backend {
    pub fn spawn() -> std::io::Result<Self> {
        let (cmd_tx, cmd_rx) = unbounded();
        let (ev_tx, ev_rx) = unbounded();
        thread::Builder::new()
            .name("openmixer-usb".into())
            .spawn(move || Worker::new(cmd_rx, ev_tx).run())?;
        Ok(Self {
            commands: cmd_tx,
            events: ev_rx,
        })
    }

    pub fn write(&self, requests: Vec<Request>) {
        if requests.is_empty() {
            return;
        }
        if self.commands.send(Command::Write(requests)).is_err() {
            log::error!("the USB thread has stopped");
        }
    }
}

struct Worker {
    commands: Receiver<Command>,
    events: Sender<Event>,
    device: Option<IdDevice>,
    last_error: Option<String>,
}

impl Worker {
    fn new(commands: Receiver<Command>, events: Sender<Event>) -> Self {
        Self {
            commands,
            events,
            device: None,
            last_error: None,
        }
    }

    fn emit(&self, event: Event) -> bool {
        self.events.send(event).is_ok()
    }

    fn run(mut self) {
        let mut next_meter = Instant::now();
        let mut next_monitor = Instant::now();
        loop {
            if self.device.is_none() && !self.connect() {
                // Drop writes queued while offline; state is pushed on connect.
                match self.commands.recv_timeout(RECONNECT_INTERVAL) {
                    Ok(_) | Err(RecvTimeoutError::Timeout) => {
                        while self.commands.try_recv().is_ok() {}
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }

            let wait = next_meter.saturating_duration_since(Instant::now());
            let mut pending: Vec<Request> = Vec::new();
            match self.commands.recv_timeout(wait) {
                Ok(cmd) => self.queue(&mut pending, cmd),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            while let Ok(cmd) = self.commands.try_recv() {
                self.queue(&mut pending, cmd);
            }
            if !pending.is_empty() {
                self.flush(&pending);
            }

            let now = Instant::now();
            if now >= next_meter {
                next_meter = now + POLL_INTERVAL;
                self.poll_meters();
            }
            if now >= next_monitor {
                next_monitor = now + MONITOR_INTERVAL;
                self.poll_monitor();
            }
        }
    }

    fn connect(&mut self) -> bool {
        match IdDevice::open() {
            Ok(dev) => {
                let profile = dev.profile();
                let interface = dev.interface();
                self.device = Some(dev);
                self.last_error = None;
                self.emit(Event::Connected { profile, interface })
            }
            Err(e) => {
                let msg = e.to_string();
                if self.last_error.as_deref() != Some(msg.as_str()) {
                    log::warn!("{msg}");
                    self.emit(Event::Disconnected(msg.clone()));
                    self.last_error = Some(msg);
                }
                false
            }
        }
    }

    /// Keeps only the latest value per control, in first-seen order, so a
    /// fader drag does not queue hundreds of stale transfers.
    fn queue(&self, pending: &mut Vec<Request>, cmd: Command) {
        let Command::Write(reqs) = cmd;
        for req in reqs {
            match pending.iter_mut().find(|p| p.key() == req.key()) {
                Some(slot) => *slot = req,
                None => pending.push(req),
            }
        }
    }

    fn flush(&mut self, pending: &[Request]) {
        let Some(dev) = &self.device else {
            return;
        };
        if !dev.profile().writable() {
            return;
        }
        for req in pending {
            if let Err(e) = dev.send(req) {
                self.fail(e, "write");
                return;
            }
        }
    }

    fn poll_meters(&mut self) {
        let Some(dev) = &self.device else {
            return;
        };
        match dev.meter_block() {
            Ok(block) => {
                let nodes = dev.profile().nodes.len().min(protocol::METER_MAX_NODES);
                self.emit(Event::Meters(protocol::decode_meters(&block, nodes)));
            }
            Err(e) => self.fail(e, "meters"),
        }
    }

    fn poll_monitor(&mut self) {
        let Some(dev) = &self.device else {
            return;
        };
        if !dev.profile().protocol_verified {
            return;
        }
        match dev.monitor_volume() {
            Ok(raw) => {
                self.emit(Event::MonitorLevel(protocol::raw_to_level(raw)));
            }
            Err(e) => return self.fail(e, "monitor volume"),
        }
        if let Ok(dim) = dev.monitor_flag(protocol::MonitorSwitch::Dim.selector()) {
            self.emit(Event::MonitorDim(dim));
        }
    }

    fn fail(&mut self, e: DeviceError, what: &str) {
        if e.is_fatal() {
            self.device = None;
            let msg = e.to_string();
            self.emit(Event::Disconnected(msg.clone()));
            self.last_error = Some(msg);
        } else {
            log::debug!("{what}: {e}");
            if what == "write" {
                self.emit(Event::WriteError(format!("{what} failed: {e}")));
            }
        }
    }
}
