# OpenBMS Monitor

Modern cross-platform monitor and configuration tool for the **Seplos BMS V2.0** (boards 10C/10E, module EMU1101, SH Energy ETECH firmware), as a replacement for the vendor's Windows-only BatteryMonitor.

Status: early development (skeleton). Protocol knowledge: [seplos-emu1101-docs](https://github.com/privatecoder/seplos-emu1101-docs).

## Layout

| Path | Content |
|---|---|
| `crates/openbms-proto` | protocol library: ASCII frames (0x42 telemetry, 0x47 parameters, 0x51 device info, 0x5A intra-pack), Modbus RTU (system values 0x1000) |
| `crates/openbms-transport` | serial (USB-RS485) and TCP (RS485-to-Ethernet gateway) transport |
| `crates/openbms-cli` | command line tool `openbms` |
| `app/` | desktop app: Tauri 2 + React + TypeScript + Tailwind |
| `data/` | cell database and JSON schema |
| `help/` | context help content (de/en) |

## Build

```sh
cargo test                      # protocol tests incl. captured frames
cargo run -p openbms-cli -- --tcp 192.168.1.10:4196 --bus can scan
cd app && npm install && npm run tauri dev
```

Targets: macOS (arm64/x64), Windows 11 (arm64/x64).

## License

MIT, see [LICENSE](LICENSE). Not affiliated with Seplos or SH Energy.
