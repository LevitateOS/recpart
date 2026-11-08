
/** What a recpart run printed, read the one way recpart writes it: the result
 * document on stdout, or, on failure, an error document on stderr. */
export type RecpartOutput<T> =
	| { kind: "ok"; value: T }
	| { kind: "error"; error: ErrorPayload }
	| { kind: "unreadable"; reason: string };

export function readRecpartOutput<T extends { schema_version: number }>(
	run: { exitCode: number | null; stdout: string; stderr: string },
	schemaVersion: number,
): RecpartOutput<T> {
	const ok = run.exitCode === 0;
	const text = ok ? run.stdout : run.stderr;
	let value: { schema_version?: unknown } | null;
	try {
		value = JSON.parse(text);
	} catch {
		return { kind: "unreadable", reason: `recpart printed no JSON (${ok ? "stdout" : "stderr"}): ${text.trim().slice(0, 200)}` };
	}
	const expected = ok ? schemaVersion : ERROR_SCHEMA_VERSION;
	if (!value || value.schema_version !== expected) {
		return { kind: "unreadable", reason: `recpart printed schema ${String(value?.schema_version)}, expected ${expected}` };
	}
	return ok ? { kind: "ok", value: value as T } : { kind: "error", error: value as unknown as ErrorPayload };
}
