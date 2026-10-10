import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Copy, Download, Pencil, Plus, RotateCcw, Search, Trash2, Upload } from "lucide-react";
import { useCellDb, type DbEntry, type Origin } from "../cells/store";
import { CellEditor, FIELDS } from "../cells/CellEditor";
import { DeratingTable } from "../cells/DeratingTable";
import { missingRequired, toAmps } from "../cells/rules";
import { REQUIRED_FOR_CHECKS, type Cell, type Rate } from "../cells/types";
import { dbErrors } from "../cells/validate";
import { Button } from "../components/ui/button";
import { Panel } from "../components/ui/card";
import { Sheet, SheetContent } from "../components/ui/sheet";
import { InfoIcon } from "../help";
import { cn, fmt } from "../lib/utils";

const get = (o: unknown, path: string) => path.split(".").reduce<unknown>((x, k) => (x && typeof x === "object" ? (x as Record<string, unknown>)[k] : undefined), o);
const name = (c: Cell) => [c.manufacturer, c.model, c.variant].filter(Boolean).join(" ");
const originTone: Record<Origin, string> = { bundled: "text-muted", overridden: "text-discharge", own: "text-charge" };

function blankCell(): Cell {
  return {
    schema_version: 1, id: "", entry_version: 1, updated: new Date().toISOString().slice(0, 10), manufacturer: "", model: "", chemistry: "LFP",
    datasheet: { title: "", source_type: "manufacturer_pdf", url: "" }, provenance: {},
  };
}

export function Cells() {
  const { t } = useTranslation();
  const db = useCellDb();
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<"all" | "bundled" | "mine">("all");
  const [selected, setSelected] = useState<string | null>(null);
  const [editing, setEditing] = useState<{ cell: Cell; isNew: boolean } | null>(null);
  const [transfer, setTransfer] = useState<"import" | "export" | null>(null);

  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    return db.entries.filter((e) =>
      (filter === "all" || (filter === "bundled" ? e.origin !== "own" : e.origin !== "bundled")) &&
      (!q || `${name(e.cell)} ${e.cell.capacity?.nominal_ah ?? ""}ah ${e.cell.id}`.toLowerCase().includes(q)));
  }, [db.entries, query, filter]);
  const current = db.entries.find((e) => e.cell.id === selected) ?? list[0];

  return (
    <div className="view-in mx-auto flex h-full max-w-6xl flex-col gap-5">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="flex items-center gap-2 font-display text-4xl font-semibold tracking-tight">{t("cells.title")}<InfoIcon id="topic.cells" /></h1>
          <p className="mt-1 text-muted">{t("cells.count", { total: db.entries.length, mine: db.user.length })}</p>
        </div>
        <div className="flex gap-2">
          <Button size="sm" onClick={() => setEditing({ cell: blankCell(), isNew: true })}><Plus className="h-4 w-4" />{t("cells.new")}</Button>
          <Button size="sm" variant="ghost" onClick={() => setTransfer("import")}><Upload className="h-4 w-4" />{t("cells.import")}</Button>
          <Button size="sm" variant="ghost" onClick={() => setTransfer("export")}><Download className="h-4 w-4" />{t("cells.export")}</Button>
        </div>
      </header>
      {db.error && <p className="text-sm text-alarm">{db.error}</p>}

      <div className="grid min-h-0 flex-1 grid-cols-[20rem_1fr] gap-5">
        <Panel className="flex min-h-0 flex-col">
          <div className="space-y-2 border-b border-line p-3">
            <label className="relative block">
              <Search className="pointer-events-none absolute left-2.5 top-2.5 h-4 w-4 text-muted" aria-hidden />
              <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t("cells.search")} aria-label={t("cells.search")}
                className="h-9 w-full rounded-md border border-line bg-surface pl-8 pr-3 text-sm text-ink outline-none focus:border-charge" />
            </label>
            <div className="flex rounded-md bg-sunken p-0.5 text-sm" role="group">
              {(["all", "bundled", "mine"] as const).map((f) => (
                <button key={f} aria-pressed={filter === f} onClick={() => setFilter(f)}
                  className={cn("flex-1 rounded px-2 py-1", filter === f ? "bg-surface text-ink shadow-sm" : "text-muted hover:text-ink")}>{t(`cells.filter.${f}`)}</button>
              ))}
            </div>
          </div>
          <ul className="min-h-0 flex-1 overflow-y-auto">
            {list.map((e) => (
              <li key={e.cell.id}>
                <button onClick={() => setSelected(e.cell.id)}
                  className={cn("w-full border-b border-line px-3 py-2.5 text-left hover:bg-sunken/50", current?.cell.id === e.cell.id && "bg-sunken")}>
                  <div className="font-medium">{name(e.cell)}</div>
                  <div className="flex flex-wrap gap-x-2 text-xs text-muted">
                    <span>{fmt(e.cell.capacity?.nominal_ah, 0, "Ah")}</span>
                    <span className={originTone[e.origin]}>{t(`cells.origin.${e.origin}`)}</span>
                    {e.cell.charge_derating && <span>{t("cells.hasTable")}</span>}
                    {missingRequired(e.cell, REQUIRED_FOR_CHECKS).length > 0 && <span className="text-discharge">{t("cells.incomplete")}</span>}
                  </div>
                </button>
              </li>
            ))}
            {!list.length && <li className="p-4 text-sm text-muted">{t("cells.none")}</li>}
          </ul>
        </Panel>

        <div className="min-h-0 overflow-y-auto">
          {current ? (
            <Detail entry={current}
              onEdit={() => setEditing({ cell: current.cell, isNew: false })}
              onDuplicate={() => setEditing({ cell: { ...structuredClone(current.cell), id: `${current.cell.id}-copy`, entry_version: 1 }, isNew: true })}
              onRestore={() => db.remove(current.cell.id)}
              onDelete={() => { db.remove(current.cell.id); setSelected(null); }} />
          ) : <p className="text-muted">{t("cells.none")}</p>}
        </div>
      </div>

      {editing && (
        <CellEditor open={!!editing} onOpenChange={(o) => !o && setEditing(null)} initial={editing.cell} isNew={editing.isNew}
          onSave={async (c) => {
            if (editing.isNew && db.entries.some((e) => e.cell.id === c.id)) throw new Error(t("cells.idExists", { id: c.id }));
            await db.save(c); setSelected(c.id);
          }} />
      )}
      <Transfer mode={transfer} onClose={() => setTransfer(null)} user={db.user} all={db.entries.map((e) => e.cell)} onImport={db.importCells} />
    </div>
  );
}

function Detail({ entry, onEdit, onDuplicate, onRestore, onDelete }: { entry: DbEntry; onEdit: () => void; onDuplicate: () => void; onRestore: () => void; onDelete: () => void }) {
  const { t } = useTranslation();
  const c = entry.cell;
  const missing = missingRequired(c, REQUIRED_FOR_CHECKS);
  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="font-display text-3xl font-semibold">{name(c)}</h2>
          <p className="text-sm text-muted">
            <span className={originTone[entry.origin]}>{t(`cells.origin.${entry.origin}`)}</span>, {c.chemistry}{c.form_factor ? `, ${t(`cells.form.${c.form_factor}`)}` : ""}, {t("cells.version", { v: c.entry_version, d: c.updated })}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button size="sm" onClick={onEdit}><Pencil className="h-4 w-4" />{entry.origin === "bundled" ? t("cells.adapt") : t("cells.edit")}</Button>
          <Button size="sm" variant="ghost" onClick={onDuplicate}><Copy className="h-4 w-4" />{t("cells.duplicate")}</Button>
          {entry.origin === "overridden" && <Button size="sm" variant="ghost" onClick={onRestore}><RotateCcw className="h-4 w-4" />{t("cells.restore")}</Button>}
          {entry.origin === "own" && <Button size="sm" variant="ghost" onClick={onDelete}><Trash2 className="h-4 w-4" />{t("cells.delete")}</Button>}
        </div>
      </div>

      <Panel className="p-4 text-sm">
        <div className="text-muted">{t("cells.f.title")}</div>
        <div className="font-medium">{c.datasheet.title}</div>
        <div className="mt-1 break-all text-muted">{c.datasheet.url}</div>
        <div className="mt-1 text-xs text-muted">{t(`cells.src.${c.datasheet.source_type}`)}</div>
        {missing.length > 0 && <p className="mt-2 text-discharge">{t("cells.missing", { list: missing.join(", ") })}</p>}
      </Panel>

      <div className="grid gap-5 md:grid-cols-2">
        {FIELDS.map((g) => (
          <Panel key={g.group} className="p-4">
            <h3 className="mb-2 font-display text-lg font-semibold">{t(`cells.g.${g.group}`)}</h3>
            <dl className="space-y-1.5 text-sm">
              {g.fields.map((f) => <ValueRow key={f.path} cell={c} path={f.path} unit={f.unit} />)}
            </dl>
          </Panel>
        ))}
      </div>

      <Panel className="p-4">
        <h3 className="mb-2 flex items-center gap-1.5 font-display text-lg font-semibold">{t("cells.g.derating")}<InfoIcon id="topic.derating_rule" /></h3>
        <DeratingTable cell={c} />
        <ProvenanceNote p={c.provenance?.charge_derating} />
      </Panel>
    </div>
  );
}

/** A value with its provenance: verified mark, "not stated" for verified absence, note as tooltip. */
function ValueRow({ cell, path, unit }: { cell: Cell; path: string; unit?: string }) {
  const { t } = useTranslation();
  const v = get(cell, path);
  const p = cell.provenance?.[path];
  let text: string;
  if (v === undefined) text = p?.verified && p.note ? t("cells.notStated") : "–";
  else if (typeof v === "object") {
    const r = v as Rate, a = toAmps(cell, r.value, r.basis);
    text = `${fmt(r.value, 2)} ${r.basis}${a !== undefined ? ` (${fmt(a, 0, "A")})` : ""}`;
  } else text = fmt(v as number, unit === "V" ? 2 : unit === "Ah" || unit === "Wh" || unit === "A" ? 0 : 0, unit);
  return (
    <div className="flex items-baseline justify-between gap-3" title={p?.note}>
      <dt className="text-muted">{t(`cells.f.${path.split(".")[1]}`)}</dt>
      <dd className="flex items-center gap-1.5 text-right">
        <span className={v === undefined ? "text-muted" : "font-medium"}>{text}</span>
        {p?.verified ? <Check className="h-3.5 w-3.5 text-ok" aria-label={t("cells.verified")} /> : v !== undefined && <span className="text-xs text-discharge">{t("cells.unverified")}</span>}
      </dd>
    </div>
  );
}

function ProvenanceNote({ p }: { p?: { verified: boolean; note?: string } }) {
  const { t } = useTranslation();
  if (!p?.note) return null;
  return <p className="mt-2 text-xs text-muted">{p.verified ? `${t("cells.verified")}: ` : ""}{p.note}</p>;
}

/** Import (paste JSON) and export (copy JSON) without file dialogs. */
function Transfer({ mode, onClose, user, all, onImport }: { mode: "import" | "export" | null; onClose: () => void; user: Cell[]; all: Cell[]; onImport: (c: Cell[]) => Promise<void> }) {
  const { t } = useTranslation();
  const [text, setText] = useState("");
  const [scope, setScope] = useState<"mine" | "all">("mine");
  const [msg, setMsg] = useState<string | null>(null);
  const exported = JSON.stringify(scope === "mine" ? user : all, null, 2);
  const doImport = async () => {
    let data: unknown;
    try { data = JSON.parse(text); } catch (e) { setMsg(String(e)); return; }
    const arr = Array.isArray(data) ? data : [data];
    const errs = dbErrors(arr);
    if (errs.length) { setMsg(`${t("cells.invalid")} ${errs.join("; ")}`); return; }
    await onImport(arr as Cell[]);
    setMsg(t("cells.imported", { count: arr.length }));
    setText("");
  };
  return (
    <Sheet open={mode !== null} onOpenChange={(o) => { if (!o) { onClose(); setMsg(null); } }}>
      <SheetContent title={mode === "import" ? t("cells.importTitle") : t("cells.exportTitle")} className="w-[40rem]">
        {mode === "export" ? (
          <div className="space-y-3">
            <div className="flex rounded-md bg-sunken p-0.5 text-sm" role="group">
              {(["mine", "all"] as const).map((s) => (
                <button key={s} aria-pressed={scope === s} onClick={() => setScope(s)}
                  className={cn("flex-1 rounded px-2 py-1", scope === s ? "bg-surface text-ink shadow-sm" : "text-muted hover:text-ink")}>{t(`cells.scope.${s}`)}</button>
              ))}
            </div>
            <textarea readOnly value={exported} className="h-[60vh] w-full rounded-md border border-line bg-surface p-3 font-mono text-xs text-ink" />
            <Button variant="primary" onClick={() => navigator.clipboard.writeText(exported).then(() => setMsg(t("cells.copied")), (e) => setMsg(String(e)))}>
              <Copy className="h-4 w-4" />{t("cells.copy")}
            </Button>
          </div>
        ) : (
          <div className="space-y-3">
            <p className="text-sm text-muted">{t("cells.importHint")}</p>
            <textarea value={text} onChange={(e) => setText(e.target.value)} spellCheck={false}
              className="h-[55vh] w-full rounded-md border border-line bg-surface p-3 font-mono text-xs text-ink outline-none focus:border-charge" />
            <Button variant="primary" disabled={!text.trim()} onClick={doImport}><Upload className="h-4 w-4" />{t("cells.import")}</Button>
          </div>
        )}
        {msg && <p className="mt-3 text-sm text-muted">{msg}</p>}
      </SheetContent>
    </Sheet>
  );
}
