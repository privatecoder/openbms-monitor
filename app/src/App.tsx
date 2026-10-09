import { useEffect, useState } from "react";
import { api, type Bus, type DeviceInfo, type Telemetry } from "./api";

// First skeleton: connect, scan, show telemetry of the selected pack.
// Real screens (dashboard, parameters, help) follow the spec in BatteryMonitor-Rebuild.md.
export default function App() {
  const [ports, setPorts] = useState<string[]>([]);
  const [kind, setKind] = useState<"serial" | "tcp">("tcp");
  const [target, setTarget] = useState("");
  const [bus, setBus] = useState<Bus>("can");
  const [connected, setConnected] = useState(false);
  const [packs, setPacks] = useState<[number, DeviceInfo][]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [telemetry, setTelemetry] = useState<Telemetry | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => { api.listPorts().then(setPorts).catch(() => setPorts([])); }, []);

  useEffect(() => {
    if (!connected || selected === null) return;
    let alive = true;
    const tick = async () => {
      try { const t = await api.telemetry(selected); if (alive) { setTelemetry(t); setError(null); } }
      catch (e) { if (alive) setError(String(e)); }
    };
    tick();
    const id = setInterval(tick, 2000);
    return () => { alive = false; clearInterval(id); };
  }, [connected, selected]);

  const run = async (f: () => Promise<void>) => {
    setBusy(true);
    try { await f(); setError(null); } catch (e) { setError(String(e)); } finally { setBusy(false); }
  };

  const connect = () => run(async () => {
    await api.connect(kind === "serial" ? { kind, path: target, bus } : { kind, addr: target, bus });
    setConnected(true);
    const found = await api.scan();
    setPacks(found);
    setSelected(found[0]?.[0] ?? null);
  });

  const cells = telemetry?.cell_voltages ?? [];
  const max = Math.max(...cells), min = Math.min(...cells);

  return (
    <div className="flex h-full bg-zinc-50 text-zinc-900 dark:bg-zinc-950 dark:text-zinc-100">
      <aside className="w-64 shrink-0 border-r border-zinc-200 p-4 dark:border-zinc-800">
        <h1 className="mb-6 text-lg font-semibold">OpenBMS Monitor</h1>
        <div className="space-y-2 text-sm">
          <div className="flex gap-1">
            {(["tcp", "serial"] as const).map((k) => (
              <button key={k} onClick={() => setKind(k)}
                className={`flex-1 rounded-md px-2 py-1 ${kind === k ? "bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900" : "bg-zinc-200 dark:bg-zinc-800"}`}>
                {k === "tcp" ? "TCP" : "Serial"}
              </button>
            ))}
          </div>
          {kind === "serial" ? (
            <select value={target} onChange={(e) => setTarget(e.target.value)} className="w-full rounded-md bg-zinc-200 px-2 py-1 dark:bg-zinc-800">
              <option value="">Port wählen …</option>
              {ports.map((p) => <option key={p}>{p}</option>)}
            </select>
          ) : (
            <input value={target} onChange={(e) => setTarget(e.target.value)} placeholder="192.168.1.10:4196"
              className="w-full rounded-md bg-zinc-200 px-2 py-1 dark:bg-zinc-800" />
          )}
          <select value={bus} onChange={(e) => setBus(e.target.value as Bus)} className="w-full rounded-md bg-zinc-200 px-2 py-1 dark:bg-zinc-800">
            <option value="can">CAN-Buchse (9600 Bd)</option>
            <option value="pack">RS485-1/2 (19200 Bd)</option>
          </select>
          <button disabled={busy || !target} onClick={connect}
            className="w-full rounded-md bg-emerald-600 px-2 py-1.5 font-medium text-white disabled:opacity-40">
            {busy ? "Suche Packs …" : connected ? "Neu verbinden" : "Verbinden"}
          </button>
        </div>
        <nav className="mt-6 space-y-1 text-sm">
          {packs.map(([a, d]) => (
            <button key={a} onClick={() => setSelected(a)}
              className={`block w-full rounded-md px-2 py-1 text-left ${selected === a ? "bg-zinc-200 dark:bg-zinc-800" : ""}`}>
              Pack {a} <span className="text-zinc-500">· {d.device_name} · {d.firmware_version}</span>
            </button>
          ))}
        </nav>
      </aside>
      <main className="flex-1 overflow-auto p-6">
        {error && <div className="mb-4 rounded-md bg-red-100 px-3 py-2 text-sm text-red-800 dark:bg-red-950 dark:text-red-200">{error}</div>}
        {!telemetry ? (
          <p className="text-zinc-500">Verbinden und einen Pack wählen.</p>
        ) : (
          <div className="space-y-6">
            <div className="grid grid-cols-2 gap-4 md:grid-cols-4">
              {[
                ["SOC", `${telemetry.soc.toFixed(1)} %`],
                ["Spannung", `${telemetry.pack_voltage.toFixed(2)} V`],
                ["Strom", `${(telemetry.current || (telemetry.idle_current_ma ?? 0) / 1000).toFixed(3)} A`],
                ["Zyklen", `${telemetry.cycles}`],
              ].map(([k, v]) => (
                <div key={k} className="rounded-xl border border-zinc-200 p-4 dark:border-zinc-800">
                  <div className="text-xs uppercase tracking-wide text-zinc-500">{k}</div>
                  <div className="mt-1 text-2xl font-semibold tabular-nums">{v}</div>
                </div>
              ))}
            </div>
            <div className="rounded-xl border border-zinc-200 p-4 dark:border-zinc-800">
              <div className="mb-3 text-sm text-zinc-500">Zellspannungen · Δ {((max - min) * 1000).toFixed(0)} mV</div>
              <div className="grid grid-cols-8 gap-2">
                {cells.map((v, i) => (
                  <div key={i} className={`rounded-md p-2 text-center text-sm tabular-nums ${v === max ? "bg-amber-100 dark:bg-amber-900/40" : v === min ? "bg-sky-100 dark:bg-sky-900/40" : "bg-zinc-100 dark:bg-zinc-900"}`}>
                    <div className="text-xs text-zinc-500">{i + 1}</div>{v.toFixed(3)}
                  </div>
                ))}
              </div>
            </div>
          </div>
        )}
      </main>
    </div>
  );
}
