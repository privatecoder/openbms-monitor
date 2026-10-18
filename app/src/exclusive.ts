/** Runs `fn` unless a run started with the same `flag` is still going (a second click before the answer): the flag is
 *  set synchronously, before any await, so two clicks in one frame cannot both start. Resolves to undefined when
 *  skipped. */
export async function runExclusive<T>(flag: { current: boolean }, fn: () => Promise<T>): Promise<T | undefined> {
  if (flag.current) return undefined;
  flag.current = true;
  try {
    return await fn();
  } finally {
    flag.current = false;
  }
}
