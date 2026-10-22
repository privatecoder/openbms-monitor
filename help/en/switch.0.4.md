---
title: Lock of the extended off
short: Patched firmware with the pack cutoff (MAINT 1.6), applies to a direct off via the RS485-1/2 bus and the CAN socket: 1 (on, as with the old value FF) locks the extended off, a direct off stays the soft off as before. 0 (off) releases the extended off: switching this pack off directly then blocks the charge and discharge path including current limiter and pre-charge, and the lock survives restarts. The master's on broadcasts and parameter imports do not release it; to release it, switch this pack on directly.
related: topic.switches
---

