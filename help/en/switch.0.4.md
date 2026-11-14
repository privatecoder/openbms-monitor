---
title: Extended off
short: Patched firmware with the pack cutoff (EMU1101-EXT Full), applies to “Set pack lock (addressed off)” via the RS485-1/2 bus or the CAN socket: 1 (on, as with the old value FF) switches the extended off on: a direct off then locks the charge and discharge path including the current limiter and precharge, and the lock survives restarts. 0 (off): a direct off stays the previous soft off. On broadcasts of the master and parameter imports do not release the lock; switch this pack on directly to release it.
related: topic.switches
---

