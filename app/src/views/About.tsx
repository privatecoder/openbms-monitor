import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import Markdown from "react-markdown";
import { BatteryCharging, BookOpen, Bug, Download, ExternalLink, GitBranch, Package, RefreshCw, Scale, ScrollText } from "lucide-react";
import { api, type AboutLink, type AppInfo } from "../api";
import { Panel } from "../components/ui/card";
import { Button } from "../components/ui/button";
import { checkUpdate, installUpdate, setAutoCheck, updateErrorKey, useUpdate } from "../update";
import changelog from "../../../CHANGELOG.md?raw";
import { StoragePanel } from "./Storage";

/** The changelog without its title and intro: from the first version heading on. */
export const changelogBody = (raw: string) => {
  const i = raw.search(/^## /m);
  return i < 0 ? raw : raw.slice(i);
};

/** links in Markdown stay text: the webview would navigate away from the app */
const md = { a: ({ children }: { children?: ReactNode }) => <span className="underline decoration-line">{children}</span> };
const mdClass = "[&_code]:rounded [&_code]:bg-sunken [&_code]:px-1 [&_code]:text-[0.85em] [&_h2]:font-display [&_h2]:text-lg [&_h2]:font-semibold [&_h2:not(:first-child)]:pt-3 [&_li]:ml-4 [&_li]:list-disc [&_li]:text-ink/85 [&_ul]:space-y-1";

/** Check for a newer release, show its notes, install it on a click. */
function UpdatePanel() {
  const { t } = useTranslation();
  const u = useUpdate();
  const pct = u.progress?.total ? Math.min(100, Math.round((u.progress.done / u.progress.total) * 100)) : null;
  const mb = (n: number) => (n / 1_048_576).toFixed(1);
  return (
    <Panel className="flex flex-col gap-3 p-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="font-medium">{t("about.update.title")}</div>
          <p className="text-sm text-muted" role="status">
            {u.phase === "checking" ? t("about.update.checking")
              : u.phase === "current" ? t("about.update.current")
              : u.phase === "available" && u.info ? t("about.update.available", { version: u.info.version })
              : u.phase === "installing" ? (u.progress ? (pct != null ? t("about.update.progress", { pct }) : t("about.update.progressMb", { mb: mb(u.progress.done) })) : t("about.update.starting"))
              : u.phase === "error" && u.failed === "check" ? t("about.update.checkFailed")
              : t("about.update.hint")}
          </p>
        </div>
        <div className="flex gap-2">
          {u.phase === "available"
            ? <Button size="sm" onClick={() => void installUpdate()}><Download className="h-4 w-4" />{t("about.update.install")}</Button>
            : <Button size="sm" variant="ghost" disabled={u.phase === "checking" || u.phase === "installing"} onClick={() => void checkUpdate()}>
                <RefreshCw className={u.phase === "checking" ? "h-4 w-4 animate-spin" : "h-4 w-4"} />{t("about.update.check")}
              </Button>}
        </div>
      </div>
      {u.phase === "installing" && <div className="h-1.5 overflow-hidden rounded bg-sunken"><div className="h-full bg-charge transition-[width]" style={{ width: `${pct ?? 15}%` }} /></div>}
      {u.error && u.failed === "check" && <p className="text-xs text-muted">{u.error}</p>}
      {u.error && u.failed === "install" && <p className="text-sm text-alarm">{t(updateErrorKey(u.error))}{!u.error.startsWith("busy:") && <span className="block text-xs text-muted">{u.error}</span>}</p>}
      {u.phase === "available" && <p className="text-sm text-muted">{t("about.update.installHint")}</p>}
      {u.info?.notes && (u.phase === "available" || u.phase === "installing") && (
        <details className="text-sm">
          <summary className="cursor-pointer text-muted hover:text-ink">{t("about.update.notes", { version: u.info.version })}</summary>
          <div className={`mt-2 space-y-2 ${mdClass}`}><Markdown components={md}>{u.info.notes}</Markdown></div>
        </details>
      )}
      <label className="flex items-center gap-2 text-sm text-muted">
        <input type="checkbox" checked={u.auto} onChange={(e) => setAutoCheck(e.target.checked)} />{t("about.update.auto")}
      </label>
    </Panel>
  );
}

const LINKS: [AboutLink, typeof GitBranch][] = [["repo", GitBranch], ["releases", Package], ["docs", BookOpen], ["issues", Bug], ["license", Scale]];

export function About() {
  const { t, i18n } = useTranslation();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { api.appInfo().then(setInfo, (e) => setError(String(e))); }, []);
  const open = (which: AboutLink) => { api.openLink(which).catch((e) => setError(String(e))); };

  return (
    <div className="view-in mx-auto flex max-w-4xl flex-col gap-5 pb-8">
      <header className="flex items-center gap-4">
        <BatteryCharging className="h-12 w-12 shrink-0 text-ok" strokeWidth={1.5} aria-hidden />
        <div>
          <h1 className="font-display text-4xl font-semibold tracking-tight">{t("app")}</h1>
          <p className="mt-1 text-muted">
            {t("about.version", { version: info?.version ?? "…" })}{info?.version.includes("-") && <> · {t("about.prerelease")}</>}
          </p>
        </div>
      </header>
      <p className="max-w-3xl text-ink/80">{t("about.intro")}</p>
      {error && <p className="text-sm text-alarm">{error}</p>}

      <div className="flex flex-wrap gap-2">
        {LINKS.map(([id, I]) => (
          <Button key={id} size="sm" variant="ghost" onClick={() => open(id)} title={t(`about.link.${id}Hint`)}>
            <I className="h-4 w-4" />{t(`about.link.${id}`)}<ExternalLink className="h-3 w-3 text-muted" aria-hidden />
          </Button>
        ))}
      </div>

      <UpdatePanel />

      <StoragePanel />

      <section className="flex flex-col gap-3">
        <div className="flex items-center justify-between gap-3">
          <h2 className="flex items-center gap-2 font-display text-2xl font-semibold"><ScrollText className="h-5 w-5" aria-hidden />{t("about.changelog")}</h2>
          <Button size="sm" variant="ghost" onClick={() => open("changelog")}>{t("about.link.changelog")}<ExternalLink className="h-3 w-3 text-muted" aria-hidden /></Button>
        </div>
        {i18n.language !== "en" && <p className="text-sm text-muted">{t("about.changelogLang")}</p>}
        <Panel className={`space-y-3 p-5 ${mdClass}`}>
          <Markdown components={md}>{changelogBody(changelog)}</Markdown>
        </Panel>
      </section>
    </div>
  );
}
