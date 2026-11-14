//! Which action goes over which of the three UARTs, and under which conditions, in one place: the firmware stage
//! (original, or a supported EMU1101-EXT: Light 1.5.4 or Full 1.6.4 and later, `ExtInfo::supported`; its image flags say which
//! functions it has), the release and lock bits of Group0/Group7, the address, and whether the app implements the way
//! at all.
//!
//! - UART0: RS485-1/2, Seplos ASCII, 19200 Bd; the host needs a quiet bus (the master's polling stopped).
//! - UART1: the RS485 pair of the CAN socket, 9600 Bd: OEM ASCII reads, Modbus (master), the EMU1101-EXT binary service
//!   and its ASCII commands; addressed, also while the master polls.
//! - UART2: the display port (binary 55…AA, e.g. a display or a Bluetooth dongle): only the locally attached board, only
//!   with Group7 bit 3; the app does not speak it, it is listed so the UI can name it.
//!
//! The catalog follows the firmware analysis (UART write matrix, 16.06.13 and the EMU1101-EXT builds). Its rows are data;
//! `decide` evaluates one row for one pack's facts. Unknown stays unknown: a stored proof carries no image flags.

use crate::maint::{MarkerWord, CAP_CALIBRATION, CAP_CCL_STEP, CAP_ENERGY, CAP_HISTORY, CAP_SOC_STORE, SUPPORTED_15, SUPPORTED_16};
use serde::Serialize;

/// The patch line every supported build belongs to or follows (major, minor); which builds of it are supported decides
/// `ExtInfo::supported` (Light 1.5.4, Full 1.6.4 and later), a pack below it counts as without patch.
pub const EXT_MIN: [u8; 2] = [1, 5];
/// From this patch version on, image flag bit 2 announces the stored pack cutoff (Group0 bit 4/5).
pub const CUTOFF_MIN: [u8; 2] = [1, 6];

/// Group0 bits of the patched firmware.
pub const G0_WRITE: u8 = 0;
pub const G0_START: u8 = 2;
pub const G0_POWER_LOCK: u8 = 3;
pub const G0_EXTENDED_OFF: u8 = 4;
pub const G0_LOCK_STATE: u8 = 5;
/// Full (image flag bit 7): bit 6 the hard stop at a recharge pause while bit 7 is 0 (an explicit choice); bit 7
/// at 1 takes the build's default (image flag bit 0)
pub const G0_HARD_STOP: u8 = 6;
pub const G0_HARD_STOP_INHERIT: u8 = 7;
/// Group7 bits that gate a way.
pub const G7_DISPLAY: u8 = 3;
pub const G7_EXTENDED_DISPLAY: u8 = 4;
pub const G7_AUTO_ADDRESS: u8 = 5;

/// The stored pack cutoff of an identity with patch `patch` and image flag bit 2 `flag`: both needed.
pub fn pack_cutoff_of(patch: [u8; 2], flag: bool) -> bool {
    flag && patch >= CUTOFF_MIN
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Uart0,
    Uart1,
    Uart2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    ReadTelemetry,
    ReadStatus,
    ReadInfo,
    ReadParameters,
    WriteParameters,
    WriteName,
    ReadSerials,
    WriteSerials,
    ReadClock,
    SetClock,
    PowerOnOff,
    PackLockSet,
    PackLockRelease,
    Restart,
    ForceHeater,
    ClearHistory,
    SetRemaining,
    SetLearnedFull,
    Calibrate,
    SetCanProtocol,
    HardStop,
    ReadHistory,
    ReadIdentity,
    ReadDiagnosis,
    ReadCalibration,
    ReadSocStore,
    /// the master's CCL decrease step (EXT, read-capability bit 4; a numeric setting outside the OEM block)
    ReadCclStep,
    WriteCclStep,
    MaintStatus,
    MaintStart,
    MaintStop,
    ReadSystem,
    DisplayTunnel,
    Flash,
}

impl Action {
    /// In the order the UI lists them.
    pub const ALL: [Action; 34] = [
        Action::ReadTelemetry, Action::ReadStatus, Action::ReadInfo, Action::ReadParameters, Action::WriteParameters, Action::WriteName,
        Action::ReadSerials, Action::WriteSerials, Action::ReadClock, Action::SetClock, Action::PowerOnOff, Action::PackLockSet,
        Action::PackLockRelease, Action::Restart,
        Action::ForceHeater, Action::ClearHistory, Action::SetRemaining, Action::SetLearnedFull, Action::Calibrate, Action::SetCanProtocol,
        Action::HardStop, Action::ReadHistory, Action::ReadIdentity, Action::ReadDiagnosis, Action::ReadCalibration, Action::ReadSocStore, Action::ReadCclStep, Action::WriteCclStep, Action::MaintStatus, Action::MaintStart, Action::MaintStop, Action::ReadSystem,
        Action::DisplayTunnel, Action::Flash,
    ];

    /// Whether the action changes the pack (a write, a switch, a calibration …).
    pub fn writes(self) -> bool {
        !matches!(
            self,
            Action::ReadTelemetry | Action::ReadStatus | Action::ReadInfo | Action::ReadParameters | Action::ReadSerials | Action::ReadClock
                | Action::ReadHistory | Action::ReadIdentity | Action::ReadDiagnosis | Action::ReadCalibration | Action::ReadSocStore
                | Action::ReadCclStep | Action::MaintStatus | Action::ReadSystem
        )
    }

    /// A write the app runs only on a connection with an installation (not „Nur ansehen“). The flash and the
    /// maintenance have flows of their own.
    pub fn needs_installation(self) -> bool {
        self.writes() && !matches!(self, Action::Flash | Action::MaintStart | Action::MaintStop)
    }

    /// A detection query: safe to send to any pack, but only a patched one answers it. Its `ANY` way is no promise that
    /// the original firmware has the function.
    pub fn probe(self) -> bool {
        matches!(self, Action::ReadIdentity)
    }
}

/// What the firmware of the pack must be for a way, as data: a new patch function needs a row here, not new code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum Firmware {
    /// a patch of at least this version (major, minor) in the pack's own 64/03
    PatchFrom { version: [u8; 2] },
    /// this image flag bit set in the pack's own 64/03 (EXT05; TEMP(maint-1.6): MNT format 3)
    ImageFlag { bit: u8 },
    /// any bit of `mask` set in the read-capability mask of the pack's own 64/03 (EXT05, `maint::CAP_*`)
    ReadCap { mask: u16 },
}

/// Image flag bit 0: the hard stop at a recharge pause compiled in (with `FLAG_HARD_STOP`: only its default).
pub const FLAG_HARD_STOP_ON: u8 = 0;
/// Image flag bits a way depends on today.
pub const FLAG_POWER: u8 = 1;
pub const FLAG_CUTOFF: u8 = 2;
/// Status, clear and identity of 0x64 also over RS485-1/2
pub const FLAG_UART0: u8 = 4;
/// The Group0 bits 3/4 enable at 1 (every EMU1101-EXT build): bit 3 at 1 allows power and clock over the CAN socket,
/// bit 4 at 1 switches the extended off on (Full; reserved in Light).
pub const FLAG_POSITIVE: u8 = 3;
/// The memory diagnosis 0x64/04 over RS485-1/2 and the CAN socket (Light, Full)
pub const FLAG_DIAGNOSIS: u8 = 5;
/// A planned restart saves the SOC first; RTN 08 when that fails, then no restart (Full)
pub const FLAG_CHECKPOINT: u8 = 6;
/// The hard stop at a recharge pause chosen by Group0 bits 6/7 (Full)
pub const FLAG_HARD_STOP: u8 = 7;

/// The first supported build that carries each image flag, for the matrix's „ab EMU1101-EXT …“.
pub const FLAG_SINCE: [(u8, [u8; 3]); 7] = [
    (FLAG_POWER, SUPPORTED_15), (FLAG_UART0, SUPPORTED_15), (FLAG_DIAGNOSIS, SUPPORTED_15), (FLAG_POSITIVE, SUPPORTED_15),
    (FLAG_CUTOFF, SUPPORTED_16), (FLAG_CHECKPOINT, SUPPORTED_16), (FLAG_HARD_STOP, SUPPORTED_16),
];

/// The first supported build that carries each read-capability bit (format 5), for the matrix's „ab EMU1101-EXT …“.
pub const CAP_SINCE: [(u8, [u8; 3]); 5] = [(CAP_HISTORY, [1, 5, 4]), (CAP_CALIBRATION, [1, 5, 4]), (CAP_SOC_STORE, [1, 5, 4]), (CAP_ENERGY, [1, 5, 4]), (CAP_CCL_STEP, [1, 6, 7])];

/// Whether an image flag byte announces the positive Group0 bits 3/4 (None: not known, read as not).
pub fn positive_of(flags: Option<u8>) -> bool {
    flags.is_some_and(|f| f & (1 << FLAG_POSITIVE) != 0)
}

/// Bit 3 at 1 allows power and clock: a build with image flag bit 3 (every EMU1101-EXT); without known flags (a stored
/// proof) every supported build. TEMP(maint-1.6): the first 1.6 (flags 06) keeps bit 3 at 0; a stored proof of it is read
/// as positive, its power ways stay unknown without the flags anyway.
pub fn positive_build(patch: Patch, flags: Option<u8>) -> bool {
    positive_of(flags) || (flags.is_none() && matches!(patch, Patch::Version(_)))
}

/// The Group0 bits whose direction depends on the build (`inverted`): texts name them as a release without a value and
/// leave the direction to the checked switch texts.
pub const BUILD_POLARITY: [(u8, u8); 2] = [(0, G0_POWER_LOCK), (0, G0_EXTENDED_OFF)];

/// Whether Group0 bit `bit` acts at 1 where the routes are written for 0: bit 4 (the extended off) always, bit 3 (power
/// and clock) in a positive build (`positive_build`).
pub fn inverted(group: u8, bit: u8, patch: Patch, flags: Option<u8>) -> bool {
    group == 0 && (bit == G0_EXTENDED_OFF || (bit == G0_POWER_LOCK && positive_build(patch, flags)))
}

/// A further condition of a way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum Need {
    /// GroupN bit B must be 1 (Group0 bits 3/4 as written for the inverse builds: inverted per `inverted`)
    BitSet { group: u8, bit: u8 },
    /// GroupN bit B must be 0 (the same inversion)
    BitClear { group: u8, bit: u8 },
    /// only the master (address 0)
    MasterOnly,
    /// RS485-1/2: reliable only with the master's polling stopped (a note; the app does not refuse on it)
    QuietBus,
    /// only the board the display port is attached to (UART2 ignores the frame's address)
    LocalBoard,
    /// the master's maintenance marker must be off (start) or on (stop); a foreign word refuses both
    Marker { on: bool },
    /// the firmware base (from the pack's 64/03) at least this
    BaseFrom { base: [u8; 3] },
    /// the master works with slaves (effective slave count > 0; maintenance sets it to 0)
    WithSlaves,
}

/// What a way returns where the ways of one action differ in scope (the help and hints name it, so the OEM master's
/// shortened String 1 is never read as the patch's full answer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Yields {
    /// battery and BMS serial number (String 1 and 2), up to 30 bytes each
    BothSerials,
    /// only the battery serial number (String 1), its first 22 characters
    BatteryString1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Route {
    pub action: Action,
    pub channel: Channel,
    /// command on the wire, as the analysis names it
    pub wire: &'static str,
    /// all of them (e.g. the pack cutoff and, over the CAN socket, the power and clock build)
    pub firmware: &'static [Firmware],
    pub needs: &'static [Need],
    /// whether this app sends it
    pub implemented: bool,
    /// the scope of its answer where it differs from another way of the action
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yields: Option<Yields>,
}

const fn r(action: Action, channel: Channel, wire: &'static str, firmware: &'static [Firmware], needs: &'static [Need], implemented: bool) -> Route {
    Route { action, channel, wire, firmware, needs, implemented, yields: None }
}

impl Route {
    const fn yields(mut self, y: Yields) -> Route {
        self.yields = Some(y);
        self
    }
}

use Action as A;
use Channel::{Uart0 as U0, Uart1 as U1, Uart2 as U2};
const ANY: &[Firmware] = &[];
/// a supported patch (EMU1101-EXT ≥ `EXT_MIN`)
const EXT: &[Firmware] = &[Firmware::PatchFrom { version: EXT_MIN }];
/// the power and clock build (image flag bit 1)
const POWER: &[Firmware] = &[Firmware::ImageFlag { bit: FLAG_POWER }];
/// the stored pack cutoff: patch ≥ `CUTOFF_MIN` with image flag bit 2
const CUTOFF: &[Firmware] = &[Firmware::PatchFrom { version: CUTOFF_MIN }, Firmware::ImageFlag { bit: FLAG_CUTOFF }];
/// 0x64 over RS485-1/2 (image flag bit 4)
const UART0_EXT: &[Firmware] = &[Firmware::ImageFlag { bit: FLAG_UART0 }];
/// 0x64/04 (image flag bit 5)
const DIAGNOSIS: &[Firmware] = &[Firmware::ImageFlag { bit: FLAG_DIAGNOSIS }];
/// the configurable hard stop (image flag bit 7)
const HARD_STOP: &[Firmware] = &[Firmware::ImageFlag { bit: FLAG_HARD_STOP }];
/// format 5: the history over the CAN socket, the calibration pages, the 64/06 frame (stored SOC records or live energy)
const CAP_HISTORY_FW: &[Firmware] = &[Firmware::ReadCap { mask: 1 << CAP_HISTORY }];
const CAP_CALIBRATION_FW: &[Firmware] = &[Firmware::ReadCap { mask: 1 << CAP_CALIBRATION }];
const CAP_SOC_FW: &[Firmware] = &[Firmware::ReadCap { mask: (1 << CAP_SOC_STORE) | (1 << CAP_ENERGY) }];
/// capability bit 4 (Full 1.6.7): the CCL decrease step 64/07–08
const CAP_CCL_FW: &[Firmware] = &[Firmware::ReadCap { mask: 1 << CAP_CCL_STEP }];
const CUTOFF_POWER: &[Firmware] = &[Firmware::PatchFrom { version: CUTOFF_MIN }, Firmware::ImageFlag { bit: FLAG_CUTOFF }, Firmware::ImageFlag { bit: FLAG_POWER }];

const QUIET: &[Need] = &[Need::QuietBus];
const LOCAL: &[Need] = &[Need::BitSet { group: 7, bit: G7_DISPLAY }, Need::LocalBoard];
const MASTER: &[Need] = &[Need::MasterOnly];
const WRITE_RELEASE: &[Need] = &[Need::BitSet { group: 0, bit: G0_WRITE }];
const POWER_FREE: &[Need] = &[Need::BitClear { group: 0, bit: G0_POWER_LOCK }];
const START: &[Need] = &[Need::MasterOnly, Need::Marker { on: false }, Need::BitSet { group: 0, bit: G0_START }, Need::BitClear { group: 7, bit: G7_AUTO_ADDRESS }];
const STOP: &[Need] = &[Need::MasterOnly, Need::Marker { on: true }];
/// RS485-1/2: every pack at its own address (a slave may carry a marker from its time as master)
const STOP_U0: &[Need] = &[Need::Marker { on: true }, Need::QuietBus];
/// a new stored lock: the extended off switched on (Group0 bit 4 = 0); releasing needs it not
const LOCK_SET_U0: &[Need] = &[Need::BitClear { group: 0, bit: G0_EXTENDED_OFF }, Need::QuietBus];
const LOCK_SET_U1: &[Need] = &[Need::BitClear { group: 0, bit: G0_EXTENDED_OFF }, Need::BitClear { group: 0, bit: G0_POWER_LOCK }];
const STATUS_LOCAL: &[Need] = &[Need::BitSet { group: 7, bit: G7_DISPLAY }, Need::LocalBoard];
const SYSTEM_LOCAL: &[Need] = &[Need::BitSet { group: 7, bit: G7_DISPLAY }, Need::BitSet { group: 7, bit: G7_EXTENDED_DISPLAY }, Need::MasterOnly, Need::WithSlaves, Need::LocalBoard];
const TUNNEL_LOCAL: &[Need] = &[Need::BaseFrom { base: [16, 6, 13] }, Need::BitSet { group: 7, bit: G7_DISPLAY }, Need::LocalBoard];
const MODBUS_SYSTEM: &[Need] = &[Need::MasterOnly, Need::BaseFrom { base: [16, 6, 4] }];
const MODBUS_STRING1: &[Need] = &[Need::MasterOnly, Need::BaseFrom { base: [16, 6, 11] }];

/// Every way the firmware has, also the ones the app does not use (`implemented: false`).
pub const ROUTES: &[Route] = &[
    r(A::ReadTelemetry, U0, "ASCII 42", ANY, QUIET, true),
    r(A::ReadTelemetry, U1, "ASCII 42", ANY, &[], true),
    r(A::ReadTelemetry, U2, "binär 46 (spontan)", ANY, LOCAL, false),
    r(A::ReadStatus, U0, "ASCII 44", ANY, QUIET, true),
    r(A::ReadStatus, U1, "ASCII 44", ANY, &[], true),
    r(A::ReadStatus, U2, "binär 46 (spontan, Status/Ereignisse)", ANY, STATUS_LOCAL, false),
    r(A::ReadInfo, U0, "ASCII 51", ANY, QUIET, true),
    r(A::ReadInfo, U1, "ASCII 51", ANY, &[], true),
    r(A::ReadParameters, U0, "ASCII 47", ANY, QUIET, true),
    r(A::ReadParameters, U1, "binär 47", EXT, &[], true),
    r(A::ReadParameters, U2, "binär 47", ANY, LOCAL, false),
    r(A::WriteParameters, U0, "ASCII A1 (169 Byte)", ANY, QUIET, true),
    r(A::WriteParameters, U1, "binär A1 (158 Byte)", EXT, WRITE_RELEASE, true),
    r(A::WriteParameters, U2, "binär A1 (158 Byte)", ANY, LOCAL, false),
    r(A::WriteName, U0, "ASCII A1", ANY, QUIET, true),
    r(A::ReadSerials, U0, "ASCII A2/A4", ANY, QUIET, true).yields(Yields::BothSerials),
    r(A::ReadSerials, U1, "binär A2/A4", EXT, &[], true).yields(Yields::BothSerials),
    r(A::ReadSerials, U1, "Modbus FC04 0000×21 (String 1, 22 Zeichen)", ANY, MODBUS_STRING1, true).yields(Yields::BatteryString1),
    r(A::WriteSerials, U0, "ASCII A3/A5", ANY, QUIET, true),
    r(A::ReadClock, U0, "ASCII 4D", ANY, QUIET, true),
    r(A::ReadClock, U1, "ASCII 4D", POWER, &[], true),
    r(A::SetClock, U0, "ASCII 4E", ANY, QUIET, true),
    r(A::SetClock, U1, "ASCII 4E", POWER, POWER_FREE, true),
    r(A::PowerOnOff, U0, "ASCII 45/04", ANY, QUIET, true),
    r(A::PowerOnOff, U1, "ASCII 45/04", POWER, POWER_FREE, true),
    r(A::PackLockSet, U0, "ASCII 45/04 Aus (gezielt)", CUTOFF, LOCK_SET_U0, true),
    r(A::PackLockSet, U1, "ASCII 45/04 Aus (gezielt)", CUTOFF_POWER, LOCK_SET_U1, true),
    r(A::PackLockRelease, U0, "ASCII 45/04 Ein (gezielt)", CUTOFF, QUIET, true),
    r(A::PackLockRelease, U1, "ASCII 45/04 Ein (gezielt)", CUTOFF_POWER, POWER_FREE, true),
    r(A::Restart, U0, "ASCII 45/06", ANY, QUIET, true),
    r(A::Restart, U1, "ASCII 45/06", POWER, POWER_FREE, true),
    r(A::ForceHeater, U0, "ASCII 45/03", ANY, QUIET, true),
    r(A::ClearHistory, U0, "ASCII 45/05", ANY, QUIET, true),
    r(A::SetRemaining, U0, "ASCII 49/3B", ANY, QUIET, true),
    r(A::SetLearnedFull, U0, "ASCII 49/3A", ANY, QUIET, true),
    r(A::Calibrate, U0, "ASCII A0", ANY, QUIET, true),
    r(A::SetCanProtocol, U0, "ASCII A6", ANY, QUIET, true),
    r(A::SetCanProtocol, U2, "binär 63", ANY, LOCAL, false),
    r(A::HardStop, U0, "ASCII A1 (Group0.Bit6/7)", HARD_STOP, QUIET, true),
    r(A::HardStop, U1, "binär A1 (Group0.Bit6/7)", HARD_STOP, WRITE_RELEASE, true),
    r(A::HardStop, U2, "binär A1 (Group0.Bit6/7)", HARD_STOP, LOCAL, false),
    r(A::ReadHistory, U0, "ASCII 4B", ANY, QUIET, true),
    r(A::ReadHistory, U1, "ASCII 4B", CAP_HISTORY_FW, &[], true),
    r(A::ReadIdentity, U0, "ASCII 64/03 (Erkennung)", UART0_EXT, QUIET, true),
    r(A::ReadIdentity, U1, "ASCII 64/03 (Erkennung)", ANY, &[], true),
    r(A::ReadDiagnosis, U0, "ASCII 64/04", DIAGNOSIS, QUIET, true),
    r(A::ReadDiagnosis, U1, "ASCII 64/04", DIAGNOSIS, &[], true),
    r(A::ReadCalibration, U0, "ASCII 64/05", CAP_CALIBRATION_FW, QUIET, true),
    r(A::ReadCalibration, U1, "ASCII 64/05", CAP_CALIBRATION_FW, &[], true),
    r(A::ReadSocStore, U0, "ASCII 64/06", CAP_SOC_FW, QUIET, true),
    r(A::ReadSocStore, U1, "ASCII 64/06", CAP_SOC_FW, &[], true),
    // every pack at its own address; only the master's value acts on the bank, so the app reads and writes it only at
    // address 0 (`ccl.rs`). RS485-1/2 needs no Group0 bit (local parameter access), only the quiet bus
    r(A::ReadCclStep, U0, "ASCII 64/07", CAP_CCL_FW, QUIET, true),
    r(A::ReadCclStep, U1, "ASCII 64/07", CAP_CCL_FW, &[], true),
    r(A::WriteCclStep, U0, "ASCII 64/08", CAP_CCL_FW, QUIET, true),
    r(A::WriteCclStep, U1, "ASCII 64/08", CAP_CCL_FW, WRITE_RELEASE, true),
    r(A::MaintStatus, U0, "ASCII 64/02", UART0_EXT, QUIET, true),
    r(A::MaintStatus, U1, "ASCII 64/02", EXT, MASTER, true),
    r(A::MaintStart, U1, "ASCII 64/01", EXT, START, true),
    r(A::MaintStop, U0, "ASCII 64/00", UART0_EXT, STOP_U0, true),
    r(A::MaintStop, U1, "ASCII 64/00", EXT, STOP, true),
    r(A::ReadSystem, U1, "Modbus FC03 1000×21", ANY, MODBUS_SYSTEM, true),
    r(A::ReadSystem, U2, "binär 46 erweitert (Systemblock)", ANY, SYSTEM_LOCAL, false),
    r(A::DisplayTunnel, U0, "ASCII AA/AB", ANY, &[Need::BaseFrom { base: [16, 6, 13] }, Need::QuietBus], false),
    r(A::DisplayTunnel, U2, "binär AA/AB", ANY, TUNNEL_LOCAL, false),
    r(A::Flash, U0, "Bootloader", ANY, QUIET, true),
];

/// The ways of `action`, in channel order (a channel may have several, e.g. the CAN socket's two serial reads).
pub fn routes(action: Action) -> impl Iterator<Item = &'static Route> {
    ROUTES.iter().filter(move |r| r.action == action)
}

/// The first way of `action` over `channel`.
pub fn route(action: Action, channel: Channel) -> Option<&'static Route> {
    ROUTES.iter().find(|r| r.action == action && r.channel == channel)
}

/// Known, not, or not known (e.g. an image flag behind a stored proof).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tri {
    Yes,
    No,
    Unknown,
}

impl From<Option<bool>> for Tri {
    fn from(v: Option<bool>) -> Self {
        match v {
            Some(true) => Tri::Yes,
            Some(false) => Tri::No,
            None => Tri::Unknown,
        }
    }
}

/// The patch a pack reports in its own 64/03, as far as this connection knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "version")]
pub enum Patch {
    /// not known (no proof on this connection, the pack silent)
    Unknown,
    /// known to have no supported patch identity (original firmware, another identity format)
    None,
    /// this patch version (major, minor)
    Version([u8; 2]),
}

/// What is known of one pack: its patch and image flags, its last Group0/Group7 (a hint for a preview; a way that writes
/// reads them fresh before it sends) and its address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Facts {
    pub patch: Patch,
    /// the image flag byte of its own 64/03; None: not known (a stored proof carries none)
    pub flags: Option<u8>,
    /// the read-capability mask of its own 64/03 (0 before format 5); None: not known (a stored proof carries none)
    pub caps: Option<u16>,
    pub group0: Option<u8>,
    pub group7: Option<u8>,
    pub address: u8,
    /// the connection has an installation (writes need one; „Nur ansehen“ writes nothing)
    pub installation: bool,
    /// the master's maintenance marker as its last 64/02 showed it
    pub marker: Option<MarkerWord>,
    /// the firmware base from the pack's 64/03 (16.06.13 → [16, 6, 13]); None: not known (0x51 shows only 16.06)
    pub base: Option<[u8; 3]>,
    /// the master's effective slave count as its last 64/02 showed it
    pub slaves: Option<u8>,
}

impl Facts {
    fn firmware(&self, f: Firmware) -> Tri {
        match (f, self.patch) {
            (_, Patch::None) => Tri::No,
            (Firmware::PatchFrom { .. }, Patch::Unknown) => Tri::Unknown,
            (Firmware::PatchFrom { version }, Patch::Version(v)) => Tri::from(Some(v >= version)),
            (Firmware::ImageFlag { bit }, _) => Tri::from(self.flags.map(|f| f & (1 << bit) != 0)),
            (Firmware::ReadCap { mask }, _) => Tri::from(self.caps.map(|c| c & mask != 0)),
        }
    }

    fn group(&self, g: u8) -> Option<u8> {
        match g {
            0 => self.group0,
            7 => self.group7,
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    /// all conditions known and met
    Ready,
    /// a condition is not known (an unread bit, an image flag behind a stored proof): a read may try, a write reads first
    Unknown,
    /// a condition is known not to hold
    Blocked,
    /// the firmware has the way, the app does not use it
    NotImplemented,
}

/// Why a way is not ready, as stable ids the UI translates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "id")]
pub enum Reason {
    Firmware { need: Firmware, known: Tri },
    Bit { group: u8, bit: u8, want: bool, known: Option<bool> },
    MasterOnly,
    QuietBus,
    LocalBoard,
    NotImplemented,
    /// the master has no slaves now (`known`), or that is not known
    WithSlaves { known: Option<u8> },
    /// „Nur ansehen“: the connection has no installation
    ViewOnly,
    /// the marker must be off/on; `known`: "off", "on", "foreign" or None (not read)
    Marker { on: bool, known: Option<&'static str> },
    Base { from: [u8; 3], known: Option<[u8; 3]> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Decision {
    pub route: Route,
    pub state: State,
    /// the conditions that fail or are not known, and the notes (quiet bus, local board) that always apply
    pub reasons: Vec<Reason>,
}

/// Evaluate `route` for `facts`. Blocked wins over unknown; a way the app does not use is `NotImplemented` with the
/// firmware's conditions still named.
pub fn decide(route: &Route, facts: &Facts) -> Decision {
    let mut reasons = Vec::new();
    let mut state = State::Ready;
    let mut worse = |s: State| {
        if s == State::Blocked || (s == State::Unknown && state == State::Ready) {
            state = s;
        }
    };
    for &need in route.firmware {
        match facts.firmware(need) {
            Tri::Yes => {}
            known => {
                worse(if known == Tri::No { State::Blocked } else { State::Unknown });
                reasons.push(Reason::Firmware { need, known });
            }
        }
    }
    for need in route.needs {
        match *need {
            Need::BitSet { group, bit } | Need::BitClear { group, bit } => {
                // the positive build reads bits 3/4 the other way round
                let want = matches!(need, Need::BitSet { .. }) != inverted(group, bit, facts.patch, facts.flags);
                let known = facts.group(group).map(|v| v & (1 << bit) != 0);
                if known != Some(want) {
                    worse(if known.is_none() { State::Unknown } else { State::Blocked });
                    reasons.push(Reason::Bit { group, bit, want, known });
                }
            }
            Need::MasterOnly => {
                if facts.address != 0 {
                    worse(State::Blocked);
                    reasons.push(Reason::MasterOnly);
                }
            }
            Need::QuietBus => reasons.push(Reason::QuietBus),
            Need::LocalBoard => reasons.push(Reason::LocalBoard),
            Need::Marker { on } => {
                let known = facts.marker.map(|m| match m {
                    MarkerWord::Off => "off",
                    MarkerWord::On => "on",
                    MarkerWord::Foreign(_) => "foreign",
                });
                if known != Some(if on { "on" } else { "off" }) {
                    worse(if known.is_none() { State::Unknown } else { State::Blocked });
                    reasons.push(Reason::Marker { on, known });
                }
            }
            Need::WithSlaves => {
                if facts.slaves.is_none_or(|n| n == 0) {
                    worse(if facts.slaves.is_none() { State::Unknown } else { State::Blocked });
                    reasons.push(Reason::WithSlaves { known: facts.slaves });
                }
            }
            Need::BaseFrom { base } => {
                if facts.base.is_none_or(|b| b < base) {
                    worse(if facts.base.is_none() { State::Unknown } else { State::Blocked });
                    reasons.push(Reason::Base { from: base, known: facts.base });
                }
            }
        }
    }
    if route.implemented && route.action.needs_installation() && !facts.installation {
        worse(State::Blocked);
        reasons.push(Reason::ViewOnly);
    }
    if !route.implemented {
        state = State::NotImplemented;
        reasons.push(Reason::NotImplemented);
    }
    Decision { route: *route, state, reasons }
}

/// Every way of every action for one pack.
pub fn view(facts: &Facts) -> Vec<Decision> {
    Action::ALL.iter().flat_map(|a| routes(*a)).map(|r| decide(r, facts)).collect()
}

/// Whether `action` may go over `channel` for `facts` by its firmware and bits alone (`Ready`); the identity, the
/// reservation and the fresh read of the sending path come on top.
pub fn ready(action: Action, channel: Channel, facts: &Facts) -> bool {
    route(action, channel).is_some_and(|r| decide(r, facts).state == State::Ready)
}

/// The patch a connection proved for one pack, as far as a parameter write needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Proof {
    /// Group0 bit 0 releases the CAN socket's parameter write
    pub write_gate: bool,
    /// Group0 bit 2 releases the maintenance start
    pub start_gate: bool,
    /// the power and clock build (Group0 bit 3 locks it)
    pub power: Tri,
    /// the stored pack cutoff (Group0 bit 4 extended off, bit 5 lock state)
    pub cutoff: Tri,
}

/// What a parameter write over a channel does with Group0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Group0Plan {
    /// sent as the pack has them now (the fresh read), whatever the edit says
    pub keep: u8,
    /// a change of these refuses the whole write here: nothing is sent
    pub refuse_change: u8,
    /// these must be 1 in the fresh read, else nothing is sent
    pub need_set: u8,
}

impl Group0Plan {
    /// The bits an edit cannot change on this way (the UI shows them fixed).
    pub fn fixed(&self) -> u8 {
        self.keep | self.refuse_change
    }
}

/// Why a parameter write was refused before sending (`apply_group0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Group0Refusal {
    /// the CAN socket's write release (bit 0) is off
    WriteLocked,
    /// the edit changes the start release (bit 2), which only RS485-1/2 or the display changes
    StartBit,
}

/// The Group0 plan of a parameter write over `channel` for a pack with `proof` (None: no proof on this connection).
///
/// - Bit 5 (lock state) is status only: always as the pack has it, so an old backup never clears a stored cutoff.
/// - CAN socket without proof: every patch bit as the pack has it.
/// - CAN socket with proof: bit 3 never changes (the power build refuses the whole write, E3; without it the bit has no
///   function); bit 4 only with the proven cutoff; bit 0 only where it gates (and writing needs it set, a pack may lock
///   itself 1→0); bit 2 refuses the write when changed (start gate) or is kept (no function).
/// - RS485-1/2: the way to change the bits; with a proof the bits without function in that build stay as they are.
pub fn group0_plan(channel: Channel, proof: Option<Proof>) -> Group0Plan {
    const W: u8 = 1 << G0_WRITE;
    const S: u8 = 1 << G0_START;
    const P: u8 = 1 << G0_POWER_LOCK;
    const C: u8 = 1 << G0_EXTENDED_OFF;
    const L: u8 = 1 << G0_LOCK_STATE;
    let on = |b: bool, m: u8| if b { m } else { 0 };
    match (channel, proof) {
        (Channel::Uart1, None) => Group0Plan { keep: L | W | S | P | C, refuse_change: 0, need_set: 0 },
        (Channel::Uart1, Some(p)) => Group0Plan {
            keep: L | P | on(p.cutoff != Tri::Yes, C) | on(!p.write_gate, W) | on(!p.start_gate, S),
            refuse_change: on(p.start_gate, S),
            need_set: on(p.write_gate, W),
        },
        (_, None) => Group0Plan { keep: L, refuse_change: 0, need_set: 0 },
        (_, Some(p)) => Group0Plan {
            keep: L | on(!p.write_gate, W) | on(!p.start_gate, S) | on(p.power == Tri::No, P) | on(p.cutoff == Tri::No, C),
            refuse_change: 0,
            need_set: 0,
        },
    }
}

/// Apply `plan` to the Group0 byte `block` of the block to send, built on the fresh read's `fresh`.
pub fn apply_group0(plan: Group0Plan, fresh: u8, block: &mut u8) -> Result<(), Group0Refusal> {
    *block = (*block & !plan.keep) | (fresh & plan.keep);
    if fresh & plan.need_set != plan.need_set {
        return Err(Group0Refusal::WriteLocked);
    }
    if (*block ^ fresh) & plan.refuse_change != 0 {
        return Err(Group0Refusal::StartBit);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The facts of a pack by the stages the tests name: a supported patch (`maint`), the power build and the stored
    /// cutoff, each known or not; a stored proof (patch known, flags not) is `maint` Yes with power/cutoff Unknown.
    fn facts(maint: Tri, power: Tri, cutoff: Tri, group0: Option<u8>, address: u8) -> Facts {
        let patch = match (maint, cutoff) {
            (_, Tri::Yes) => Patch::Version([1, 6]),
            (Tri::Yes, _) => Patch::Version([1, 5]),
            (Tri::No, _) => Patch::None,
            (Tri::Unknown, _) => Patch::Unknown,
        };
        let flags = match (power, cutoff) {
            (Tri::Unknown, _) | (_, Tri::Unknown) => None,
            // every supported power build (Light, Full) has the positive bit 3
            (p, c) => Some((u8::from(p == Tri::Yes) << FLAG_POWER) | (u8::from(c == Tri::Yes) << FLAG_CUTOFF) | (u8::from(p == Tri::Yes || c == Tri::Yes) << FLAG_POSITIVE)),
        };
        Facts { patch, flags, caps: flags.map(|_| 0), group0, group7: None, address, installation: true, marker: None, base: Some([16, 6, 13]), slaves: None }
    }

    #[test]
    fn every_action_has_a_way_and_no_way_is_listed_twice() {
        for a in Action::ALL {
            assert!(routes(a).count() > 0, "{a:?}");
        }
        for (i, x) in ROUTES.iter().enumerate() {
            assert!(!ROUTES[i + 1..].iter().any(|y| y.action == x.action && y.channel == x.channel && y.wire == x.wire), "{x:?}");
        }
    }

    /// The catalog the UI evaluates (`app/src/generated/access.json`: constants, every route) and the test vectors of
    /// `decide` and `group0_plan` its own evaluation must reproduce (`access.vectors.json`, read by `access.test.ts`). A change of the
    /// catalog fails here until the file is written again: `UPDATE_ACCESS=1 cargo test -p openbms-proto`.
    #[test]
    fn the_ui_catalog_is_current() {
        let tris = [Tri::Yes, Tri::No, Tri::Unknown];
        let markers = [None, Some(MarkerWord::Off), Some(MarkerWord::On), Some(MarkerWord::Foreign(0x1234_5678))];
        let bases = [None, Some([16, 6, 3]), Some([16, 6, 12]), Some([16, 6, 13])];
        // a fixed walk over the fact space: every value of each field shows up, combined differently
        let facts: Vec<Facts> = (0..96usize)
            .map(|i| Facts {
                patch: [Patch::Unknown, Patch::None, Patch::Version([1, 4]), Patch::Version([1, 5]), Patch::Version([1, 6]), Patch::Version([1, 7])][i % 6],
                flags: [None, Some(0x00), Some(0x02), Some(0x06), Some(0x0E), Some(0x3A), Some(0xFE)][(i / 6) % 7],
                // flags 3A with caps 000F, FE with 001F, and the reverse pairings (caps without flags, flags without caps)
                caps: [None, Some(0x0000), Some(0x000F), Some(0x0001), Some(0x0008), Some(0x001F)][(i / 3) % 6],
                group0: [None, Some(0x00), Some(0xFF), Some(0xF7), Some(0xEF), Some(0xDA)][(i * 7) % 6],
                group7: [None, Some(0x00), Some(0x08), Some(0x18), Some(0x38)][(i * 5) % 5],
                address: [0, 3][(i / 2) % 2],
                installation: i % 5 != 0,
                marker: markers[(i * 3) % 4],
                base: bases[(i / 4) % 4],
                slaves: [None, Some(0), Some(3)][(i / 6) % 3],
            })
            .collect();
        let decisions: Vec<serde_json::Value> = facts
            .iter()
            .map(|f| {
                let ds: Vec<_> = ROUTES.iter().map(|r| decide(r, f)).map(|d| serde_json::json!({ "state": d.state, "reasons": d.reasons })).collect();
                serde_json::json!({ "facts": f, "decisions": ds })
            })
            .collect();
        let mut plans = vec![];
        for channel in [U0, U1, U2] {
            plans.push(serde_json::json!({ "channel": channel, "proof": null, "plan": group0_plan(channel, None) }));
            for wg in [false, true] {
                for sg in [false, true] {
                    for power in tris {
                        for cutoff in tris {
                            let proof = Proof { write_gate: wg, start_gate: sg, power, cutoff };
                            plans.push(serde_json::json!({ "channel": channel, "proof": proof, "plan": group0_plan(channel, Some(proof)) }));
                        }
                    }
                }
            }
        }
        let catalog = serde_json::json!({
            "comment": "generated by openbms-proto access::tests::the_ui_catalog_is_current; do not edit",
            "dip_max": crate::address::DIP_MAX,
            // P0..P86 in BMS order: technical key, display unit, raw conversion and width (2 bytes up to P59, then 1)
            "params": crate::params::DEFINITIONS.iter().enumerate().map(|(i, (key, raw, unit))| {
                let (scale, offset) = raw.scale_offset();
                serde_json::json!({ "key": key, "unit": unit, "scale": scale, "offset": offset, "signed": raw.signed(), "bytes": if i < crate::params::FIRST_U8 { 2 } else { 1 } })
            }).collect::<Vec<_>>(),
            "lines": { "1.5": SUPPORTED_15, "1.6": SUPPORTED_16 },
            "flag_since": FLAG_SINCE.iter().map(|(bit, v)| (bit.to_string(), *v)).collect::<std::collections::BTreeMap<_, _>>(),
            "cap_since": CAP_SINCE.iter().map(|(bit, v)| (bit.to_string(), *v)).collect::<std::collections::BTreeMap<_, _>>(),
            "caps": { "history": CAP_HISTORY, "calibration": CAP_CALIBRATION, "soc_store": CAP_SOC_STORE, "energy": CAP_ENERGY, "ccl_step": CAP_CCL_STEP },
            "ext_min": EXT_MIN,
            "cutoff_min": CUTOFF_MIN,
            "bits": {
                "g0_write": G0_WRITE, "g0_start": G0_START, "g0_power_lock": G0_POWER_LOCK, "g0_extended_off": G0_EXTENDED_OFF,
                "g0_lock_state": G0_LOCK_STATE, "g0_hard_stop": G0_HARD_STOP, "g0_hard_stop_inherit": G0_HARD_STOP_INHERIT, "g7_display": G7_DISPLAY, "g7_extended_display": G7_EXTENDED_DISPLAY, "g7_auto_address": G7_AUTO_ADDRESS,
            },
            "flags": { "power": FLAG_POWER, "cutoff": FLAG_CUTOFF, "uart0": FLAG_UART0, "positive": FLAG_POSITIVE, "diagnosis": FLAG_DIAGNOSIS, "checkpoint": FLAG_CHECKPOINT, "hard_stop": FLAG_HARD_STOP },
            "needs_installation": Action::ALL.iter().filter(|a| a.needs_installation()).collect::<Vec<_>>(),
            "probe_actions": Action::ALL.iter().filter(|a| a.probe()).collect::<Vec<_>>(),
            "build_polarity": BUILD_POLARITY,
            "routes": ROUTES,
        });
        let vectors = serde_json::json!({ "comment": catalog["comment"], "decide": decisions, "group0": plans });
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/src/generated");
        for (name, value) in [("access.json", &catalog), ("access.vectors.json", &vectors)] {
            let text = format!("{}\n", serde_json::to_string(value).unwrap());
            let path = dir.join(name);
            if std::env::var_os("UPDATE_ACCESS").is_some() {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(&path, &text).unwrap();
            }
            let now = std::fs::read_to_string(&path).unwrap_or_default();
            assert!(now == text, "app/src/generated/{name} is stale: UPDATE_ACCESS=1 cargo test -p openbms-proto");
        }
    }

    #[test]
    fn the_new_reads_follow_their_own_capability_bits() {
        let f = |flags: Option<u8>, caps: Option<u16>| Facts {
            patch: Patch::Version([1, 5]), flags, caps, group0: None, group7: None, address: 3, installation: false, marker: None, base: None, slaves: None,
        };
        // 1.5.4: flags 3A, caps 000F; every new read is ready without an installation or a release bit
        let light = f(Some(0x3A), Some(0x000F));
        for a in [A::ReadHistory, A::ReadCalibration, A::ReadSocStore] {
            assert!(ready(a, U1, &light), "{a:?}");
        }
        // the flags alone (no mask) give none of them; the CAN socket history needs bit 0, the RS485 one nothing
        let old = f(Some(0x3A), Some(0));
        assert!(!ready(A::ReadHistory, U1, &old) && ready(A::ReadHistory, U0, &old));
        assert_eq!(decide(route(A::ReadCalibration, U1).unwrap(), &old).state, State::Blocked);
        // a stored proof (caps not known): unknown, not blocked
        assert_eq!(decide(route(A::ReadSocStore, U1).unwrap(), &f(None, None)).state, State::Unknown);
        // 64/06 with only the energy bit, or only the SOC bit
        assert!(ready(A::ReadSocStore, U1, &f(Some(0), Some(1 << CAP_ENERGY))));
        assert!(ready(A::ReadSocStore, U1, &f(Some(0), Some(1 << CAP_SOC_STORE))));
        assert!(!ready(A::ReadSocStore, U1, &f(Some(0xFF), Some(1 << CAP_HISTORY))));
        assert!(!A::ReadCalibration.writes() && !A::ReadSocStore.writes());
    }

    #[test]
    fn the_ccl_step_follows_capability_bit_4_and_its_write_bit_0() {
        let f = |caps: Option<u16>, group0: Option<u8>| Facts {
            patch: Patch::Version([1, 6]), flags: Some(0xFE), caps, group0, group7: None, address: 0, installation: true, marker: None, base: None, slaves: None,
        };
        // Full 1.6.7 (001F): the read needs no release bit, the write over the CAN socket Group0 bit 0
        assert!(ready(A::ReadCclStep, U1, &f(Some(0x001F), None)));
        assert!(ready(A::WriteCclStep, U1, &f(Some(0x001F), Some(0x01))));
        let locked = decide(route(A::WriteCclStep, U1).unwrap(), &f(Some(0x001F), Some(0xFE)));
        assert_eq!((locked.state, locked.reasons), (State::Blocked, vec![Reason::Bit { group: 0, bit: G0_WRITE, want: true, known: Some(false) }]));
        assert_eq!(decide(route(A::WriteCclStep, U1).unwrap(), &f(Some(0x001F), None)).state, State::Unknown);
        // Light and Full up to 1.6.6 (000F), the version alone, or a stored proof without the mask
        assert_eq!(decide(route(A::ReadCclStep, U1).unwrap(), &f(Some(0x000F), None)).state, State::Blocked);
        assert_eq!(decide(route(A::WriteCclStep, U1).unwrap(), &f(Some(0x000F), Some(0xFF))).state, State::Blocked);
        assert_eq!(decide(route(A::ReadCclStep, U1).unwrap(), &f(None, None)).state, State::Unknown);
        // „Nur ansehen“ reads, never writes
        let view = Facts { installation: false, ..f(Some(0x001F), Some(0xFF)) };
        assert!(ready(A::ReadCclStep, U1, &view) && !ready(A::WriteCclStep, U1, &view));
        assert!(!A::ReadCclStep.writes() && A::WriteCclStep.writes() && A::WriteCclStep.needs_installation());
        // RS485-1/2 too: no release bit there, only the quiet bus (a note); the CAN socket's write needs bit 0
        assert_eq!(routes(A::WriteCclStep).map(|r| (r.channel, r.implemented)).collect::<Vec<_>>(), vec![(U0, true), (U1, true)]);
        assert_eq!(routes(A::ReadCclStep).map(|r| (r.channel, r.implemented)).collect::<Vec<_>>(), vec![(U0, true), (U1, true)]);
        assert_eq!((route(A::WriteCclStep, U0).unwrap().needs, route(A::ReadCclStep, U0).unwrap().needs), (QUIET, QUIET));
        let u0 = decide(route(A::WriteCclStep, U0).unwrap(), &f(Some(0x001F), Some(0xFE)));
        assert_eq!((u0.state, u0.reasons), (State::Ready, vec![Reason::QuietBus]));
        assert_eq!(decide(route(A::WriteCclStep, U0).unwrap(), &f(Some(0x000F), Some(0xFF))).state, State::Blocked);
        assert!(!ready(A::WriteCclStep, U0, &view));
        // every pack answers at its own address: no master-only condition in the firmware's way
        assert!(routes(A::WriteCclStep).chain(routes(A::ReadCclStep)).all(|r| !r.needs.contains(&Need::MasterOnly)));
    }

    #[test]
    fn the_group0_plan_per_channel_and_proof() {
        let proof = |write_gate, start_gate, power, cutoff| Some(Proof { write_gate, start_gate, power, cutoff });
        // no proof: the CAN socket keeps every patch bit, RS485-1/2 only the lock state
        assert_eq!(group0_plan(U1, None), Group0Plan { keep: 0x3D, refuse_change: 0, need_set: 0 });
        assert_eq!(group0_plan(U0, None), Group0Plan { keep: 0x20, refuse_change: 0, need_set: 0 });
        // CAN socket, 1.5 with both gates: bit 0 free but needed set, bit 2 refused when changed, bits 3/4/5 kept
        let p = group0_plan(U1, proof(true, true, Tri::Yes, Tri::No));
        assert_eq!(p, Group0Plan { keep: 0x38, refuse_change: 0x04, need_set: 0x01 });
        assert_eq!(p.fixed(), 0x3C);
        // the proven cutoff frees bit 4 there
        assert_eq!(group0_plan(U1, proof(true, true, Tri::Yes, Tri::Yes)).keep, 0x28);
        // RS485-1/2 with a proof: only the bits without function in that build stay
        assert_eq!(group0_plan(U0, proof(true, true, Tri::Unknown, Tri::Unknown)).keep, 0x20);
        assert_eq!(group0_plan(U0, proof(true, true, Tri::No, Tri::No)).keep, 0x38);
        // applying: kept bits from the pack, the refusals before anything is sent
        let p = group0_plan(U1, proof(true, true, Tri::Yes, Tri::No));
        let mut b = 0x00;
        assert_eq!(apply_group0(p, 0xFF, &mut b), Err(Group0Refusal::StartBit));
        let mut b = 0xFF;
        assert_eq!(apply_group0(p, 0xFE, &mut b), Err(Group0Refusal::WriteLocked));
        let mut b = 0xCC;
        assert_eq!(apply_group0(p, 0xFF, &mut b), Ok(()));
        assert_eq!(b, 0xFC);
    }

    #[test]
    fn the_matrix_rows_of_the_firmware_analysis() {
        // only RS485-1/2: heater, history clear, 49, calibration, serial number writes, CAN protocol (besides UART2)
        for a in [A::ForceHeater, A::ClearHistory, A::SetRemaining, A::SetLearnedFull, A::Calibrate, A::WriteSerials, A::WriteName, A::Flash] {
            assert_eq!(routes(a).map(|r| r.channel).collect::<Vec<_>>(), vec![U0], "{a:?}");
        }
        // the history also over the CAN socket, there only with read-capability bit 0 (EXT05)
        assert_eq!(routes(A::ReadHistory).map(|r| r.channel).collect::<Vec<_>>(), vec![U0, U1]);
        assert_eq!(route(A::ReadHistory, U1).unwrap().firmware, &[Firmware::ReadCap { mask: 1 }]);
        assert_eq!(routes(A::SetCanProtocol).map(|r| r.channel).collect::<Vec<_>>(), vec![U0, U2]);
        // maintenance over the CAN socket at the master; start only there
        for a in [A::MaintStatus, A::MaintStart, A::MaintStop] {
            assert!(route(a, U1).unwrap().needs.contains(&Need::MasterOnly), "{a:?}");
        }
        assert_eq!(routes(A::MaintStart).count(), 1);
        // status, clear and identity also over RS485-1/2 with image flag bit 4, at every address
        for a in [A::MaintStatus, A::MaintStop, A::ReadIdentity] {
            let r = route(a, U0).unwrap();
            assert_eq!(r.firmware, UART0_EXT, "{a:?}");
            assert!(!r.needs.contains(&Need::MasterOnly) && r.needs.contains(&Need::QuietBus), "{a:?}");
        }
        // UART2 is never implemented and always local
        assert!(ROUTES.iter().filter(|r| r.channel == U2).all(|r| !r.implemented && r.needs.contains(&Need::LocalBoard)));
        // power and clock over the CAN socket: own image flag, Group0 bit 3 clear (reading the clock without it)
        for a in [A::SetClock, A::PowerOnOff, A::Restart] {
            let r = route(a, U1).unwrap();
            assert_eq!((r.firmware, r.needs), (POWER, POWER_FREE), "{a:?}");
        }
        assert_eq!(route(A::ReadClock, U1).unwrap().needs, &[] as &[Need]);
    }

    #[test]
    fn the_positive_build_reads_bits_3_and_4_the_other_way_round() {
        // Full 1.6.5: flags FE (power, cutoff, positive, UART0 marker, diagnosis, checkpoint, configurable hard stop)
        let f = |g0: u8| Facts { patch: Patch::Version([1, 6]), flags: Some(0xFE), caps: Some(0), group0: Some(g0), group7: None, address: 2, installation: true, marker: None, base: None, slaves: None };
        // TEMP(maint-1.6): the first 1.6 build (flags 06) keeps bit 3 at 0
        let first = |g0: u8| Facts { flags: Some(0x06), ..f(g0) };
        assert!(ready(A::PowerOnOff, U1, &first(0xF7)) && !ready(A::PowerOnOff, U1, &first(0xFF)));
        // Light 1.5.5 (flags 3A) reads bit 3 as Full
        let p15 = |g0: u8| Facts { patch: Patch::Version([1, 5]), flags: Some(0x3A), ..f(g0) };
        assert!(ready(A::PowerOnOff, U1, &p15(0xFF)) && !ready(A::PowerOnOff, U1, &p15(0xF7)));
        // power over the CAN socket: bit 3 at 1 allows it
        assert!(ready(A::PowerOnOff, U1, &f(0xFF)));
        assert!(!ready(A::PowerOnOff, U1, &f(0xF7)));
        // a new pack lock: bit 4 at 1 switches the extended off on
        assert!(ready(A::PackLockSet, U0, &f(0xFF)));
        assert!(!ready(A::PackLockSet, U0, &f(0xEF)));
        let blocked = decide(route(A::PackLockSet, U0).unwrap(), &f(0xEF));
        assert_eq!(blocked.reasons[0], Reason::Bit { group: 0, bit: G0_EXTENDED_OFF, want: true, known: Some(false) });
        // other bits never inverted
        let v16 = Patch::Version([1, 6]);
        assert!(!inverted(0, G0_WRITE, v16, None) && !inverted(7, 4, v16, None) && inverted(0, 3, v16, None) && inverted(0, 4, Patch::Unknown, None));
        assert!(!inverted(0, 3, Patch::Version([1, 5]), Some(0x12)) && !inverted(0, 3, Patch::Unknown, None));
    }

    #[test]
    fn the_rs485_clear_needs_the_uart0_flag_and_the_marker_on_at_any_address() {
        let r = route(A::MaintStop, U0).unwrap();
        let on = Facts { patch: Patch::Version([1, 6]), flags: Some(1 << FLAG_UART0), caps: None, group0: None, group7: None, address: 5, installation: false, marker: Some(MarkerWord::On), base: None, slaves: None };
        assert_eq!(decide(r, &on).state, State::Ready);
        // 1.6 without the flag: blocked; behind a stored proof (flags unknown): unknown
        assert_eq!(decide(r, &Facts { flags: Some(0x06), ..on }).state, State::Blocked);
        assert_eq!(decide(r, &Facts { flags: None, ..on }).state, State::Unknown);
        assert_eq!(decide(r, &Facts { marker: Some(MarkerWord::Off), ..on }).state, State::Blocked);
        // no installation needed: a board on the bench
        assert!(!A::MaintStop.needs_installation());
    }

    #[test]
    fn a_uart1_parameter_write_needs_the_patch_and_bit0() {
        let r = route(A::WriteParameters, U1).unwrap();
        assert_eq!(decide(r, &facts(Tri::Yes, Tri::Unknown, Tri::Unknown, Some(0x01), 3)).state, State::Ready);
        let locked = decide(r, &facts(Tri::Yes, Tri::Unknown, Tri::Unknown, Some(0xFE), 3));
        assert_eq!(locked.state, State::Blocked);
        assert_eq!(locked.reasons, vec![Reason::Bit { group: 0, bit: G0_WRITE, want: true, known: Some(false) }]);
        // original firmware: blocked; behind a stored proof the bit unread: unknown
        assert_eq!(decide(r, &facts(Tri::No, Tri::No, Tri::No, Some(0xFF), 3)).state, State::Blocked);
        assert_eq!(decide(r, &facts(Tri::Yes, Tri::Unknown, Tri::Unknown, None, 3)).state, State::Unknown);
    }

    #[test]
    fn power_over_the_can_socket_needs_the_own_image_flag_and_bit3_set() {
        // Light/Full: bit 3 at 1 allows
        assert!(ready(A::PowerOnOff, U1, &facts(Tri::Yes, Tri::Yes, Tri::No, Some(0xFF), 2)));
        assert!(!ready(A::PowerOnOff, U1, &facts(Tri::Yes, Tri::Yes, Tri::No, Some(0xF7), 2)));
        // a stored proof: the flag is not known
        assert_eq!(decide(route(A::PowerOnOff, U1).unwrap(), &facts(Tri::Yes, Tri::Unknown, Tri::Unknown, Some(0xFF), 2)).state, State::Unknown);
        // RS485-1/2 needs nothing of it, only the note on the quiet bus
        let d = decide(route(A::PowerOnOff, U0).unwrap(), &facts(Tri::No, Tri::No, Tri::No, None, 2));
        assert_eq!((d.state, d.reasons), (State::Ready, vec![Reason::QuietBus]));
    }

    #[test]
    fn maintenance_start_only_at_the_master_with_its_release() {
        let r = route(A::MaintStart, U1).unwrap();
        let ok = Facts { group7: Some(0x00), marker: Some(MarkerWord::Off), ..facts(Tri::Yes, Tri::No, Tri::No, Some(0x04), 0) };
        assert_eq!(decide(r, &ok).state, State::Ready);
        assert_eq!(decide(r, &Facts { address: 1, ..ok }).state, State::Blocked);
        assert_eq!(decide(r, &Facts { group7: Some(1 << G7_AUTO_ADDRESS), ..ok }).state, State::Blocked);
        // a foreign marker word refuses start and stop; an unread one leaves them open
        let foreign = Facts { marker: Some(MarkerWord::Foreign(7)), ..ok };
        assert_eq!(decide(r, &foreign).state, State::Blocked);
        assert_eq!(decide(route(A::MaintStop, U1).unwrap(), &foreign).state, State::Blocked);
        assert_eq!(decide(r, &Facts { marker: None, ..ok }).state, State::Unknown);
        // stop does not depend on the start release
        assert_eq!(decide(route(A::MaintStop, U1).unwrap(), &Facts { group0: Some(0), marker: Some(MarkerWord::On), ..ok }).state, State::Ready);
    }

    #[test]
    fn a_way_of_the_display_port_names_its_conditions_and_is_not_implemented() {
        let d = decide(route(A::SetCanProtocol, U2).unwrap(), &Facts { group7: Some(1 << G7_DISPLAY), ..facts(Tri::No, Tri::No, Tri::No, None, 0) });
        assert_eq!(d.state, State::NotImplemented);
        assert_eq!(d.reasons, vec![Reason::LocalBoard, Reason::NotImplemented]);
        let off = decide(route(A::SetCanProtocol, U2).unwrap(), &Facts { group7: Some(0), ..facts(Tri::No, Tri::No, Tri::No, None, 0) });
        assert!(off.reasons.contains(&Reason::Bit { group: 7, bit: G7_DISPLAY, want: true, known: Some(false) }));
    }

    #[test]
    fn a_new_pack_lock_needs_the_extended_off_releasing_it_does_not() {
        // 1.6 read positive: bit 3 at 1 allows power, bit 4 at 1 switches the extended off on
        let f = |g0| facts(Tri::Yes, Tri::Yes, Tri::Yes, Some(g0), 4);
        // EF: power free, extended off off (bit 4 = 0), no lock: an off does not lock
        assert_eq!(decide(route(A::PackLockSet, U0).unwrap(), &f(0xEF)).state, State::Blocked);
        assert_eq!(decide(route(A::PackLockSet, U1).unwrap(), &f(0xEF)).state, State::Blocked);
        assert_eq!(decide(route(A::PackLockSet, U1).unwrap(), &f(0xFF)).state, State::Ready);
        // F7: power locked over the CAN socket (bit 3 = 0); RS485-1/2 does not ask
        assert_eq!(decide(route(A::PackLockSet, U1).unwrap(), &f(0xF7)).state, State::Blocked);
        assert_eq!(decide(route(A::PackLockSet, U0).unwrap(), &f(0xF7)).state, State::Ready);
        assert_eq!(decide(route(A::PackLockRelease, U0).unwrap(), &f(0xDF)).state, State::Ready);
        assert_eq!(decide(route(A::PackLockRelease, U1).unwrap(), &f(0xDF)).state, State::Ready);
        // over the CAN socket also the power and clock build
        let no_power = Facts { flags: Some(1 << FLAG_CUTOFF), ..f(0xFF) };
        assert_eq!(decide(route(A::PackLockSet, U1).unwrap(), &no_power).state, State::Blocked);
        assert_eq!(decide(route(A::PackLockSet, U0).unwrap(), &no_power).state, State::Ready);
    }

    #[test]
    fn view_only_blocks_the_writes_not_the_reads() {
        let f = Facts { installation: false, ..facts(Tri::No, Tri::No, Tri::No, Some(0xFF), 1) };
        assert_eq!(decide(route(A::WriteParameters, U0).unwrap(), &f).state, State::Blocked);
        assert!(decide(route(A::WriteParameters, U0).unwrap(), &f).reasons.contains(&Reason::ViewOnly));
        assert_eq!(decide(route(A::ReadParameters, U0).unwrap(), &f).state, State::Ready);
        assert_eq!(decide(route(A::Flash, U0).unwrap(), &f).state, State::Ready);
    }

    #[test]
    fn the_masters_modbus_ways_name_their_firmware_base() {
        let system = route(A::ReadSystem, U1).unwrap();
        assert_eq!(decide(system, &facts(Tri::No, Tri::No, Tri::No, None, 0)).state, State::Ready);
        assert_eq!(decide(system, &Facts { base: Some([16, 6, 3]), ..facts(Tri::No, Tri::No, Tri::No, None, 0) }).state, State::Blocked);
        assert_eq!(decide(system, &Facts { base: None, ..facts(Tri::No, Tri::No, Tri::No, None, 0) }).state, State::Unknown);
        // the CAN socket's serial reads: the binary one of a patched pack and the master's shortened String 1
        let serials: Vec<_> = routes(A::ReadSerials).filter(|r| r.channel == U1).map(|r| decide(r, &facts(Tri::No, Tri::No, Tri::No, None, 0)).state).collect();
        assert_eq!(serials, vec![State::Blocked, State::Ready]);
    }

    #[test]
    fn the_display_ports_system_block_and_tunnel_name_their_conditions() {
        let on = Facts { group7: Some((1 << G7_DISPLAY) | (1 << G7_EXTENDED_DISPLAY)), slaves: Some(0), ..facts(Tri::No, Tri::No, Tri::No, None, 0) };
        let d = decide(route(A::ReadSystem, U2).unwrap(), &on);
        assert!(d.reasons.contains(&Reason::WithSlaves { known: Some(0) }), "{d:?}");
        assert!(!decide(route(A::ReadSystem, U2).unwrap(), &Facts { slaves: Some(3), ..on }).reasons.iter().any(|r| matches!(r, Reason::WithSlaves { .. })));
        let old = decide(route(A::DisplayTunnel, U2).unwrap(), &Facts { base: Some([16, 6, 12]), ..on });
        assert!(old.reasons.contains(&Reason::Base { from: [16, 6, 13], known: Some([16, 6, 12]) }));
    }

    #[test]
    fn only_the_listed_bits_change_direction_with_the_build() {
        let patches = [Patch::Unknown, Patch::None, Patch::Version([1, 5]), Patch::Version([1, 6])];
        let flags = [None, Some(0x00), Some(0x06), Some(0x3A), Some(0xFE)];
        for group in [0, 7] {
            for bit in 0..8 {
                let any = patches.iter().any(|&p| flags.iter().any(|&f| inverted(group, bit, p, f)));
                assert_eq!(any, BUILD_POLARITY.contains(&(group, bit)), "Group{group}.Bit{bit}");
            }
        }
    }

    #[test]
    fn the_serial_ways_name_their_scope_and_the_identity_is_a_probe() {
        let scopes: Vec<_> = routes(A::ReadSerials).map(|r| (r.channel, r.yields)).collect();
        assert_eq!(scopes, vec![(U0, Some(Yields::BothSerials)), (U1, Some(Yields::BothSerials)), (U1, Some(Yields::BatteryString1))]);
        assert!(ROUTES.iter().filter(|r| r.action != A::ReadSerials).all(|r| r.yields.is_none()));
        assert_eq!(Action::ALL.iter().filter(|a| a.probe()).collect::<Vec<_>>(), vec![&A::ReadIdentity]);
        // the probe's CAN socket way needs no firmware: asking is safe, an answer comes only from a patch
        assert_eq!(route(A::ReadIdentity, U1).unwrap().firmware, ANY);
    }

    #[test]
    fn the_cutoff_needs_patch_1_6_and_the_flag() {
        assert!(pack_cutoff_of([1, 6], true));
        assert!(!pack_cutoff_of([1, 5], true));
        assert!(!pack_cutoff_of([1, 6], false));
    }
}
