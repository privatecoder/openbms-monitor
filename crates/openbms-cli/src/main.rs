use clap::{Parser, Subcommand, ValueEnum};
use openbms_transport::{list_serial_ports, Bus, Connection, Endpoint};

#[derive(Parser)]
#[command(name = "openbms", about = "Read Seplos BMS V2.0 packs over RS485 (serial or TCP gateway)")]
struct Cli {
    /// Serial port, e.g. /dev/tty.usbserial-1234 or COM3
    #[arg(long, conflicts_with = "tcp")]
    serial: Option<String>,
    /// RS485-to-Ethernet gateway, host:port
    #[arg(long)]
    tcp: Option<String>,
    /// Bus the adapter is connected to (sets the baud rate)
    #[arg(long, value_enum, default_value_t = BusArg::Can)]
    bus: BusArg,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum BusArg {
    /// RS485-1/2 sockets, 19200 baud
    Pack,
    /// RS485 pins of the CAN socket, 9600 baud
    Can,
}

#[derive(Subcommand)]
enum Cmd {
    /// List serial ports
    Ports,
    /// Find packs (addresses 0..15)
    Scan,
    /// Telemetry (0x42) of one pack
    Telemetry { #[arg(default_value_t = 0)] address: u8 },
    /// Device info (0x51) of one pack
    Info { #[arg(default_value_t = 0)] address: u8 },
    /// All parameters (0x47), pack bus only
    Params { #[arg(default_value_t = 0)] address: u8 },
    /// System values of the master (Modbus 0x1000), CAN socket bus only
    System,
}

fn main() {
    let cli = Cli::parse();
    if let Cmd::Ports = cli.cmd {
        for p in list_serial_ports() {
            println!("{p}");
        }
        return;
    }
    let bus = match cli.bus {
        BusArg::Pack => Bus::PackBus,
        BusArg::Can => Bus::CanSocketBus,
    };
    let endpoint = match (cli.serial, cli.tcp) {
        (Some(path), _) => Endpoint::Serial { path, baud: bus.baud() },
        (None, Some(addr)) => Endpoint::Tcp { addr },
        _ => {
            eprintln!("either --serial or --tcp is required");
            std::process::exit(2);
        }
    };
    let mut c = match Connection::open(&endpoint) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cannot open connection: {e}");
            std::process::exit(1);
        }
    };
    let out = match cli.cmd {
        Cmd::Ports => unreachable!(),
        Cmd::Scan => Ok(serde_json::to_value(c.scan(0..=15).into_iter().map(|(a, d)| serde_json::json!({"address": a, "device": d})).collect::<Vec<_>>()).unwrap()),
        Cmd::Telemetry { address } => c.telemetry(address).map(|v| serde_json::to_value(v).unwrap()),
        Cmd::Info { address } => c.device_info(address).map(|v| serde_json::to_value(v).unwrap()),
        Cmd::Params { address } => c.parameters(address).map(|v| serde_json::to_value(v).unwrap()),
        Cmd::System => c.system_values().map(|v| serde_json::to_value(v).unwrap()),
    };
    match out {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap()),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
