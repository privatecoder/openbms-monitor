/** Every error the backend gives, decoded in one place. Structured errors are JSON objects with a `kind` (as text, as an
 *  Error's message or as the object itself); anything else is plain text. The formatters (`maint.ts`, `control.ts`,
 *  `connCheck.ts`, `calibration.ts`, `wiring.ts`) ask this instead of each parsing on its own. */
export type Decoded = Record<string, unknown> & { kind?: unknown };

export function decode(e: unknown): Decoded | null {
  if (e && typeof e === "object" && !(e instanceof Error)) return e as Decoded;
  const text = typeof e === "string" ? e : e instanceof Error ? e.message : null;
  if (text === null) return null;
  try {
    const v: unknown = JSON.parse(text);
    return v && typeof v === "object" ? (v as Decoded) : null;
  } catch {
    return null;
  }
}

/** The `kind` of a structured error, else undefined. */
export const kindOf = (e: unknown): unknown => decode(e)?.kind;

/** The text of an error as it came (structured ones as their JSON). */
export const textOf = (e: unknown): string => (typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e ?? ""));
