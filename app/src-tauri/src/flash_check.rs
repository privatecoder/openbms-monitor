//! The comparison before/after a flash or a restart: what the target showed before and what it shows once it is back,
//! side by side. A report, not a check: nothing is refused, repeated or written back because of it. Measured values
//! move between two reads; only the stored ones (parameters, switches, identity) are expected to stay, the charge is
//! shown with its difference. The cause of a difference is not told: the reads cannot show it (a restart may load an
//! older stored SOC, both copies invalid fall back to P59/P58).

use crate::marker::Found;
use openbms_flash::session::PackState;
use openbms_proto::{access, ExtInfo, Telemetry};
use serde::Serialize;

/// A SOC difference from this many percentage points on is shown as a difference (not a measurement's drift).
pub const SOC_STEP: f64 = 5.0;
/// A learned full capacity that moved by this many Ah or more.
pub const FULL_STEP: f64 = 1.0;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Charge {
    pub soc: f64,
    pub remaining_ah: f64,
    pub full_ah: f64,
    pub cycles: u16,
}

impl Charge {
    pub fn of(t: &Telemetry) -> Self {
        Self { soc: t.soc, remaining_ah: t.remaining_capacity_ah, full_ah: t.full_capacity_ah, cycles: t.cycles }
    }
}

/// The charge after against before: a SOC step of `SOC_STEP` or more, another learned full capacity or cycle count.
pub fn charge_differs(before: &Charge, after: &Charge) -> bool {
    (after.soc - before.soc).abs() >= SOC_STEP || (after.full_ah - before.full_ah).abs() >= FULL_STEP || after.cycles != before.cycles
}

/// The patched build's identity as `0x64/03` reports it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Build {
    pub version: String,
    pub format: u8,
    pub build_id: String,
    pub flags: u8,
}

impl Build {
    fn of(i: &ExtInfo) -> Self {
        Self { version: i.version(), format: i.format, build_id: i.build_id_hex(), flags: i.image_flags() }
    }
}

/// One side of the comparison.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Side {
    /// String 2, the board's serial number (None: not readable)
    pub serial: Option<String>,
    /// `0x64/03` over RS485-1/2 (None: not answered there: the original firmware or a build without image flag bit 4)
    pub build: Option<Build>,
    /// Group0 bit 3 means "power and clock over the CAN socket allowed" when set (EMU1101-EXT); false: when clear (TEMP(maint-1.6): the first MAINT 1.6)
    /// (1.5/1.5.1); None: no patch functions known for this side
    pub positive: Option<bool>,
    pub switches: Option<[u8; 8]>,
    pub charge: Option<Charge>,
}

impl Side {
    /// `found`: the RS485-1/2 identity read; `positive`: the bit-3 direction where the identity does not tell it (the
    /// image flashed, `None` for the side before without an identity).
    pub fn new(serial: Option<String>, found: Option<&Found>, positive: Option<bool>, state: &PackState) -> Self {
        let info = found.and_then(|f| f.info);
        Self {
            serial,
            build: info.as_ref().map(Build::of),
            positive: info.map(|i| access::positive_build(access::Patch::Version(i.patch), Some(i.image_flags()))).or(positive),
            switches: state.parameters.as_ref().map(|p| p.function_switches),
            charge: state.telemetry.as_ref().map(Charge::of),
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ParamChange {
    pub index: usize,
    pub key: &'static str,
    pub unit: &'static str,
    pub before: f64,
    pub after: f64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct SwitchChange {
    pub group: usize,
    pub before: u8,
    pub after: u8,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Comparison {
    pub before: Side,
    pub after: Side,
    /// "same" | "changed" (another String 2 answered) | "unchecked" (one side not readable)
    pub identity: &'static str,
    /// the parameters P0..P86 that changed; None: one side not read, not compared
    pub params: Option<Vec<ParamChange>>,
    /// the switch bytes that changed (raw); None: not compared
    pub switches: Option<Vec<SwitchChange>>,
    /// the charge differs (`charge_differs`); None: one side not read
    pub charge: Option<bool>,
    /// "same": everything compared and nothing differs; "differs"; "incomplete": nothing differs, but not everything
    /// was read
    pub verdict: &'static str,
}

pub fn compare(before: Side, after: Side, state_before: &PackState, state_after: &PackState) -> Comparison {
    let identity = match (&before.serial, &after.serial) {
        (Some(b), Some(a)) if b == a => "same",
        (Some(_), Some(_)) => "changed",
        _ => "unchecked",
    };
    let (params, switches) = match (&state_before.parameters, &state_after.parameters) {
        (Some(b), Some(a)) => (
            Some(
                b.parameters
                    .iter()
                    .zip(&a.parameters)
                    .filter(|(x, y)| x.raw != y.raw)
                    .map(|(x, y)| ParamChange { index: x.index, key: x.key, unit: x.unit, before: x.value, after: y.value })
                    .collect::<Vec<_>>(),
            ),
            Some(
                (0..8)
                    .filter(|g| b.function_switches[*g] != a.function_switches[*g])
                    .map(|group| SwitchChange { group, before: b.function_switches[group], after: a.function_switches[group] })
                    .collect::<Vec<_>>(),
            ),
        ),
        _ => (None, None),
    };
    let charge = match (&before.charge, &after.charge) {
        (Some(b), Some(a)) => Some(charge_differs(b, a)),
        _ => None,
    };
    let differs = identity == "changed" || params.as_ref().is_some_and(|p| !p.is_empty()) || switches.as_ref().is_some_and(|s| !s.is_empty()) || charge == Some(true);
    let complete = identity != "unchecked" && params.is_some() && charge.is_some();
    let verdict = if differs { "differs" } else if complete { "same" } else { "incomplete" };
    Comparison { before, after, identity, params, switches, charge, verdict }
}

/// One line for the flash log and the maintenance events.
pub fn summary(c: &Comparison) -> String {
    let charge = |s: &Side| s.charge.as_ref().map_or("-".into(), |c| format!("soc={} remaining={} full={} cycles={}", c.soc, c.remaining_ah, c.full_ah, c.cycles));
    let switches = |s: &Side| s.switches.map_or("-".into(), |w| w.iter().map(|b| format!("{b:02X}")).collect::<String>());
    format!(
        "COMPARE verdict={} identity={} params_changed={} switches before={} after={} charge before[{}] after[{}] build before={} after={}",
        c.verdict,
        c.identity,
        c.params.as_ref().map_or("-".into(), |p| p.iter().map(|x| format!("P{}:{}->{}", x.index, x.before, x.after)).collect::<Vec<_>>().join(",")),
        switches(&c.before),
        switches(&c.after),
        charge(&c.before),
        charge(&c.after),
        c.before.build.as_ref().map_or("-".into(), |b| format!("{}/{}/{:02X}", b.version, b.build_id, b.flags)),
        c.after.build.as_ref().map_or("-".into(), |b| format!("{}/{}/{:02X}", b.version, b.build_id, b.flags)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use openbms_proto::Parameters;

    /// A synthetic 0x47 INFO: P0..P59 = 1000 + i (P10 as given), P60.. = i, the switches as given.
    fn params(p10: u16, switches: [u8; 8]) -> Parameters {
        let mut b = vec![0, 0x3C];
        for i in 0..60u16 {
            b.extend(if i == 10 { p10 } else { 1000 + i }.to_be_bytes());
        }
        b.push(0x1B);
        b.extend(60..87u8);
        b.push(0x08);
        b.extend(switches);
        b.extend(b"SYN-TEST  ");
        Parameters::parse(&b).unwrap()
    }

    fn charge(soc: f64, full: f64, cycles: u16) -> Charge {
        Charge { soc, remaining_ah: soc * full / 100.0, full_ah: full, cycles }
    }

    fn state(p10: u16, g0: u8) -> PackState {
        PackState { parameters: Some(params(p10, [g0, 0, 0, 0, 0, 0, 0, 0])), telemetry: None }
    }

    fn side(serial: &str, g0: u8, c: Option<Charge>) -> Side {
        Side { serial: Some(serial.into()), build: None, positive: None, switches: Some([g0, 0, 0, 0, 0, 0, 0, 0]), charge: c }
    }

    #[test]
    fn an_unchanged_pack_with_a_small_drift_is_the_same() {
        let (b, a) = (state(3500, 0x18), state(3500, 0x18));
        let c = compare(side("SN-A", 0x18, Some(charge(79.1, 280.0, 7))), side("SN-A", 0x18, Some(charge(78.0, 280.0, 7))), &b, &a);
        assert_eq!((c.verdict, c.identity, c.charge), ("same", "same", Some(false)));
        assert_eq!(c.params, Some(vec![]));
        assert_eq!(c.switches, Some(vec![]));
    }

    #[test]
    fn a_soc_step_back_after_the_flash_is_a_difference() {
        let (b, a) = (state(3500, 0x18), state(3500, 0x18));
        let c = compare(side("SN-A", 0x18, Some(charge(79.1, 280.0, 7))), side("SN-A", 0x18, Some(charge(26.8, 280.0, 7))), &b, &a);
        assert_eq!((c.verdict, c.charge), ("differs", Some(true)));
        // another learned full capacity or cycle count, too
        assert!(charge_differs(&charge(50.0, 280.0, 7), &charge(50.0, 270.0, 7)));
        assert!(charge_differs(&charge(50.0, 280.0, 7), &charge(50.0, 280.0, 8)));
    }

    #[test]
    fn changed_parameters_and_switch_bytes_are_listed_field_by_field() {
        let (b, a) = (state(3500, 0x18), state(3400, 0x38));
        let c = compare(side("SN-A", 0x18, None), side("SN-A", 0x38, None), &b, &a);
        let p = c.params.unwrap();
        assert_eq!((p.len(), p[0].index, p[0].before, p[0].after), (1, 10, 35.0, 34.0));
        assert_eq!(c.switches, Some(vec![SwitchChange { group: 0, before: 0x18, after: 0x38 }]));
        assert_eq!(c.verdict, "differs");
    }

    #[test]
    fn another_board_answering_is_a_difference_and_a_missing_read_stays_unchecked() {
        let s = state(3500, 0x18);
        assert_eq!(compare(side("SN-A", 0x18, None), side("SN-B", 0x18, None), &s, &s).identity, "changed");
        let mut unread = side("SN-A", 0x18, None);
        unread.serial = None;
        let c = compare(side("SN-A", 0x18, Some(charge(50.0, 280.0, 7))), unread, &s, &PackState::default());
        assert_eq!((c.identity, c.params.as_ref(), c.charge, c.verdict), ("unchecked", None, None, "incomplete"));
    }

    #[test]
    fn the_bit_3_direction_comes_from_the_identity_else_from_the_image() {
        use openbms_flash::maint::policy::Master;
        let mut i = ExtInfo { format: 5, base: [16, 6, 13], patch: [1, 5], build_id: [0x96; 8], hard_stop: Some(false), power_rtc: true, pack_cutoff: false, uart0: true, patch_rev: 1, reserved_flags: 0 , read_caps: 0};
        let f = |i: ExtInfo| Found { kind: "patched", info: Some(i), state: None, trusted: true, master: Master::No0x64 };
        assert_eq!(Side::new(None, Some(&f(i)), Some(true), &PackState::default()).positive, Some(false));
        i.patch = [1, 6];
        i.reserved_flags = 0x08;
        assert_eq!(Side::new(None, Some(&f(i)), None, &PackState::default()).positive, Some(true));
        assert_eq!(Side::new(None, None, Some(true), &PackState::default()).positive, Some(true));
        assert_eq!(Side::new(None, None, None, &PackState::default()).positive, None);
    }
}
