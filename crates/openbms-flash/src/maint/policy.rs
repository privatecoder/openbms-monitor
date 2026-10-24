//! What the app makes of the master's answer, and the one rule for flashing the master itself (design:
//! docs/superpowers/specs/2026-10-13-simple-flash-flow.md, scenario C'). Pure, so every case is tested.
//!
//! What a patched master offers follows from its identity (`known::caps_of`, a supported build): an unlisted build or a
//! later OEM base of a supported version gets the functions too. The build list (`known::PATCH_BUILDS`) only names a
//! build; it grants nothing.

use super::dialog::Probe;
use crate::known::{self, Caps, ImageKind, PatchBuild};
use openbms_proto::{MaintInfo, MaintState, MarkerWord};

/// A recognised patched master: its identity, what it offers and its listed name (None: build not listed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Patch {
    pub info: MaintInfo,
    pub caps: Caps,
    pub listed: Option<&'static str>,
}

impl Patch {
    /// The identity against `table`: capabilities of a supported build (None: not a supported patch), the name if listed.
    pub fn recognise_in(table: &'static [PatchBuild], info: MaintInfo) -> Option<Self> {
        let caps = known::caps_of(&info)?;
        let listed = known::patch_in(table, info.format, info.base, info.patch, &info.build_id_hex()).map(|b| b.name);
        Some(Self { info, caps, listed })
    }

    pub fn recognise(info: MaintInfo) -> Option<Self> {
        Self::recognise_in(known::PATCH_BUILDS, info)
    }
}

/// The master as read over the CAN socket bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Master {
    /// a supported patch (format 2, patch version with the patch functions); `state`: its `02` answer (None: not read)
    Patched { patch: Patch, state: Option<MaintState> },
    /// a patch the app does not support: format 1 or another layout, RTN 05 to `03`, or format 2 below the supported
    /// patch version (then with its identity); shown, never started or stopped, no patch functions
    UnknownPatch(Option<MaintInfo>),
    /// RTN 04: no 0x64 (original or another build; the firmware is not proven)
    No0x64,
    /// no valid answer, or no CAN socket endpoint
    Unknown,
}

impl Master {
    pub fn of(probe: Option<Probe>) -> Self {
        Self::of_in(known::PATCH_BUILDS, probe)
    }

    pub fn of_in(table: &'static [PatchBuild], probe: Option<Probe>) -> Self {
        match probe {
            Some(Probe::Info(info)) => match Patch::recognise_in(table, info) {
                Some(patch) => Master::Patched { patch, state: None },
                None => Master::UnknownPatch(Some(info)),
            },
            Some(Probe::OtherPatch) => Master::UnknownPatch(None),
            Some(Probe::No0x64) => Master::No0x64,
            Some(Probe::Failed) | None => Master::Unknown,
        }
    }

    /// The same master with its `02` state.
    pub fn with_state(self, state: Option<MaintState>) -> Self {
        match self {
            Master::Patched { patch, .. } => Master::Patched { patch, state },
            m => m,
        }
    }

    pub fn caps(&self) -> Option<Caps> {
        match self {
            Master::Patched { patch, .. } => Some(patch.caps),
            _ => None,
        }
    }

    pub fn state(&self) -> Option<MaintState> {
        match self {
            Master::Patched { state, .. } => *state,
            _ => None,
        }
    }

    /// The marker as the master reports it; None without a supported patch or without a state read.
    pub fn marker(&self) -> Option<MarkerWord> {
        self.state().map(|s| s.marker())
    }

    /// The slave count the master works with (0 while its marker holds the bus quiet, or with DIP 5–8 OFF).
    pub fn effective_slaves(&self) -> Option<u8> {
        self.state().map(|s| s.effective_slaves)
    }

    /// The app may send it start and stop.
    pub fn trusted(&self) -> bool {
        self.caps().is_some_and(|c| c.status_split)
    }

    /// „Master anhalten“ is offered: a trusted master with the marker off.
    pub fn may_halt(&self) -> bool {
        self.trusted() && self.marker() == Some(MarkerWord::Off)
    }

    /// „Master freigeben“ is offered (and `stop` sent) only when `02` shows the marker on; a foreign marker word or an
    /// unknown patch never gets a `stop`.
    pub fn may_release(&self) -> bool {
        self.trusted() && self.marker() == Some(MarkerWord::On)
    }
}

/// What a pack's own `03` says about its patch, for this connection only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackFinding {
    /// a format-2/3 identity with a supported patch version: its capabilities and its listed name (None: not listed)
    Proven { info: MaintInfo, caps: known::PackCaps, listed: Option<&'static str> },
    /// any other valid `MNT`, or a patch below the supported one: no patch functions; overrides a stored proof
    Negative(Option<MaintInfo>),
    /// silence or an error: says nothing; a stored proof keeps holding
    Undecided,
}

impl PackFinding {
    pub fn of(answer: super::dialog::PackAnswer) -> Self {
        Self::of_in(known::PATCH_BUILDS, answer)
    }

    pub fn of_in(table: &'static [PatchBuild], answer: super::dialog::PackAnswer) -> Self {
        use super::dialog::PackAnswer;
        match answer {
            PackAnswer::Info(info) => match known::pack_caps_of(&info) {
                Some(caps) => PackFinding::Proven { info, caps, listed: known::patch_in(table, info.format, info.base, info.patch, &info.build_id_hex()).map(|b| b.name) },
                None => PackFinding::Negative(Some(info)),
            },
            PackAnswer::OtherMnt => PackFinding::Negative(None),
            PackAnswer::Undecided => PackFinding::Undecided,
        }
    }
}

/// Scenario C': a trusted patched master whose marker is on gets no image that is not a patched build. The original
/// ignores the marker: it would poll right after the flash and the marker word would stay hidden in flash. The user
/// releases the master first (DIP 5–8 OFF keep the bus quiet meanwhile). Every other master is left to the bus check.
pub fn release_first(master: &Master, image: ImageKind) -> bool {
    image != ImageKind::Maintenance && master.may_release()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A format-4 identity; 1.5 and 1.6 as their supported revisions 1.5.3 and 1.6.3, any other version with revision 0.
    fn info(base: [u8; 3], patch: [u8; 2], build_id: [u8; 8]) -> MaintInfo {
        let patch_rev = match patch { [1, 5] => 3, [1, 6] => 3, _ => 0 };
        MaintInfo { format: 4, base, patch, build_id, hard_stop: Some(false), power_rtc: true, pack_cutoff: false, reserved_flags: 0x20, patch_rev, uart0: true }
    }
    fn state(word: u32, slaves: u8) -> Option<MaintState> {
        Some(MaintState { marker_word: word, effective_slaves: slaves })
    }
    /// the build ID of the listed 1.6.3
    const LISTED: [u8; 8] = [0xBA, 0xA0, 0x1E, 0xCD, 0x5A, 0xF4, 0xEB, 0x28];
    const OTHER: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

    fn patched(i: MaintInfo, s: Option<MaintState>) -> Master {
        Master::of(Some(Probe::Info(i))).with_state(s)
    }

    #[test]
    fn the_listed_build_is_named_and_trusted() {
        let m = patched(info([16, 6, 13], [1, 6], LISTED), state(u32::MAX, 3));
        assert!(matches!(m, Master::Patched { patch: Patch { listed: Some("16.06.13 MAINT 1.6.3"), .. }, .. }));
        assert!(m.trusted() && m.may_halt() && !m.may_release());
        assert_eq!(m.caps(), Some(known::PROOF_CAPS));
    }

    #[test]
    fn an_unlisted_build_with_a_supported_patch_version_gets_the_same_configuration() {
        let listed = patched(info([16, 6, 13], [1, 6], LISTED), state(u32::MAX, 3));
        for i in [info([16, 6, 13], [1, 5], OTHER), info([16, 6, 13], [1, 6], OTHER), info([16, 7, 2], [1, 5], OTHER)] {
            let m = patched(i, state(u32::MAX, 3));
            let Master::Patched { patch, .. } = m else { panic!("{m:?}") };
            assert_eq!(patch.listed, None);
            assert_eq!((m.caps(), m.trusted(), m.may_halt(), m.may_release()), (listed.caps(), true, true, false));
        }
    }

    #[test]
    fn a_second_listed_entry_is_configured_like_the_first() {
        static TABLE: [PatchBuild; 2] = [
            PatchBuild { name: "16.06.13 MAINT 1.6.3", base: [16, 6, 13], patch: [1, 6], patch_rev: 3, mnt_format: 4, flags: 0xFE, build_id: "baa01ecd5af4eb28", code_sha256: "a", ehex_sha256: "b" },
            PatchBuild { name: "16.06.14 MAINT 1.6.3", base: [16, 6, 14], patch: [1, 6], patch_rev: 3, mnt_format: 4, flags: 0xFE, build_id: "0102030405060708", code_sha256: "c", ehex_sha256: "d" },
        ];
        let first = Master::of_in(&TABLE, Some(Probe::Info(info([16, 6, 13], [1, 6], LISTED)))).with_state(state(0, 0));
        let second = Master::of_in(&TABLE, Some(Probe::Info(info([16, 6, 14], [1, 6], OTHER)))).with_state(state(0, 0));
        assert!(matches!(second, Master::Patched { patch: Patch { listed: Some("16.06.14 MAINT 1.6.3"), .. }, .. }));
        assert_eq!((second.caps(), second.may_release(), second.may_halt()), (first.caps(), first.may_release(), first.may_halt()));
    }

    #[test]
    fn an_older_build_offers_nothing() {
        // 1.5.1, 1.5.2 and 1.6.2 (format 4), 1.5 (format 3): shown as a patch the app does not use
        for i in [MaintInfo { patch_rev: 1, ..info([16, 6, 13], [1, 5], OTHER) }, MaintInfo { patch_rev: 2, ..info([16, 6, 13], [1, 5], OTHER) }, MaintInfo { patch_rev: 2, ..info([16, 6, 13], [1, 6], OTHER) },
            MaintInfo { format: 3, patch_rev: 0, ..info([16, 6, 13], [1, 5], OTHER) }] {
            let m = patched(i, state(u32::MAX, 3));
            assert!(matches!(m, Master::UnknownPatch(Some(_))), "{m:?}");
            assert!(!m.trusted() && m.caps().is_none());
        }
        // TEMP(maint-1.6): the first 1.6 (V1_6_CUTOFF, flags 06) is supported, by its build
        let id = [0x18, 0xDA, 0xF6, 0x1D, 0x8A, 0x93, 0x67, 0x1D];
        let temp = patched(MaintInfo { format: 3, patch_rev: 0, reserved_flags: 0, pack_cutoff: true, ..info([16, 6, 13], [1, 6], id) }, state(u32::MAX, 3));
        assert_eq!(temp.caps(), Some(Caps { mnt_format: 3, ..known::PROOF_CAPS }));
        assert!(matches!(temp, Master::Patched { patch: Patch { listed: Some("16.06.13 MAINT 1.6"), .. }, .. }));
    }

    #[test]
    fn patch_1_4_format_1_no_0x64_and_unknown_offer_nothing() {
        let old = Master::of(Some(Probe::Info(info([16, 6, 13], [1, 4], OTHER))));
        assert!(matches!(old, Master::UnknownPatch(Some(_))));
        for m in [old, Master::of(Some(Probe::OtherPatch)), Master::of(Some(Probe::No0x64)), Master::of(Some(Probe::Failed)), Master::of(None)] {
            let m = m.with_state(state(0, 0));
            assert!(!m.trusted() && !m.may_halt() && !m.may_release() && m.caps().is_none());
            assert_eq!((m.marker(), m.effective_slaves()), (None, None));
        }
        assert_eq!(Master::of(Some(Probe::Failed)), Master::Unknown);
    }

    #[test]
    fn a_foreign_marker_word_or_no_state_is_neither_halted_nor_released() {
        let m = patched(info([16, 6, 13], [1, 6], LISTED), state(0x1234_5678, 0));
        assert_eq!(m.marker(), Some(MarkerWord::Foreign(0x1234_5678)));
        assert!(!m.may_halt() && !m.may_release());
        let m = patched(info([16, 6, 13], [1, 6], LISTED), None);
        assert!(m.trusted() && !m.may_halt() && !m.may_release());
    }

    #[test]
    fn a_packs_own_03_is_proven_negative_or_undecided() {
        use super::super::dialog::PackAnswer;
        let p = PackFinding::of(PackAnswer::Info(info([16, 6, 13], [1, 6], OTHER)));
        assert!(matches!(p, PackFinding::Proven { caps, listed: None, .. } if caps.write_gate_bit0 && caps.uart1_identity));
        assert!(matches!(PackFinding::of(PackAnswer::Info(info([16, 6, 13], [1, 6], LISTED))), PackFinding::Proven { listed: Some("16.06.13 MAINT 1.6.3"), .. }));
        assert!(matches!(PackFinding::of(PackAnswer::Info(info([16, 6, 13], [1, 4], OTHER))), PackFinding::Negative(Some(_))));
        assert_eq!(PackFinding::of(PackAnswer::OtherMnt), PackFinding::Negative(None));
        assert_eq!(PackFinding::of(PackAnswer::Undecided), PackFinding::Undecided);
    }

    #[test]
    fn a_master_with_its_marker_on_is_released_before_a_non_patched_image() {
        let on = patched(info([16, 6, 13], [1, 6], LISTED), state(0, 0));
        let off = patched(info([16, 6, 13], [1, 6], LISTED), state(u32::MAX, 0));
        assert!(release_first(&on, ImageKind::Original));
        assert!(release_first(&on, ImageKind::Custom));
        assert!(!release_first(&on, ImageKind::Maintenance));
        assert!(!release_first(&off, ImageKind::Original));
        for m in [Master::No0x64, Master::Unknown, Master::UnknownPatch(None)] {
            assert!(!release_first(&m, ImageKind::Original));
        }
    }
}
