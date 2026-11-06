//! OpenMixer: a Linux control panel for Audient iD audio interfaces.

mod backend;
mod config;
mod daemon;
mod device;
mod linux_fixes;
mod mixer;
mod tray;
mod ui;

use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use device::{IdDevice, protocol};

#[derive(Parser)]
#[command(version, about = "Mixer for Audient iD audio interfaces on Linux")]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
    /// Show a preview of the mixer (iD14 MKII) while no interface is connected.
    #[arg(long)]
    preview: bool,
    /// Internal: run only the mixer window (started by the tray process).
    #[arg(long, hide = true)]
    window: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open the interface and print what it reports (read-only).
    Probe,
    /// Install (or remove with --remove) the WirePlumber left/right balance fix.
    FixBalance {
        #[arg(long)]
        remove: bool,
    },
    /// Print the udev rule needed to access the interface.
    UdevRule,
}

fn probe() -> Result<()> {
    let dev = IdDevice::open().context("opening the interface")?;
    let p = dev.profile();
    println!(
        "{} (2708:{:04x}) — control through interface {}",
        p.name,
        p.product_id,
        dev.interface()
    );
    println!(
        "protocol verified: {} · mixer known: {}",
        p.protocol_verified, p.mixer_known
    );
    match dev.monitor_volume() {
        Ok(raw) => println!(
            "monitor volume: {} dB",
            mixer::format_db(protocol::raw_to_db(raw))
        ),
        Err(e) => println!("monitor volume: {e}"),
    }
    let nodes = p.nodes.len().min(protocol::METER_MAX_NODES);
    for _ in 0..10 {
        match dev.meter_block() {
            Ok(block) => {
                let line: Vec<String> = protocol::decode_meters(&block, nodes)
                    .iter()
                    .zip(p.nodes)
                    .map(|(l, n)| format!("{}={:.2}", n.name, l))
                    .collect();
                println!("{}", line.join(" "));
            }
            Err(e) => println!("meters: {e}"),
        }
        thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    match cli.command {
        Some(Cmd::Probe) => probe(),
        Some(Cmd::FixBalance { remove: false }) => {
            linux_fixes::install_soft_mixer()?;
            println!("Fix installed, WirePlumber restarted.");
            Ok(())
        }
        Some(Cmd::FixBalance { remove: true }) => {
            linux_fixes::remove_soft_mixer()?;
            println!("Fix removed, WirePlumber restarted.");
            Ok(())
        }
        Some(Cmd::UdevRule) => {
            print!("{}", linux_fixes::UDEV_RULE);
            eprintln!("\nInstall with:\n  {}", linux_fixes::UDEV_INSTALL_COMMAND);
            Ok(())
        }
        None => {
            if cli.window {
                ui::run(cli.preview, true);
                Ok(())
            } else {
                daemon::run(cli.preview)
            }
        }
    }
}
