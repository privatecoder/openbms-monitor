//! What the app makes of the master's answer, and the one rule for flashing the master itself (design:
//! docs/superpowers/specs/2026-10-13-simple-flash-flow.md, scenario C'). Pure, so every case is tested.

use super::dialog::Probe;
use crate::known::{self, ImageKind};
use openbms_proto::{MaintInfo, MaintStatus, MarkerWord};

/// The master as read over the CAN socket bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Master {
    /// MAINT 1.1 with a build ID from the catalog
    Maint { info: MaintInfo, release: &'static str },
    /// an `MNT` answer with a build ID the catalog does not know: shown, never started or stopped
    UnknownBuild(MaintInfo),
    /// RTN 05 to `03`, a valid `02`: MAINT 1.0 compatible
    Legacy(MaintStatus),
    /// RTN 04: no 0x64 (original or another build)
    No0x64,
    /// no valid answer, or no CAN socket endpoint
    Unknown,
}

impl Master {
    pub fn of(probe: Option<Probe>) -> Self {
        match probe {
            Some(Probe::Info(info)) => match known::maint_build(&info.build_id_hex()) {
                Some(r) => Master::Maint { info, release: r.name },
                None => Master::UnknownBuild(info),
            },
            Some(Probe::Legacy(s)) => Master::Legacy(s),
            Some(Probe::No0x64) => Master::No0x64,
            Some(Probe::Failed) | None => Master::Unknown,
        }
    }

    /// The marker as the master reports it; None without 0x64.
    pub fn marker(&self) -> Option<MarkerWord> {
        match self {
            Master::Maint { info, .. } | Master::UnknownBuild(info) => Some(info.marker()),
            Master::Legacy(s) => Some(if s.marker { MarkerWord::On } else { MarkerWord::Off }),
            Master::No0x64 | Master::Unknown => None,
        }
    }

    /// The slave count the master works with (0 while its marker holds the bus quiet, or with DIP 5–8 OFF).
    pub fn effective_slaves(&self) -> Option<u8> {
        match self {
            Master::Maint { info, .. } | Master::UnknownBuild(info) => Some(info.effective_slaves),
            Master::Legacy(s) => Some(s.effective_slaves),
            Master::No0x64 | Master::Unknown => None,
        }
    }

    /// A known maintenance build: the app may send it start and stop.
    pub fn trusted(&self) -> bool {
        matches!(self, Master::Maint { .. } | Master::Legacy(_))
    }

    /// „Master anhalten“ is offered: a trusted build with the marker off.
    pub fn may_halt(&self) -> bool {
        self.trusted() && self.marker() == Some(MarkerWord::Off)
    }

    /// „Master freigeben“ is offered (and `stop` sent) only when 0x64 shows the marker on; a foreign marker
    /// word or an unknown build never gets a `stop`.
    pub fn may_release(&self) -> bool {
        self.trusted() && self.marker() == Some(MarkerWord::On)
    }
}

/// Scenario C': a trusted patched master whose marker is on gets no image that is not a maintenance build. The
/// original ignores the marker: it would poll right after the flash and the marker word would stay hidden in
/// flash. The user releases the master first (DIP 5–8 OFF keep the bus quiet meanwhile). Every other master is
/// left to the bus check.
pub fn release_first(master: &Master, image: ImageKind) -> bool {
    image != ImageKind::Maintenance && master.may_release()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(marker_word: u32, slaves: u8, build_id: [u8; 8]) -> MaintInfo {
        MaintInfo { base: [16, 6, 13], patch: [1, 1], marker_word, effective_slaves: slaves, build_id }
    }
    const KNOWN: [u8; 8] = [0xFE, 0x38, 0x19, 0x85, 0xBF, 0xFF, 0x8A, 0xD0];
    const OTHER: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

    #[test]
    fn the_build_id_decides_trust() {
        let m = Master::of(Some(Probe::Info(info(u32::MAX, 3, KNOWN))));
        assert!(matches!(m, Master::Maint { release: "16.06.13 MAINT 1.1", .. }));
        assert!(m.trusted() && m.may_halt() && !m.may_release());
        let u = Master::of(Some(Probe::Info(info(0, 0, OTHER))));
        assert!(matches!(u, Master::UnknownBuild(_)));
        assert!(!u.trusted() && !u.may_halt() && !u.may_release());
        assert_eq!(u.marker(), Some(MarkerWord::On));
    }

    #[test]
    fn legacy_no_0x64_and_unknown() {
        let on = Master::of(Some(Probe::Legacy(MaintStatus { marker: true, effective_slaves: 0 })));
        assert!(on.trusted() && on.may_release() && !on.may_halt());
        assert_eq!(on.effective_slaves(), Some(0));
        for m in [Master::of(Some(Probe::No0x64)), Master::of(Some(Probe::Failed)), Master::of(None)] {
            assert!(!m.trusted() && !m.may_halt() && !m.may_release());
            assert_eq!((m.marker(), m.effective_slaves()), (None, None));
        }
        assert_eq!(Master::of(Some(Probe::Failed)), Master::Unknown);
    }

    #[test]
    fn a_foreign_marker_word_is_neither_halted_nor_released() {
        let m = Master::of(Some(Probe::Info(info(0x1234_5678, 0, KNOWN))));
        assert_eq!(m.marker(), Some(MarkerWord::Foreign(0x1234_5678)));
        assert!(!m.may_halt() && !m.may_release());
    }

    #[test]
    fn a_master_with_its_marker_on_is_released_before_a_non_maintenance_image() {
        let on = Master::of(Some(Probe::Info(info(0, 0, KNOWN))));
        let off = Master::of(Some(Probe::Info(info(u32::MAX, 0, KNOWN))));
        assert!(release_first(&on, ImageKind::Original));
        assert!(release_first(&on, ImageKind::Custom));
        assert!(!release_first(&on, ImageKind::Maintenance));
        assert!(!release_first(&off, ImageKind::Original));
        // nothing to release: left to the bus check (DIP way)
        for m in [Master::No0x64, Master::Unknown, Master::of(Some(Probe::Info(info(0, 0, OTHER))))] {
            assert!(!release_first(&m, ImageKind::Original));
        }
    }
}
