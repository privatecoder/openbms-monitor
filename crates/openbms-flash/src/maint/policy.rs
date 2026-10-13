//! What the app knows about the master, and which preparation a flash of the master needs (design:
//! "Master state as seen by the app", "Flashing the master itself"). Pure, so every combination is tested.

use super::dialog::Probe;
use crate::known::ImageKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MasterState {
    /// patched firmware, marker 0, `slaves` effective (= DIP count)
    PatchedOff { slaves: u8 },
    /// patched firmware, marker 1, and the marker is accounted for
    PatchedOn,
    /// verified RTN 04
    No0x64,
    /// no answer, connection error, other RTN
    Unknown,
    /// marker 1 that nobody can account for: locked
    MarkerUnexplained,
}

/// `probe` None = the control channel could not be used. `marker_expected`: an open marker session
/// exists, the app has just set the marker, or the user stated where it comes from.
pub fn classify(probe: Option<Probe>, marker_expected: bool) -> MasterState {
    match probe {
        Some(Probe::Status(s)) if !s.marker => MasterState::PatchedOff { slaves: s.effective_slaves },
        // marker set but still polling (1/N): not a state the app can act on
        Some(Probe::Status(s)) if s.effective_slaves > 0 => MasterState::Unknown,
        Some(Probe::Status(_)) if marker_expected => MasterState::PatchedOn,
        Some(Probe::Status(_)) => MasterState::MarkerUnexplained,
        Some(Probe::Unsupported) => MasterState::No0x64,
        _ => MasterState::Unknown,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// the user sets DIP 5–8 OFF and confirms
    DipOff,
    /// send `stop` and wait for 0/0 (after DipOff)
    StopToZero,
    /// quiet bus + 0x47 at address 0 (+ fresh 1/0 while the marker is set)
    Measure,
    /// not a known build: the user confirms it is released for a shared bus, or the master is isolated
    IsolatedOrReleased,
}

/// What 0x64 must show after the flash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    MarkerOn,
    Zero,
    /// 0/0 expected; 1/0 means an old marker or other content in 0x23000
    ZeroOrUnexplained,
    /// RTN 04 (original firmware)
    Unsupported,
    NoAssumption,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub before: Vec<Step>,
    pub after: Expect,
    /// recovery row for `marker-unexplained`: no `stop`, sector content stays unexplained
    pub recovery: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// the master still polls: activate maintenance mode or set DIP 5–8 OFF first
    QuietFirst,
    /// nothing is unlocked while the state is unknown
    Unknown,
    /// marker of unexplained origin: only the recovery to the original image
    Locked,
}

fn row(before: &[Step], after: Expect) -> Result<Plan, Refusal> {
    Ok(Plan { before: before.to_vec(), after, recovery: false })
}

pub fn master_flash_plan(state: MasterState, image: ImageKind) -> Result<Plan, Refusal> {
    use MasterState as M;
    use Step::*;
    match (state, image) {
        (M::Unknown, _) => Err(Refusal::Unknown),
        (M::MarkerUnexplained, ImageKind::Original) => Ok(Plan { before: vec![DipOff, Measure], after: Expect::Unsupported, recovery: true }),
        (M::MarkerUnexplained, _) => Err(Refusal::Locked),
        // the image kind decides first: an unknown image may ignore the marker
        (M::PatchedOn | M::PatchedOff { .. }, ImageKind::Custom) => row(&[DipOff, StopToZero, Measure, IsolatedOrReleased], Expect::NoAssumption),
        (M::No0x64, ImageKind::Custom) => row(&[DipOff, Measure, IsolatedOrReleased], Expect::NoAssumption),
        (M::PatchedOn, ImageKind::Maintenance) => row(&[Measure], Expect::MarkerOn),
        // the original ignores the marker: clear it under DIP OFF so no hidden state stays in flash
        (M::PatchedOn, ImageKind::Original) => row(&[DipOff, StopToZero, Measure], Expect::Unsupported),
        (M::PatchedOff { slaves: 1.. }, _) => Err(Refusal::QuietFirst),
        (M::PatchedOff { slaves: 0 }, ImageKind::Maintenance) => row(&[Measure], Expect::Zero),
        (M::PatchedOff { slaves: 0 }, ImageKind::Original) => row(&[Measure], Expect::Unsupported),
        (M::No0x64, ImageKind::Maintenance) => row(&[DipOff, Measure], Expect::ZeroOrUnexplained),
        (M::No0x64, ImageKind::Original) => row(&[DipOff, Measure], Expect::Unsupported),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    AsExpected,
    /// keep the session open, no inverter release, show DIP 5–8 OFF and the recovery hint
    Unclear,
    MarkerUnexplained,
    /// custom image: show what answers, DIP 5–8 stay OFF
    UnknownImage,
}

pub fn after_flash(expect: Expect, probe: Option<Probe>) -> Outcome {
    let s = match probe {
        Some(Probe::Status(s)) => Some(s),
        _ => None,
    };
    match expect {
        Expect::NoAssumption => Outcome::UnknownImage,
        Expect::MarkerOn if s.is_some_and(|s| s.marker && s.effective_slaves == 0) => Outcome::AsExpected,
        Expect::Zero | Expect::ZeroOrUnexplained if s.is_some_and(|s| !s.marker && s.effective_slaves == 0) => Outcome::AsExpected,
        Expect::ZeroOrUnexplained if s.is_some_and(|s| s.marker && s.effective_slaves == 0) => Outcome::MarkerUnexplained,
        Expect::Unsupported if probe == Some(Probe::Unsupported) => Outcome::AsExpected,
        _ => Outcome::Unclear,
    }
}

/// What 0x64 must read on the control channel right before a flash (design, step 3). Derived from the
/// stored state just before the measurement, i.e. after any `stop` of the master-flash plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fresh {
    /// 1/0
    MarkerOn,
    /// 0/0 (patched, no marker; with N > 0 stored this means DIP 5–8 are OFF now)
    Zero,
    /// RTN 04
    Unsupported,
    /// state unknown (manual DIP way): only the pack bus measurement counts
    Unchecked,
}

pub fn fresh_status(state: MasterState) -> Fresh {
    match state {
        MasterState::PatchedOn | MasterState::MarkerUnexplained => Fresh::MarkerOn,
        MasterState::PatchedOff { .. } => Fresh::Zero,
        MasterState::No0x64 => Fresh::Unsupported,
        MasterState::Unknown => Fresh::Unchecked,
    }
}

pub fn fresh_matches(f: Fresh, probe: Option<Probe>) -> bool {
    match (f, probe) {
        (Fresh::Unchecked, _) => true,
        (Fresh::MarkerOn, Some(Probe::Status(s))) => s.marker && s.effective_slaves == 0,
        (Fresh::Zero, Some(Probe::Status(s))) => !s.marker && s.effective_slaves == 0,
        (Fresh::Unsupported, Some(Probe::Unsupported)) => true,
        _ => false,
    }
}

/// After a master flash: keep reading 0x64 until the answer is final. A set marker takes effect from the
/// master's next one-second cycle, so 1/N is still in transition.
pub fn settled(probe: Option<Probe>) -> bool {
    match probe {
        None | Some(Probe::NoAnswer) => false,
        Some(Probe::Status(s)) => !(s.marker && s.effective_slaves > 0),
        Some(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::known::ImageKind as K;
    use openbms_proto::MaintStatus;
    use MasterState as M;
    use Step as S;

    fn status(marker: bool, slaves: u8) -> Option<Probe> {
        Some(Probe::Status(MaintStatus { marker, effective_slaves: slaves }))
    }

    #[test]
    fn classify_uses_the_open_session() {
        assert_eq!(classify(status(false, 11), false), M::PatchedOff { slaves: 11 });
        assert_eq!(classify(status(true, 0), true), M::PatchedOn);
        assert_eq!(classify(status(true, 0), false), M::MarkerUnexplained);
        assert_eq!(classify(Some(Probe::Unsupported), false), M::No0x64);
        // a set marker counts only with 0 effective slaves; 1/N means the master still polls
        assert_eq!(classify(status(true, 3), true), M::Unknown);
        assert_eq!(classify(status(true, 3), false), M::Unknown);
        // never shown as no-0x64 without a verified RTN 04
        for p in [None, Some(Probe::NoAnswer), Some(Probe::Rejected(5))] {
            assert_eq!(classify(p, false), M::Unknown);
        }
    }

    #[test]
    fn every_combination_has_a_defined_row() {
        use Expect as E;
        let plan = |s, k| master_flash_plan(s, k);
        let ok = |before: &[Step], after| Ok(Plan { before: before.to_vec(), after, recovery: false });
        for k in [K::Original, K::Maintenance, K::Custom] {
            assert_eq!(plan(M::Unknown, k), Err(Refusal::Unknown));
        }
        assert_eq!(plan(M::PatchedOn, K::Maintenance), ok(&[S::Measure], E::MarkerOn));
        assert_eq!(plan(M::PatchedOn, K::Original), ok(&[S::DipOff, S::StopToZero, S::Measure], E::Unsupported));
        assert_eq!(plan(M::PatchedOff { slaves: 11 }, K::Original), Err(Refusal::QuietFirst));
        assert_eq!(plan(M::PatchedOff { slaves: 11 }, K::Maintenance), Err(Refusal::QuietFirst));
        assert_eq!(plan(M::PatchedOff { slaves: 0 }, K::Maintenance), ok(&[S::Measure], E::Zero));
        assert_eq!(plan(M::PatchedOff { slaves: 0 }, K::Original), ok(&[S::Measure], E::Unsupported));
        assert_eq!(plan(M::No0x64, K::Maintenance), ok(&[S::DipOff, S::Measure], E::ZeroOrUnexplained));
        assert_eq!(plan(M::No0x64, K::Original), ok(&[S::DipOff, S::Measure], E::Unsupported));
    }

    #[test]
    fn the_image_kind_decides_first_for_custom_images() {
        let custom = [S::DipOff, S::StopToZero, S::Measure, S::IsolatedOrReleased];
        for s in [M::PatchedOn, M::PatchedOff { slaves: 0 }, M::PatchedOff { slaves: 11 }] {
            assert_eq!(master_flash_plan(s, K::Custom).unwrap().before, custom.to_vec());
        }
        assert_eq!(master_flash_plan(M::No0x64, K::Custom).unwrap().before, vec![S::DipOff, S::Measure, S::IsolatedOrReleased]);
        assert_eq!(master_flash_plan(M::No0x64, K::Custom).unwrap().after, Expect::NoAssumption);
    }

    #[test]
    fn marker_unexplained_allows_only_recovery_to_original() {
        let r = master_flash_plan(M::MarkerUnexplained, K::Original).unwrap();
        assert!(r.recovery);
        assert_eq!(r.before, vec![S::DipOff, S::Measure]);
        assert!(!r.before.contains(&S::StopToZero));
        assert_eq!(master_flash_plan(M::MarkerUnexplained, K::Maintenance), Err(Refusal::Locked));
        assert_eq!(master_flash_plan(M::MarkerUnexplained, K::Custom), Err(Refusal::Locked));
        // the normal patched-on -> original change always clears the marker first
        assert!(master_flash_plan(M::PatchedOn, K::Original).unwrap().before.contains(&S::StopToZero));
    }

    #[test]
    fn after_flash_outcomes() {
        use Expect as E;
        assert_eq!(after_flash(E::MarkerOn, status(true, 0)), Outcome::AsExpected);
        assert_eq!(after_flash(E::MarkerOn, status(true, 3)), Outcome::Unclear);
        assert_eq!(after_flash(E::ZeroOrUnexplained, status(true, 3)), Outcome::Unclear);
        assert_eq!(after_flash(E::MarkerOn, status(false, 11)), Outcome::Unclear);
        assert_eq!(after_flash(E::MarkerOn, None), Outcome::Unclear);
        assert_eq!(after_flash(E::Zero, status(false, 0)), Outcome::AsExpected);
        assert_eq!(after_flash(E::ZeroOrUnexplained, status(true, 0)), Outcome::MarkerUnexplained);
        assert_eq!(after_flash(E::ZeroOrUnexplained, status(false, 0)), Outcome::AsExpected);
        assert_eq!(after_flash(E::Unsupported, Some(Probe::Unsupported)), Outcome::AsExpected);
        assert_eq!(after_flash(E::Unsupported, status(false, 0)), Outcome::Unclear);
        assert_eq!(after_flash(E::NoAssumption, status(false, 0)), Outcome::UnknownImage);
    }

    #[test]
    fn fresh_status_follows_the_state() {
        // after `stop` the stored state is patched-off 0/0, so the fresh status expects 0/0, not 1/0
        assert_eq!(fresh_status(M::PatchedOff { slaves: 0 }), Fresh::Zero);
        // patched-off with N > 0 only reaches a flash in a DIP session: DIP 5–8 OFF reads 0/0
        assert_eq!(fresh_status(M::PatchedOff { slaves: 11 }), Fresh::Zero);
        assert_eq!(fresh_status(M::PatchedOn), Fresh::MarkerOn);
        assert_eq!(fresh_status(M::MarkerUnexplained), Fresh::MarkerOn);
        assert_eq!(fresh_status(M::No0x64), Fresh::Unsupported);
        assert_eq!(fresh_status(M::Unknown), Fresh::Unchecked);
        assert!(fresh_matches(Fresh::MarkerOn, status(true, 0)));
        assert!(!fresh_matches(Fresh::MarkerOn, status(true, 2)));
        assert!(!fresh_matches(Fresh::MarkerOn, status(false, 0)));
        assert!(fresh_matches(Fresh::Zero, status(false, 0)));
        assert!(!fresh_matches(Fresh::Zero, status(false, 1)));
        assert!(fresh_matches(Fresh::Unsupported, Some(Probe::Unsupported)));
        assert!(!fresh_matches(Fresh::Unsupported, Some(Probe::NoAnswer)));
        assert!(!fresh_matches(Fresh::Zero, None));
        assert!(fresh_matches(Fresh::Unchecked, None));
    }

    #[test]
    fn post_flash_polling_waits_for_a_settled_answer() {
        assert!(!settled(None));
        assert!(!settled(Some(Probe::NoAnswer)));
        assert!(!settled(status(true, 3))); // marker set, slave count not yet 0 (next 1 s cycle)
        assert!(settled(status(true, 0)));
        assert!(settled(status(false, 11)));
        assert!(settled(Some(Probe::Unsupported)));
        assert!(settled(Some(Probe::Rejected(5))));
    }
}
