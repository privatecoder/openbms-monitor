/** Results kept with the query they answer (installation, pack, period, address …), so a view never shows or exports
 *  an answer to an earlier query as the current one: after a change of the query, or a failed one, there is none. */
export interface Keyed<T> { key: string; value: T }

/** The value if it answers `key`, else null. */
export const forKey = <T,>(r: Keyed<T> | null, key: string): T | null => (r !== null && r.key === key ? r.value : null);
