import { bundleName } from "./views/InstallationTransfer";

/** The ZIP paths of a backup run: the usual export names, a name used twice in this run gets " (2)", " (3)" … */
export function backupPaths(targets: { id: string; name: string }[], folder: string, now = new Date()): string[] {
  const seen = new Map<string, number>();
  return targets.map(({ name }) => {
    const base = bundleName(name, now);
    const n = (seen.get(base) ?? 0) + 1;
    seen.set(base, n);
    const file = n === 1 ? base : base.replace(/\.zip$/, ` (${n}).zip`);
    return `${folder.replace(/[\\/]+$/, "")}/${file}`;
  });
}

/** Back up every target as an installation ZIP, then delete. The first failed backup stops the run: nothing is deleted. */
export async function backupThenDelete<T>(
  targets: { id: string; name: string }[],
  folder: string,
  exportOne: (id: string, path: string) => Promise<unknown>,
  del: () => Promise<T>,
): Promise<T> {
  const paths = backupPaths(targets, folder);
  for (let i = 0; i < targets.length; i++) await exportOne(targets[i].id, paths[i]);
  return del();
}
