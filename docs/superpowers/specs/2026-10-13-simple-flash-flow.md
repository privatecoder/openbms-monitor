# Simple flash flow (replaces the guided maintenance panel)

Status: approved r2, 2026-10-13 (r1 reviewed: `.orch/reviewer-codex-6-sol-simple-flash-r1.md` in the knowledge base; approved by the user). Replaces the gating parts of `2026-10-12-maintenance-mode-design.md`, `2026-10-13-maintenance-panel-flow.md`, and the not yet implemented drafts `2026-10-13-maint-1-1.md` (gating parts) and `2026-10-13-installation-identity.md` (gate parts).

## Why

User feedback after lab use: the panel is a maze of six steps, checkboxes and dependencies (inverter checkbox, pack count, two-pass slave search, single-pack proof, sessions, installation gates). Users press and tick everything until flashing is finally allowed. The vendor tool (BatteryMonitor) just flashes. Goal: **as simple as possible, with only the checks that prevent real damage.**

## The physics (what actually matters)

- Flashing runs on the pack bus (RS485-1/2, 19200 baud): `0x45` restart, then the bootloader dialog. It only works if **nobody else talks on the pack bus**.
- A master with slaves (DIP 5–8 ≠ 0) polls the pack bus every second. Then the bus is not quiet, and a polling master ignores `0x45` itself.
- The master stops polling when **DIP 5–8 are OFF** (any firmware, read cyclically) or, with the patched firmware, when the **maintenance marker** is set (`0x64` start over the CAN socket, persistent, survives flash and power loss — measured).
- A single pack has no slaves, so its bus is quiet anyway.
- Failure that matters: an interrupted transfer after `W F E` can leave a pack without a valid application. The bootloader stays; flashing the same pack again is the recovery (as far as known). The app must show this clearly and not lose it.
- While the master does not poll it reports only its own pack to the inverter (lower DCL/CCL); during the master's own flash CAN stops. Information only.

## Scenarios

| # | Bank / firmware | Target | What the user does | What the app does |
|---|---|---|---|---|
| A | single pack, any firmware | the pack | connect RS485-1/2 | bus quiet → flash |
| B | multi-pack, original master | slave or master | set **DIP 5–8 of the master OFF** | waits until the bus is quiet → flash any pack → afterwards: „DIP 5–8 zurückstellen“, then shows polling again |
| C | multi-pack, patched master (MAINT) | slave or master | connect the CAN socket too, press **„Master anhalten“** | marker start, waits for `0` slaves and quiet bus → flash any pack → afterwards **„Master freigeben“** (marker stop), then shows polling again |
| C' | multi-pack, patched master, new master image is **not** a maintenance build (original or other custom) | master | set **DIP 5–8 of the master OFF**, press **„Master freigeben und flashen“**, set DIP back | shows this order only for this case (amended 2026-10-14 after the lab run): the one button sends the marker stop while the patched image still runs, requires a fresh read with the marker off, listens 2.5 s for a quiet bus (refuses with „DIP 5–8 auf OFF“ if the master polls again), then flashes. A master without a set marker is flashed without the release step. Reason: the original ignores the marker, would poll right after the flash (the UART0 post-check fails although the flash may be fine) and the marker word would stay hidden in flash. |

## The panel (one screen, no step numbers)

1. **Status line (live, refreshed every 2 s while the panel is open):**
   - „Bus ruhig“ (green) / „Master fragt Slaves ab“ (amber) / „kein Anschluss“;
   - with a CAN socket: master firmware (`16.06.13 MAINT 1.1` from `0x64/03`, „MAINT-1.0-kompatibel“, or „Original 16.06“) and marker/slave state.
2. **„Bus ruhig machen“ (only shown while the bus is not quiet):**
   - patched master reachable: button **„Master anhalten“**;
   - otherwise: text „Am Master DIP 5–8 auf OFF stellen“ (+ picture). The status line turns green by itself.
3. **Packs (shown when the bus is quiet):** list of addresses that answer on the pack bus (one scan, `0x51`), each with version and a **„Flashen“** button; one image picker above the list. The master row (address 0) carries the C' hint when it applies.
   - The „Flashen“ button itself re-checks quiet bus (2 s) and the target's answer right before `0x45`; nothing to tick.
   - One information line above the list: „Während der Wartung meldet der Master dem Wechselrichter nur sein eigenes Pack.“
4. **„Fertig“ (shown after at least one flash or when the marker is on):**
   - patched master: button **„Master freigeben“** (marker stop);
   - DIP path: text „DIP 5–8 am Master zurückstellen“;
   - the status line then shows „Master fragt N Slaves ab“ as confirmation.

## What stays persistent

- **Marker on:** banner „Wartungsmodus aktiv: Master fragt keine Slaves ab“ with „Master freigeben“, until the marker reads off.
- **Interrupted flash (after `W F E` or unclear):** the pack may sit in its bootloader. Bootloader frames carry **no pack address**, so a silent bootloader pack could be written along with the next flash of a **different** pack on the same bus. Therefore:
  - banner „Pack X: Update abgebrochen, Pack hängt eventuell im Bootloader“;
  - flashing **other** addresses on that pack bus endpoint is refused (`bootloader-pending`); reading, DIP/marker actions and other endpoints stay free;
  - cleared automatically when pack X answers `0x51` again, or by the user's explicit confirmation „Pack X ist abgeklemmt oder repariert“;
  - honest text: re-flashing a pack that hangs in its bootloader is not possible in the app yet (the updater needs the application to answer `0x51`/`0x47` before `0x45`; a bootloader resume needs a lab test first).
- Last flash log per pack (exists).

## What is removed

- Inverter checkbox, pack/slave count input, two-pass slave search, single-pack proof, maintenance sessions, End steps, `marker-unexplained` lock, `master-unclear` / `update-unclear` locks, sector notes, the one-adapter mark, installation gates.
- Installations stay as an address book: name, CAN socket endpoint, pack bus endpoint, stored master BMS serial. Endpoints are many-to-many (no moving, no `endpoint-in-use`).

## Kept checks (the only ones)

1. Bus quiet for 2 s right before every `0x45` (inside the flash button).
2. Target answers `0x51` (and `0x47`, existing preflight) at its address right before `0x45`.
3. Image file passes the format checks; unknown images need „Eigene Firmware“ (exists).
4. `stop` (marker off) only on the user's button (also „Master freigeben und flashen“ in C'), only when `0x64` shows the marker on; never automatic.
5. `bootloader-pending` (above).
6. **Right bank:** when an installation is selected and the bus is quiet, the app reads the master's BMS serial (String 2, `0xA4` at address 0 on the pack bus) before a flash. The installation stores it the first time (no dialog); afterwards a different value refuses the flash with „An diesem Anschluss hängt eine andere Anlage (BMS-SN …)“ and an offer „Diese Anlage hat einen neuen Master: Seriennummer übernehmen“. No answer to `0xA4` (e.g. a single pack whose address-0 pack is the target itself and answers, or firmware without 0xA4): flash allowed, the line shows „Anlage nicht geprüft“. Without a selected installation nothing is checked.
7. Implementation rule: the live status timer and scans never send on the pack bus while a flash runs (existing `port_busy` / `stop_monitor`); the status line takes the flash progress then.

## MAINT 1.1

Included (from `2026-10-13-maint-1-1.md`, display/identity parts only):
- catalog: "16.06.13 MAINT 1.1" (code SHA-256 `734a4a4c73abbf991b2616b50958aff6c05cdcca93ebf3781867a179194e38f1`, build ID `fe381985bfff8ad0`), the existing entry renamed "16.06.13 MAINT 1.0";
- master read: `0x64/03` first (22-byte answer), RTN 05 → `0x64/02` (MAINT-1.0-kompatibel), RTN 04 → „0x64 nicht unterstützt“; no answer → unknown;
- the status line shows the reported version, marker word and effective slaves; a marker word other than `FFFFFFFF`/`00000000` is shown as „Marker-Sektor mit unbekanntem Inhalt“ and „Master freigeben“ is not offered (MAINT 1.1 refuses it anyway);
- an `MNT` answer with an unknown build ID is shown as „Unbekannter Wartungs-Build“; „Master anhalten/freigeben“ is not offered for it.

