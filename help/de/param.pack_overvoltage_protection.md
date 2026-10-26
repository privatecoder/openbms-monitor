---
title: Pack-Überspannungsschutz
short: Der Master meldet diesen Wert als Ladespannung (CVL) an den Wechselrichter. Als Schutz öffnet er den Lade-MOSFET, sofern Funktionsschalter 1, Bit 5 an ist.
related: topic.params
---
Als CVL gilt der Wert immer, auch wenn der Schutz maskiert ist. Als Schutz: Event 2 Bit 5; der Lade-MOSFET öffnet und der SOC springt auf 100 %. Ist Bit 5 aus, löst der Schutz nie aus und setzt den SOC auch nicht auf 100 %.
