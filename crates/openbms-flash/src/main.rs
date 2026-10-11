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
use std::time::Duration;

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
    /// Extra wait after each complete bootloader reply (0..1000 ms; default 0)
    #[arg(long, global = true, default_value_t = 0, value_parser = clap::value_parser!(u16).range(0..=1000))]
    frame_gap_ms: u16,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate an image and print every frame without opening a port
    DryRun {
        #[arg(long)]
        image: PathBuf,
        #[arg(long, default_value_t = 0)]
        address: u8,
        /// Permit a valid EMU1101 .ehex image whose code hash is not in the vendor allowlist
        #[arg(long)]
        allow_custom_firmware: bool,
    },
    /// Reset only, ask the bootloader for its packet size, and wait for the app to return
    Probe {
        #[arg(long, default_value_t = 0)]
        address: u8,
        #[arg(long)]
        log: PathBuf,
    },
    /// Write an image after an interactive confirmation
    Flash {
        #[arg(long, default_value_t = 0)]
        address: u8,
        #[arg(long)]
        image: PathBuf,
        #[arg(long)]
        log: PathBuf,
        /// Permit a valid EMU1101 .ehex image whose code hash is not in the vendor allowlist
        #[arg(long)]
        allow_custom_firmware: bool,
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
fn select_release(
    image: &ehex::Image,
    allow_custom: bool,
) -> Result<Option<&'static known::Release>> {
    match known::identify(image) {
        Ok(release) => Ok(Some(release)),
        Err(_) if allow_custom => Ok(None),
        Err(error) => Err(error),
    }
}

fn confirmation_phrase(address: u8, custom: bool) -> String {
    if custom {
        format!("FLASH CUSTOM {address}")
    } else {
        format!("FLASH {address}")
    }
}

fn image(
    path: &PathBuf,
    allow_custom: bool,
) -> Result<(ehex::Image, Option<&'static known::Release>)> {
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_none_or(|e| !e.eq_ignore_ascii_case("ehex"))
    {
        return Err(Error::Image("expected a .ehex file".into()));
    }
    let bytes = fs::read(path)?;
    let image = ehex::parse(&bytes)?;
    let release = select_release(&image, allow_custom)?;
    Ok((image, release))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn run(cli: Cli) -> Result<()> {
    let lang: Lang = cli.lang.into();
    let options = Options {
        min_frame_gap: Duration::from_millis(u64::from(cli.frame_gap_ms)),
        ..Options::default()
    };
    match &cli.command {
        Command::DryRun {
            image: path,
            address,
            allow_custom_firmware,
        } => {
            if *address > 15 {
                return Err(Error::Address(*address));
            }
            let (image, release) = image(path, *allow_custom_firmware)?;
            let frames = session::dry_run(&image)?;
            let reset = frame::encode_request_version(0x26, *address, 0x45, &[1, 6, 0])?;
            println!(
                "{} | {} | {} bytes | SHA-256 code {}",
                release.map_or(
                    lang.pick(
                        "EIGENE FIRMWARE (unbekannter Hash)",
                        "CUSTOM (unknown hash)"
                    ),
                    |r| r.name
                ),
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
            allow_custom_firmware,
        } => {
            if *address > 15 {
                return Err(Error::Address(*address));
            }
            let (image, release) = image(path, *allow_custom_firmware)?;
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
                release.map_or(
                    lang.pick(
                        "EIGENE FIRMWARE (unbekannter Hash)",
                        "CUSTOM (unknown hash)"
                    ),
                    |r| r.name
                ),
                image.code_sha256,
                lang.pick("Blöcke", "blocks"),
                frames.len() - 3
            );
            println!("{}", lang.pick("Ab W F E kann die App fehlen. Block-Timeouts werden nicht wiederholt.",
                "After W F E the application may be absent. Timed-out blocks are never retried."));
            let expected = confirmation_phrase(*address, release.is_none());
            print!(
                "{} {expected}: ",
                lang.pick("Zur Bestätigung eingeben", "Type to confirm")
            );
            io::stdout().flush()?;
            let mut answer = String::new();
            io::stdin().lock().read_line(&mut answer)?;
            if answer.trim() != expected {
                return Err(Error::Cancelled);
            }
            writeln!(
                log_file,
                "CONFIRMED target={address} source={} code_sha256={}",
                if release.is_some() {
                    "vendor-allowlist"
                } else {
                    "custom-override"
                },
                image.code_sha256
            )?;
            let result = session::flash(&mut link, *address, &image, &mut log_file, options)?;
            println!(
                "{}: {} {} | {} | {} {}",
                lang.pick("Fertig", "Complete"),
                result.blocks,
                lang.pick("Blöcke", "blocks"),
                lang.pick(
                    "App antwortet auf 0x51/0x42/0x44",
                    "app answered 0x51/0x42/0x44"
                ),
                lang.pick("Version jetzt", "version now"),
                result.after.firmware_version
            );
            println!(
                "W F D {} ({}): {:.1} / {:.1} / {:.1} ms",
                lang.pick("Antwortzeit", "response time"),
                lang.pick("Min/Median/Max", "min/median/max"),
                result.wfd_reply_times.min.as_secs_f64() * 1000.0,
                result.wfd_reply_times.median.as_secs_f64() * 1000.0,
                result.wfd_reply_times.max.as_secs_f64() * 1000.0,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_firmware_flag_is_only_available_for_image_commands() {
        let dry = Cli::try_parse_from([
            "openbms-flash",
            "dry-run",
            "--image",
            "custom.ehex",
            "--allow-custom-firmware",
        ])
        .unwrap();
        assert!(matches!(
            dry.command,
            Command::DryRun {
                allow_custom_firmware: true,
                ..
            }
        ));
        let flash = Cli::try_parse_from([
            "openbms-flash",
            "flash",
            "--image",
            "custom.ehex",
            "--log",
            "flash.log",
            "--allow-custom-firmware",
        ])
        .unwrap();
        assert!(matches!(
            flash.command,
            Command::Flash {
                allow_custom_firmware: true,
                ..
            }
        ));
        assert!(Cli::try_parse_from([
            "openbms-flash",
            "probe",
            "--log",
            "probe.log",
            "--allow-custom-firmware",
        ])
        .is_err());
    }

    #[test]
    fn unknown_code_requires_override_and_custom_confirmation() {
        let custom = ehex::Image {
            header: ehex::Header {
                mcu: "SWM181RCT6_64_M0".into(),
                device: "EMU1101_10D_HW16".into(),
                interface: "RS485Y19200N9600".into(),
                raw: vec![0x55; 256],
            },
            code: vec![0; 16],
            start: [0, 0, 0, 0xc1],
            code_sha256: "unknown-code-hash".into(),
            file_sha256: String::new(),
            stack_top: 0x2000_1000,
            reset_vector: 0xc1,
        };
        assert!(select_release(&custom, false).is_err());
        assert!(select_release(&custom, true).unwrap().is_none());
        assert_eq!(confirmation_phrase(0, true), "FLASH CUSTOM 0");
        assert_eq!(confirmation_phrase(0, false), "FLASH 0");
    }

    #[test]
    fn frame_gap_cli_defaults_to_zero_and_rejects_over_1000() {
        let default =
            Cli::try_parse_from(["openbms-flash", "probe", "--log", "probe.log"]).unwrap();
        assert_eq!(default.frame_gap_ms, 0);
        let set = Cli::try_parse_from([
            "openbms-flash",
            "probe",
            "--log",
            "probe.log",
            "--frame-gap-ms",
            "1000",
        ])
        .unwrap();
        assert_eq!(set.frame_gap_ms, 1000);
        assert!(Cli::try_parse_from([
            "openbms-flash",
            "--frame-gap-ms",
            "1001",
            "probe",
            "--log",
            "probe.log"
        ])
        .is_err());
    }
}
