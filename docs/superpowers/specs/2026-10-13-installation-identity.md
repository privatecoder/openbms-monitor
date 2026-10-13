# Installations recognised by the master's serial number

Status: shelved 2026-10-13 (r1–r4 reviewed). Superseded by `2026-10-13-simple-flash-flow.md`: no identity gates; the right-bank check uses the BMS serial (String 2) on the pack bus. Writing String 1 stays an option for later.

## Why

Today an installation is found by its endpoint: each endpoint (serial path or TCP address) belongs to exactly one installation, and connecting to it selects that installation. This does not fit reality:

- a USB adapter is portable (bench board today, real bank tomorrow);
- gateways in different installations commonly use the same IP address and port.

Assigning an endpoint to a second installation silently moves it away from the first. The endpoint says how to reach a bus, not which bank is at the other end. The bank is identified by its master.

## Firmware facts (16.06.11–16.06.13, certain from code; measured on a bench board)

- Each BMS holds two persistent 30-byte ASCII strings (flash block 4 at `0x29000`, mirror `0x2A000`):
  - **String 1 = battery serial number** ("Battery_SN" in the vendor tool). Read `0xA2`, write `0xA3`, **pack bus only** (UART0). Also exported on the CAN socket bus: **Modbus FC04 `0x0000`×21, first 22 characters, master only**, and Pylontech `0x60` (first 15 characters).
  - **String 2 = BMS board serial number** ("BMS_SN"). Read `0xA4`, write `0xA5`, pack bus only; not exported anywhere else.
- Vendor boards ship with String 2 set (e.g. `SEPLOS-` + 11 digits) and String 1 = image placeholder `EMU110110S-XXXX-20220120-00001` (unless the pack maker set it).
- Writing (`0xA3`/`0xA5`) stores immediately (rewrites the whole block 4 record; the third 30-byte area there is rewritten unchanged from RAM). No password in the firmware (the vendor tool's "SN login" is a UI lock only).
- On the CAN socket bus the master answers `0xA2`/`0xA4` with RTN 04. On the pack bus the master answers only while it does not poll (maintenance marker active, or a single pack without slaves).

## Proposal

### 1. Identity

- **Installation identity = String 1 of its master**, as read over the CAN socket bus (Modbus FC04 `0x0000`×21, 22 visible characters). Stored per installation in a new table `installation_identity(installation PRIMARY KEY, serial TEXT NOT NULL UNIQUE, at)`; removed identities go to `installation_identity_archive`.
- A value is **usable as identity** only if all hold:
  - not truncated: the value ends inside the 22 visible bytes (padding follows), so it is **at most 21 characters**;
  - strict format: printable ASCII `0x21–0x7E` only (no lossy decoding), length 4–21;
  - not the placeholder (prefix `EMU110110S-XXXX`), not empty, not erased (`0xFF`);
  - unique among stored identities.
- Longer or otherwise unusable values are shown as information only; such a master has no identity until String 1 is rewritten (point 3).
- The identity lives in the master board's flash: a replaced master board brings its own String 1 (usually the placeholder, but possibly any value). A board swap is handled by point 6 (identity change); a mismatch is never accepted silently.
- Firmware before 16.06.11 (Modbus exception) → no identity possible; such installations work without identity (rule 4c).

### 2. Recognition on connect

On every connect over a CAN socket endpoint the app reads the master's String 1 once (already done for display):
- usable and known:
  - if the shown installation is that one → nothing to do;
  - if the shown installation has **no open work** → switch to the recognised one with a notice „Erkannt: Anlage X (Seriennummer …)“; the switch invalidates phase state (inverter confirmation, measurement, scans, End progress) as an explicit switch does;
  - otherwise (shown installation has open work) → no automatic switch; banner „An diesem Anschluss hängt Anlage X“ with an explicit switch offer.
- usable and unknown → ask: „Neue Anlage anlegen“ (name dialog) or „Einer bestehenden Anlage ohne Seriennummer zuordnen“ (only installations without identity and without open work). Never assigned automatically.
- unusable → explicit picker selection as today, hint „Master ohne eindeutige Seriennummer“ with the offer from 3.
- read failed → no change of selection; a previously recognised identity is never treated as fresh.

### 3. Giving a master an identity (write String 1)

A normal **mutating maintenance action** with all its gates:
- Preconditions: an open marker session **or** a confirmed DIP session (DIP 5–8 OFF) **or** a proven single pack; fresh master status (as for measure/flash); 2 s bus quiet and `0x47` from address 0 on the pack bus; no open block; fresh inverter confirmation; the installation has no identity yet, or the user explicitly replaces it (point 6).
- Proposal: String 2 (read with `0xA4` at address 0) if it passes the strict format of point 1 (printable ASCII, ≤ 21 characters, not a placeholder, not already stored for another installation); otherwise a generated value `OBM-` + 12 random base32 characters (16 characters). The user can choose the generated value even if String 2 qualifies.
- Confirmation shows old and new value and the side effect (String 1 is also what the inverter sees via Pylontech `0x60`, first 15 characters).
- **Journal first:** under `port_busy`, before sending, the app stores `identity_change(installation, old, new, started)` (new table). It counts as open work (banner „Seriennummer wird geändert / Ergebnis unklar“).
- Write `0xA3` unicast to address 0 on the pack bus. Read back in phases (works with two adapters and with one adapter that is moved between the sockets; only one port is open at a time in one-adapter mode):
  1. **Pack bus readback** (`0xA2` at address 0, same connection, immediately): new value → journal state `written` (stored), old value → journal closed as „nicht geschrieben“, other / no answer → journal stays `unclear`.
  2. **CAN socket readback** (Modbus `0x0000`, at once with two adapters, after the move with one adapter): new value → in one transaction store the identity, archive the old one, close the journal.
  - While the journal is `written`, the gates accept the new value on the pack bus and expect it on the CAN socket; the first successful Modbus read with the new value completes the journal; the old value on the CAN socket → `unclear`.
  - Abort, crash or power loss in any phase: the journal stays open (open work, banner). On the next connect/check the app reads what it can (both buses as available) and offers a guided completion: new value on both → success; old value on both → „nicht geschrieben“; otherwise stays blocked with read-only diagnosis.
- Only String 1 is ever written. String 2 is never written.

### 4. Endpoints, resolver and the gate

a. **Endpoints are many-to-many.** New table `installation_endpoints_v2(endpoint, installation, PRIMARY KEY(endpoint, installation))`, filled once from the old table; the control endpoint stays in `installations.control` and also counts as a mapping. Saving never moves an endpoint away from another installation; `endpoint-in-use` disappears.

b. **Resolver.** `installation_of(endpoint)` (single result) is replaced by:
   - the **explicitly selected installation id** from the UI, passed to every command (`firmware_start`, measure, End, start/stop, identity write);
   - `candidates(endpoint)`: all installations that list the endpoint (as pack bus endpoint or control).
   The implicit creation in `firmware_start` (`installation_for`) is allowed only when the endpoint belongs to no installation at all and the UI shows „keine Anlage“; never for a shared or known endpoint.

c. **Gate before every mutating bus action** (start/stop, measure, flash, End, identity write), inside `port_busy`, **before** `prepare_flash` or any `update-interrupted` block is written. Each bus proves itself; nothing from one bus is carried to the other except as described in 3:
   - The selected installation must be one of `candidates(endpoint)`.
   - **CAN socket actions** (start/stop, master check before a write, End's control steps): if the installation has an identity, Modbus String 1 read right before the action must equal it. Different → `installation-mismatch`; not readable → `identity-unreadable`. No fallback.
   - **Pack bus actions** (measure, flash, identity write): whenever address 0 answers on the pack bus (it does in every maintenance path: marker active, DIP 5–8 OFF, single pack — the quiet-bus check already requires `0x47` from address 0), its fresh `0xA2` must equal the stored identity. Different, no answer to `0xA2` or an unusable value → refuse. This binds the pack-bus cable to the installation even with one adapter, without a UART1 read in the same dialog.
   - **Installation without identity:** every mutating action requires an explicit **physical confirmation** for this endpoint and this one operation („Dieses Kabel steckt an Anlage X“, installation name shown), stored with the operation, plus: refuse if any other candidate has open work, and refuse if the master answers with a usable String 1 that belongs to another installation. The help and the dialog name this as the remaining assumption and recommend giving the master an identity (point 3).
   - **Reading versus adopting.** Reading the master (0x64 status, serial numbers, 0x51) and showing the raw result is never gated. But **adopting** a read into the selected installation — writing `master_state`, clearing a session failure, clearing a `master-unclear` block, marking End progress, completing an identity journal — is a state change of that installation and needs the same proof as a mutating action: identity equal (installation with identity) or the operation-bound physical confirmation plus the candidate/open-work checks (installation without identity). Every internal `check()` call follows this rule. Without the proof the result is shown as „gelesen, nicht übernommen (Anlage nicht bestätigt)“ and nothing of the installation changes.

d. **Unreadable identity is not bypassed.** A master whose application does not answer (e.g. hanging in its bootloader) cannot be flashed by the app anyway (the flash preflight needs `0x51`/`0x47`); this stays isolation / lab recovery outside the app, as in the maintenance design. A slave in its bootloader is recovered through its own master, whose identity is readable.

### 5. Migration

- Existing installations keep their endpoints (copied to v2) and start without identity.
- On the first connect after the update, a usable String 1 that is unknown is offered for the shown installation if that installation lists the endpoint, has no identity and no open work („Seriennummer … für Anlage X übernehmen?“); otherwise the normal flow of 2.

### 6. Removing or changing an identity

- „Identität entfernen“ (store only, no device write): only without any open work, checked by **one central function** (`holds_identity`: the `holds_ports` set plus identity journal and `marker-foreign`), under `port_busy`, in one transaction; the old value goes to `installation_identity_archive`. Deleting an installation uses the same check and archives its identity.
- **Identity change** (e.g. after a master board swap, or to replace a long value) is the **only** operation that accepts a device value different from the stored identity, as a narrow, journaled exception to the equality gate of 4c:
  - Entry only through the DIP path: the user sets DIP 5–8 of the master OFF physically and confirms the physical connection to installation X (operation-bound, as for installations without identity). No `start`/`stop` is sent.
  - Other installations' work, other blocks, sector notes and `marker-foreign` of X refuse; the installation's own open session (marker or DIP) is allowed.
  - Fresh pack bus check: 2 s quiet, `0x47` from address 0, `0xA2` and `0xA4` read. The dialog shows the stored identity, the value currently read from the device and the new value, and asks for the reason (board swapped / value replaced).
  - Either **adopt** the current device value (if usable per point 1 and not stored elsewhere) or **write** a new one with the journaled `0xA3` flow of point 3.
  - The journal records old, device and new value and the reason. Until it completes (pack bus readback and CAN socket readback, or for adoption: a fresh Modbus read showing the adopted value) the old identity stays the gate reference for everything except this operation; then old identity and reason are archived and the new one becomes the reference, in one transaction.

### 7. UI and help

- Picker entries show the identity (shortened) next to the name; the setup form shows it read-only with „Identität setzen …“ / „Identität entfernen“.
- Help (`tools/gen_help.py`): how installations are recognised, the two serial numbers, the 21-character rule, what writing String 1 changes, the recovery confirmation. Generic examples only.

## Out of scope

- Detecting the exact patch level of original firmware (only MAINT 1.1 reports it).
- Per-pack identity of slaves (pack swap detection) — possible later via `0xA2`/`0xA4` on the pack bus.

## Changes after r2

- Installations without identity: explicit physical confirmation per operation (4c).
- Pack bus `0xA2` check mandatory wherever address 0 answers (4c); no `0x47` fallback.
- `0xA3` readback in phases for one adapter (3); each bus proves itself (4c).
- Bootloader-hanging master: outside the app (4d).
- Identity change during the own session (6).

## Changes after r3

- Reading is free, adopting a read into an installation needs the identity proof or the physical confirmation (4c), including every internal master check.
- Identity change as the single narrow exception to the equality gate, entered through the DIP path (6).

## Answers to the r1 questions (adopted)

1. Gate: no fallback with a stored identity; resolver with explicit installation id and candidates; open work of all candidates counts when there is no identity (point 4).
2. Writing String 1: journal before the write, strict ≤ 21-character values, uniqueness checked before writing, normal maintenance gates including the DIP path (point 3).
3. Unreadable identity: mutating actions refused; read-only stays; a master without answering application is isolation / lab recovery outside the app (point 4d).
