import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Cable } from "lucide-react";
import { Sidebar } from "./views/Sidebar";
import { MaintBanner } from "./views/MaintBanner";
import { Firmware } from "./views/Firmware";
import { Dashboard } from "./views/Dashboard";
import { PackDetail } from "./views/PackDetail";
import { Cells } from "./views/Cells";
import { Diagnosis } from "./views/Diagnosis";
import { ParametersView } from "./views/Parameters";
import { History } from "./views/History";
import { HelpProvider } from "./help";
import { useLiveData } from "./store";
import { useMaint } from "./maintStore";
import { api, isDemo, type Bus } from "./api";
import type { Monitor } from "./maint";

const demoPack = isDemo ? new URLSearchParams(location.search).get("pack") : null;
const demoView = isDemo ? new URLSearchParams(location.search).get("view") : null;
// views listed in the sidebar only while "show maintenance functions" is on
const MAINT_VIEWS = ["diag", "params", "firmware"];

export default function App() {
  const { t } = useTranslation();
  const live = useLiveData();
  const maint = useMaint();
  const [connected, setConnected] = useState(false);
  // lost or closed in the sidebar: the firmware view asks for the inverter confirmation again
  const [disconnects, setDisconnects] = useState(0);
  const [site, setSite] = useState("");
  const [bus, setBus] = useState<Bus | null>(null);
  const [view, setView] = useState(demoView ?? "dashboard");
  const [flashing, setFlashing] = useState(false);
  // "End" in the banner: the firmware view scrolls to the panel's End section (also when it is already open)
  const [focusEnd, setFocusEnd] = useState(0);
  // a screenshot link straight into a maintenance view (demoView) shows that view
  const [maintenance, setMaintenance] = useState(() => localStorage.getItem("maint.enabled") === "1" || MAINT_VIEWS.includes(demoView ?? ""));
  const [pack, setPack] = useState<number | null>(demoPack !== null ? Number(demoPack) : null);
  const current = pack !== null ? live.packs[pack] : undefined;
  // what the monitor connection is doing, for the sidebar and the pack bus panel: three distinct "not connected" texts
  const [monitor, setMonitor] = useState<Monitor>("offline");
  // the app stopped the monitor itself (firmware view, maintenance step): "paused for maintenance", no lost
  // connection, so the inverter confirmation, the measurement and the master check stay
  const monitorStopped = () => { live.reset(); setConnected(false); setPack(null); setMonitor((m) => (m === "connected" ? "paused" : m)); };
  const onMonitorStopped = useRef(monitorStopped);
  onMonitorStopped.current = monitorStopped;
  useEffect(() => {
    const un = api.onMonitorStopped(() => onMonitorStopped.current());
    return () => { un.then((f) => f()); };
  }, []);

  return (
    <HelpProvider>
      <div className="flex h-full bg-bg text-ink">
        <Sidebar
          connected={connected}
          paused={monitor === "paused"}
          site={site}
          packs={Object.keys(live.packs).map(Number)}
          lastUpdate={Math.max(0, ...Object.values(live.packs).map((p) => p.updated))}
          locked={flashing}
          maintenance={maintenance}
          setMaintenance={(v) => { localStorage.setItem("maint.enabled", v ? "1" : "0"); setMaintenance(v); if (!v && MAINT_VIEWS.includes(view)) setView("dashboard"); }}
          view={view}
          setView={(v) => { setView(v); setPack(null); }}
          onConnected={(found, s, b) => { live.setDevices(found); setSite(s); setBus(b); setConnected(true); setMonitor("connected"); if (!isDemo) setPack(null); }}
          onDevices={live.addDevices}
          onEndpoint={(ep) => maint.attach(ep)}
          onDisconnected={(lost) => { live.reset(); setConnected(false); setPack(null); setDisconnects((d) => d + 1); setMonitor(lost ? "lost" : "offline"); maint.invalidateCheck(); }}
        />
        <main className="flex-1 overflow-y-auto px-10 py-10">
          <MaintBanner overview={maint.overview} unassigned={maint.unassigned} other={maint.other}
            onAssign={() => maint.unassigned && maint.assign(maint.unassigned)} onSwitch={maint.switchTo} onEnd={() => { localStorage.setItem("maint.enabled", "1"); setMaintenance(true); setView("firmware"); setFocusEnd(Date.now()); }} />
          {view === "firmware" && maintenance ? (
            <Firmware connected={connected} monitor={monitor} disconnects={disconnects} focusEnd={focusEnd} onFocusDone={() => setFocusEnd(0)} maint={maint} onRunning={setFlashing} onBack={() => setView("dashboard")}
              onMonitorStopped={monitorStopped} />
          ) : view === "cells" ? (
            <Cells />
          ) : view === "history" ? (
            <History site={site} />
          ) : view === "params" && maintenance ? (
            <ParametersView connected={connected} bus={bus} site={site} packs={Object.keys(live.packs).map(Number).sort((a, b) => a - b)} />
          ) : !connected ? (
            <div className="flex h-full max-w-md flex-col justify-center gap-3 text-lg text-muted"><Cable className="h-8 w-8" strokeWidth={1.5} aria-hidden />{t("dash.empty")}</div>
          ) : view === "diag" && maintenance ? (
            <Diagnosis packs={live.packs} site={site} />
          ) : current ? (
            <PackDetail p={current} multi={Object.keys(live.packs).length > 1} onBack={() => setPack(null)} site={site} packs={live.packs} />
          ) : (
            <Dashboard packs={live.packs} system={live.system} site={site} onOpen={setPack} />
          )}
        </main>
      </div>
    </HelpProvider>
  );
}
