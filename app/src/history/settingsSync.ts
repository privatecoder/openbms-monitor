/** Orders the recording settings against the 2-s status poll: writes go to the backend one after the other, and a poll
 *  answer is dropped when the poll began before or during a write, so an old answer never puts old settings back into
 *  the controls (and a next click never builds its settings from them). `apply` takes what is shown. */
export function settingsSync<S>(apply: (s: S) => void) {
  let seq = 0;
  // `seq` at the last start or end of a write: polls begun before it may hold older settings
  let settled = 0;
  let writing = 0;
  let newest = 0;
  let queue: Promise<unknown> = Promise.resolve();
  return {
    poll: async (get: () => Promise<S>) => {
      const mine = ++seq;
      const s = await get();
      if (writing === 0 && mine > settled) apply(s);
    },
    /** `put` runs after the writes before it; only the newest write's answer is shown. */
    write: (put: () => Promise<S>): Promise<void> => {
      const mine = (newest = settled = ++seq);
      writing += 1;
      const done = queue.then(put).finally(() => { writing -= 1; settled = ++seq; });
      queue = done.catch(() => {});
      return done.then((s) => { if (newest === mine) apply(s); });
    },
  };
}
