//! Which action goes over which of the three UARTs, and under which conditions, in one place: the firmware stage
//! (original, patched from MAINT 1.5, the 1.5 build with power and clock, 1.6 with the stored pack cutoff), the release
//! and lock bits of Group0/Group7, the address, and whether the app implements the way at all.
//!
//! - UART0: RS485-1/2, Seplos ASCII, 19200 Bd; the host needs a quiet bus (the master's polling stopped).
//! - UART1: the RS485 pair of the CAN socket, 9600 Bd: OEM ASCII reads, Modbus (master), the MAINT binary service and
//!   the MAINT ASCII commands; addressed, also while the master polls.
//! - UART2: the display port (binary 55…AA, e.g. a display or a Bluetooth dongle): only the locally attached board, only
//!   with Group7 bit 3; the app does not speak it, it is listed so the UI can name it.
//!
//! The catalog follows the firmware analysis (UART write matrix, 16.06.13 and the MAINT builds). Its rows are data;
//! `decide` evaluates one row for one pack's facts. Unknown stays unknown: a stored proof carries no image flags.

use crate::maint::MarkerWord;
use serde::Serialize;

/// The lowest patch version the app supports as a whole (format-2/3 identity with the release bits; 1.4 has none).
pub const MAINT_MIN: [u8; 2] = [1, 5];
/// From this patch version on, image flag bit 2 announces the stored pack cutoff (Group0 bit 4/5).
pub const CUTOFF_MIN: [u8; 2] = [1, 6];

/// Group0 bits of the patched firmware.
pub const G0_WRITE: u8 = 0;
pub const G0_START: u8 = 2;
pub const G0_POWER_LOCK: u8 = 3;
pub const G0_EXTENDED_OFF: u8 = 4;
pub const G0_LOCK_STATE: u8 = 5;
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
    ReadHistory,
    ReadIdentity,
    MaintStatus,
    MaintStart,
    MaintStop,
    ReadSystem,
    DisplayTunnel,
    Flash,
}

impl Action {
    /// In the order the UI lists them.
    pub const ALL: [Action; 28] = [
        Action::ReadTelemetry, Action::ReadStatus, Action::ReadInfo, Action::ReadParameters, Action::WriteParameters, Action::WriteName,
        Action::ReadSerials, Action::WriteSerials, Action::ReadClock, Action::SetClock, Action::PowerOnOff, Action::PackLockSet,
        Action::PackLockRelease, Action::Restart,
        Action::ForceHeater, Action::ClearHistory, Action::SetRemaining, Action::SetLearnedFull, Action::Calibrate, Action::SetCanProtocol,
        Action::ReadHistory, Action::ReadIdentity, Action::MaintStatus, Action::MaintStart, Action::MaintStop, Action::ReadSystem,
        Action::DisplayTunnel, Action::Flash,
    ];

    /// Whether the action changes the pack (a write, a switch, a calibration …).
    pub fn writes(self) -> bool {
        !matches!(
            self,
            Action::ReadTelemetry | Action::ReadStatus | Action::ReadInfo | Action::ReadParameters | Action::ReadSerials | Action::ReadClock
                | Action::ReadHistory | Action::ReadIdentity | Action::MaintStatus | Action::ReadSystem
        )
    }

    /// A write the app runs only on a connection with an installation (not „Nur ansehen“). The flash and the
    /// maintenance have flows of their own.
    pub fn needs_installation(self) -> bool {
        self.writes() && !matches!(self, Action::Flash | Action::MaintStart | Action::MaintStop)
    }
}

/// What the firmware of the pack must be for a way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Firmware {
    /// every firmware (16.06.x)
    Any,
    /// a supported patch (MAINT ≥ `MAINT_MIN`)
    Maint,
    /// image flag bit 1 of the pack's own 64/03 (the power and clock build)
    PowerFlag,
    /// patch ≥ `CUTOFF_MIN` with image flag bit 2 (the stored pack cutoff)
    Cutoff,
}

/// A further condition of a way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum Need {
    /// GroupN bit B must be 1
    BitSet { group: u8, bit: u8 },
    /// GroupN bit B must be 0
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
}

const fn r(action: Action, channel: Channel, wire: &'static str, firmware: &'static [Firmware], needs: &'static [Need], implemented: bool) -> Route {
    Route { action, channel, wire, firmware, needs, implemented }
}

use Action as A;
use Channel::{Uart0 as U0, Uart1 as U1, Uart2 as U2};
const ANY: &[Firmware] = &[];
const MAINT: &[Firmware] = &[Firmware::Maint];
const POWER: &[Firmware] = &[Firmware::PowerFlag];
const CUTOFF: &[Firmware] = &[Firmware::Cutoff];
const CUTOFF_POWER: &[Firmware] = &[Firmware::Cutoff, Firmware::PowerFlag];

const QUIET: &[Need] = &[Need::QuietBus];
const LOCAL: &[Need] = &[Need::BitSet { group: 7, bit: G7_DISPLAY }, Need::LocalBoard];
const MASTER: &[Need] = &[Need::MasterOnly];
const WRITE_RELEASE: &[Need] = &[Need::BitSet { group: 0, bit: G0_WRITE }];
const POWER_FREE: &[Need] = &[Need::BitClear { group: 0, bit: G0_POWER_LOCK }];
const START: &[Need] = &[Need::MasterOnly, Need::Marker { on: false }, Need::BitSet { group: 0, bit: G0_START }, Need::BitClear { group: 7, bit: G7_AUTO_ADDRESS }];
const STOP: &[Need] = &[Need::MasterOnly, Need::Marker { on: true }];
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
    r(A::ReadParameters, U1, "binär 47", MAINT, &[], true),
    r(A::ReadParameters, U2, "binär 47", ANY, LOCAL, false),
    r(A::WriteParameters, U0, "ASCII A1 (169 Byte)", ANY, QUIET, true),
    r(A::WriteParameters, U1, "binär A1 (158 Byte)", MAINT, WRITE_RELEASE, true),
    r(A::WriteParameters, U2, "binär A1 (158 Byte)", ANY, LOCAL, false),
    r(A::WriteName, U0, "ASCII A1", ANY, QUIET, false),
    r(A::ReadSerials, U0, "ASCII A2/A4", ANY, QUIET, true),
    r(A::ReadSerials, U1, "binär A2/A4", MAINT, &[], true),
    r(A::ReadSerials, U1, "Modbus FC04 0000×21 (String 1, 22 Zeichen)", ANY, MODBUS_STRING1, true),
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
    r(A::SetLearnedFull, U0, "ASCII 49/3A", ANY, QUIET, false),
    r(A::Calibrate, U0, "ASCII A0", ANY, QUIET, true),
    r(A::SetCanProtocol, U0, "ASCII A6", ANY, QUIET, true),
    r(A::SetCanProtocol, U2, "binär 63", ANY, LOCAL, false),
    r(A::ReadHistory, U0, "ASCII 4B", ANY, QUIET, true),
    r(A::ReadIdentity, U1, "ASCII 64/03 (Erkennung)", ANY, &[], true),
    r(A::MaintStatus, U1, "ASCII 64/02", MAINT, MASTER, true),
    r(A::MaintStart, U1, "ASCII 64/01", MAINT, START, true),
    r(A::MaintStop, U1, "ASCII 64/00", MAINT, STOP, true),
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

/// What is known of one pack: its firmware stage, its last Group0/Group7 (a hint for a preview; a way that writes reads
/// them fresh before it sends) and its address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    pub maint: Tri,
    pub power: Tri,
    pub cutoff: Tri,
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
        match f {
            Firmware::Any => Tri::Yes,
            Firmware::Maint => self.maint,
            Firmware::PowerFlag => self.power,
            Firmware::Cutoff => self.cutoff,
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
                let want = matches!(need, Need::BitSet { .. });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(maint: Tri, power: Tri, cutoff: Tri, group0: Option<u8>, address: u8) -> Facts {
        Facts { maint, power, cutoff, group0, group7: None, address, installation: true, marker: None, base: Some([16, 6, 13]), slaves: None }
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

    #[test]
    fn the_matrix_rows_of_the_firmware_analysis() {
        // only RS485-1/2: heater, history clear, 49, calibration, serial number writes, CAN protocol (besides UART2)
        for a in [A::ForceHeater, A::ClearHistory, A::SetRemaining, A::SetLearnedFull, A::Calibrate, A::WriteSerials, A::WriteName, A::ReadHistory, A::Flash] {
            assert_eq!(routes(a).map(|r| r.channel).collect::<Vec<_>>(), vec![U0], "{a:?}");
        }
        assert_eq!(routes(A::SetCanProtocol).map(|r| r.channel).collect::<Vec<_>>(), vec![U0, U2]);
        // maintenance only over the CAN socket, at the master
        for a in [A::MaintStatus, A::MaintStart, A::MaintStop] {
            let r = route(a, U1).unwrap();
            assert!(r.needs.contains(&Need::MasterOnly) && routes(a).count() == 1, "{a:?}");
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
    fn power_over_the_can_socket_needs_the_own_image_flag_and_bit3_clear() {
        assert!(ready(A::PowerOnOff, U1, &facts(Tri::Yes, Tri::Yes, Tri::No, Some(0xF7), 2)));
        assert!(!ready(A::PowerOnOff, U1, &facts(Tri::Yes, Tri::Yes, Tri::No, Some(0xFF), 2)));
        // a stored proof: the flag is not known
        assert_eq!(decide(route(A::PowerOnOff, U1).unwrap(), &facts(Tri::Yes, Tri::Unknown, Tri::Unknown, Some(0xF7), 2)).state, State::Unknown);
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
        let f = |g0| facts(Tri::Yes, Tri::Yes, Tri::Yes, Some(g0), 4);
        // F7: power free, extended off disabled (bit 4 = 1), no lock: an off does not lock
        assert_eq!(decide(route(A::PackLockSet, U0).unwrap(), &f(0xF7)).state, State::Blocked);
        assert_eq!(decide(route(A::PackLockSet, U1).unwrap(), &f(0xF7)).state, State::Blocked);
        assert_eq!(decide(route(A::PackLockSet, U1).unwrap(), &f(0xE7)).state, State::Ready);
        assert_eq!(decide(route(A::PackLockRelease, U0).unwrap(), &f(0xD7)).state, State::Ready);
        assert_eq!(decide(route(A::PackLockRelease, U1).unwrap(), &f(0xD7)).state, State::Ready);
        // over the CAN socket also the power and clock build
        let no_power = Facts { power: Tri::No, ..f(0xE7) };
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
    fn the_cutoff_needs_patch_1_6_and_the_flag() {
        assert!(pack_cutoff_of([1, 6], true));
        assert!(!pack_cutoff_of([1, 5], true));
        assert!(!pack_cutoff_of([1, 6], false));
    }
}
