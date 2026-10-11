//! Standalone CLI. No monitor UI action can start a firmware write.

use clap::{Parser, Subcommand, ValueEnum};
use openbms_flash::{
    ehex, known,
    session::{self, Options},
    Error, Lang, Result,
};
use openbms_proto::frame;
use openbms_transport::{Connection, Endpoint};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

#[derive(Clone, Copy, ValueEnum)]
enum Language {
    De,
    En,
}
impl From<Language> for Lang {
    fn from(v: Language) -> Self {
        if matches!(v, Language::En) {
            Lang::En
        } else {
            Lang::De
        }
    }
}

#[derive(Parser)]
#[command(
    name = "openbms-flash",
    about = "EMU1101 firmware probe, validation and guarded update over the RS485 pack bus"
)]
struct Cli {
    /// USB-RS485 serial port (19200 baud, RS485-1/2)
    #[arg(long, conflicts_with = "tcp")]
    serial: Option<String>,
    /// TCP gateway connected to the 19200 baud pack bus
    #[arg(long)]
    tcp: Option<String>,
    #[arg(long, value_enum, default_value = "de")]
    lang: Language,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate an original vendor image and print every frame without opening a port
    DryRun {
        #[arg(long)]
        image: PathBuf,
        #[arg(long, default_value_t = 0)]
        address: u8,
    },
    /// Reset only, ask the bootloader for its packet size, and wait for the app to return
    Probe {
        #[arg(long, default_value_t = 0)]
        address: u8,
        #[arg(long)]
        log: PathBuf,
    },
    /// Write a known vendor image after an interactive confirmation
    Flash {
        #[arg(long, default_value_t = 0)]
        address: u8,
        #[arg(long)]
        image: PathBuf,
        #[arg(long)]
        log: PathBuf,
    },
}

fn open_log(path: &PathBuf) -> Result<File> {
    Ok(OpenOptions::new().write(true).create_new(true).open(path)?)
}
fn endpoint(cli: &Cli) -> Result<Endpoint> {
    match (&cli.serial, &cli.tcp) {
        (Some(path), None) => Ok(Endpoint::Serial {
            path: path.clone(),
            baud: 19_200,
        }),
        (None, Some(addr)) => Ok(Endpoint::Tcp { addr: addr.clone() }),
        _ => Err(Error::Proto(
            "choose --serial or --tcp on the 19200 baud RS485 pack bus".into(),
        )),
    }
}
fn image(path: &PathBuf) -> Result<(ehex::Image, &'static known::Release)> {
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_none_or(|e| !e.eq_ignore_ascii_case("ehex"))
    {
        return Err(Error::Image("expected a .ehex vendor file".into()));
    }
    let bytes = fs::read(path)?;
    let image = ehex::parse(&bytes)?;
    let release = known::identify(&image)?;
    Ok((image, release))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn run(cli: Cli) -> Result<()> {
    let lang: Lang = cli.lang.into();
    let options = Options::default();
    match &cli.command {
        Command::DryRun {
            image: path,
            address,
        } => {
            if *address > 15 {
                return Err(Error::Address(*address));
            }
            let (image, release) = image(path)?;
            let frames = session::dry_run(&image)?;
            let reset = frame::encode_request_version(0x26, *address, 0x45, &[1, 6, 0])?;
            println!(
                "{} | {} | {} bytes | SHA-256 code {}",
                release.name,
                image.header.device,
                image.code.len(),
                image.code_sha256
            );
            println!("RESET {}", hex(&reset));
            for (i, bytes) in frames.iter().enumerate() {
                println!("{:04} {}", i, hex(bytes));
            }
            println!(
                "{}",
                lang.pick(
                    "Nur Vorschau; nichts gesendet.",
                    "Dry run only; nothing sent."
                )
            );
        }
        Command::Probe { address, log } => {
            if *address > 15 {
                return Err(Error::Address(*address));
            }
            let mut link = Connection::open(&endpoint(&cli)?)?;
            let mut log_file = open_log(log)?;
            let result = session::probe(&mut link, *address, &mut log_file, options)?;
            println!(
                "{} {} | {} | {}: {} | {}",
                result.before.device_name,
                result.before.firmware_version,
                result.before.can_protocol,
                lang.pick("Paketgröße", "packet size"),
                result.packet_size,
                lang.pick("App wieder erreichbar", "application returned")
            );
        }
        Command::Flash {
            address,
            image: path,
            log,
        } => {
            if *address > 15 {
                return Err(Error::Address(*address));
            }
            let (image, release) = image(path)?;
            // Precompute all blocks before the first hardware command, including checksum guards.
            let frames = session::dry_run(&image)?;
            let mut link = Connection::open(&endpoint(&cli)?)?;
            let mut log_file = open_log(log)?;
            let before = session::inspect(&mut link, *address, &mut log_file, options)?;
            println!(
                "{}: {address} | {} {} | {} {} | SHA-256 {} | {} {}",
                lang.pick("Ziel", "Target"),
                before.device_name,
                before.firmware_version,
                lang.pick("Image", "Image"),
                release.name,
                image.code_sha256,
                lang.pick("Blöcke", "blocks"),
                frames.len() - 3
            );
            println!("{}", lang.pick("Ab W F E kann die App fehlen. Block-Timeouts werden nicht wiederholt.",
                "After W F E the application may be absent. Timed-out blocks are never retried."));
            print!(
                "{} FLASH {address}: ",
                lang.pick("Zur Bestätigung eingeben", "Type to confirm")
            );
            io::stdout().flush()?;
            let mut answer = String::new();
            io::stdin().lock().read_line(&mut answer)?;
            if answer.trim() != format!("FLASH {address}") {
                return Err(Error::Cancelled);
            }
            writeln!(
                log_file,
                "CONFIRMED target={address} code_sha256={}",
                image.code_sha256
            )?;
            let result = session::flash(&mut link, *address, &image, &mut log_file, options)?;
            println!(
                "{}: {} {} | {}",
                lang.pick("Fertig", "Complete"),
                result.blocks,
                lang.pick("Blöcke", "blocks"),
                lang.pick(
                    "App antwortet auf 0x51/0x42/0x44",
                    "app answered 0x51/0x42/0x44"
                )
            );
        }
    }
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    let lang: Lang = cli.lang.into();
    if let Err(e) = run(cli) {
        eprintln!("{}: {}", lang.pick("Fehler", "Error"), e.text(lang));
        std::process::exit(1);
    }
}
