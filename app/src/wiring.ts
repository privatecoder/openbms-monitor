import { useEffect, useState } from "react";
import { api, type Bus } from "./api";
import { useSettings } from "./installData";

/** Where the inverter is connected (installation setting "wiring"; with „Nur ansehen“ this session's choice). The
 *  backend's bus rule (`wiring.rs`) keeps every request of the app off the bus the inverter polls; RS485-1/2 listens
 *  first in any case, so a Seplos master polling its slaves is heard before anything is sent. */
export const INVERTERS = ["can", "uart1-master", "uart0", "uart1-each", "none"] as const;
export type Inverter = (typeof INVERTERS)[number];

/** The bus the inverter polls (`Inverter::polls`). */
export const pollsBus = (i: Inverter | null): Bus | null => (i === "uart0" ? "pack" : i === "uart1-master" || i === "uart1-each" ? "can" : null);

const inverterOf = (v: unknown): Inverter | null => {
  const i = (v as { inverter?: unknown } | null)?.inverter;
  return INVERTERS.includes(i as Inverter) ? (i as Inverter) : null;
};

// „Wechselrichter pausiert“: this session only, for the installation it was said for
let paused: { scope: string | null } | null = null;
const subs = new Set<() => void>();

export function useWiring(scope: string | null) {
  const { settings, loaded, save } = useSettings(scope);
  const [, tick] = useState(0);
  useEffect(() => { const f = () => tick((n) => n + 1); subs.add(f); return () => { subs.delete(f); }; }, []);
  const inverter = inverterOf(settings.wiring);
  const isPaused = paused !== null && paused.scope === scope;
  return {
    inverter, loaded, paused: isPaused,
    setInverter: (i: Inverter) => save("wiring", { inverter: i }),
    setPaused: (p: boolean) => { paused = p ? { scope } : null; subs.forEach((f) => f()); },
  };
}

/** Keeps the backend's bus rule on the installation shown (App, once). */
export function useWiringSync(scope: string | null) {
  const w = useWiring(scope);
  useEffect(() => { api.setWiring(w.inverter, w.paused).catch((e) => console.error("wiring:", e)); }, [w.inverter, w.paused]);
}

const REFUSALS = ["wiring-unset", "bus-inverter", "bus-polled", "bus-traffic", "bus-unknown"] as const;
export type Refusal = (typeof REFUSALS)[number];

/** A refusal of the bus rule in any error the backend gives: `{"code":…}` of a connect, or "send refused: …" inside the
 *  text of another action. */
export function refusalOf(e: unknown): Refusal | null {
  const text = typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e ?? "");
  return REFUSALS.find((r) => text.includes(`"${r}"`) || text.includes(`send refused: ${r}`)) ?? null;
}
