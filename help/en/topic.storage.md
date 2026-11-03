---
title: Data & storage
short: Shows on the About page what the app's data folder holds and frees space by hand.
related: topic.recording, topic.installations
---
- **In daily use** the recording's retention keeps the size small: every poll stays for the chosen days, then minute values. Freeing space by hand is the exception.
- **Clear recording** (one installation or all): deletes the recorded values and events. Installations, boards, parameter states and the BMS archive stay.
- **Delete migration backups:** before every change of its databases the app keeps a copy. Without them there is no way back to the state before.
- **Empty data folder:** deletes the recording, parameter states, parameter files, firmware logs, exports, migration backups and import leftovers; the app restarts and empties at the start, before it opens anything. The installations with their boards, places, layout and notifications stay, as do the BMS archive (the BMS cannot deliver older records again), the packs' safety states and imports that can still be resumed. If a step fails, the app deletes nothing more and offers “Try the rest again” or “Drop”. While an import is open it deletes nothing at all.
- **Backup first:** “Back up as ZIP” puts one ZIP file per installation with data into a folder you choose outside the data folder, like “Export installation” with recording; existing files stay untouched, “Import installation” brings the data back. From the backup until the delete the recording pauses, so nothing unsaved is deleted. If a backup fails, nothing is deleted. Parameter files, parameter states and exports are not in the ZIP: copy what you want to keep out of the folder first.
- **Space:** deleted values free their space only once the file is shrunk, which needs free space on the drive for a while. If that fails, the view says so, and new values reuse the space inside the file.
