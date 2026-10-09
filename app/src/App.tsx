import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Cable } from "lucide-react";
import { Sidebar } from "./views/Sidebar";
import { Dashboard } from "./views/Dashboard";
import { PackDetail } from "./views/PackDetail";
import { HelpProvider } from "./help";
import { useLiveData } from "./store";
import { isDemo } from "./api";

const demoPack = isDemo ? new URLSearchParams(location.search).get("pack") : null;

export default function App() {
  const { t } = useTranslation();
  const live = useLiveData();
  const [connected, setConnected] = useState(false);
  const [site, setSite] = useState("");
  const [view, setView] = useState("dashboard");
  const [pack, setPack] = useState<number | null>(demoPack !== null ? Number(demoPack) : null);
  const current = pack !== null ? live.packs[pack] : undefined;

  return (
    <HelpProvider>
      <div className="flex h-full bg-bg text-ink">
        <Sidebar
          connected={connected}
          lastUpdate={Math.max(0, ...Object.values(live.packs).map((p) => p.updated))}
          view={view}
          setView={(v) => { setView(v); setPack(null); }}
          onConnected={(found, s) => { live.setDevices(found); setSite(s); setConnected(true); if (!isDemo) setPack(null); }}
          onDisconnected={() => { live.reset(); setConnected(false); setPack(null); }}
        />
        <main className="flex-1 overflow-y-auto px-10 py-10">
          {!connected ? (
            <div className="flex h-full max-w-md flex-col justify-center gap-3 text-lg text-muted"><Cable className="h-8 w-8" strokeWidth={1.5} aria-hidden />{t("dash.empty")}</div>
          ) : current ? (
            <PackDetail p={current} onBack={() => setPack(null)} />
          ) : (
            <Dashboard packs={live.packs} system={live.system} site={site} onOpen={setPack} />
          )}
        </main>
      </div>
    </HelpProvider>
  );
}
