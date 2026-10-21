---
title: Simple off (no stored pack cutoff)
short: Patched firmware with the pack cutoff (MAINT 1.6): 1 (on, as with the old value FF) keeps a direct off the soft off as before. 0 (off) switches the extended off on: switching this pack off directly then blocks the charge and discharge path including current limiter and pre-charge, and the lock survives restarts. The master's on broadcasts and parameter imports do not release it; to release it, switch this pack on directly.
related: topic.switches
---

