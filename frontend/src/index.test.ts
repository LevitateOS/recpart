import { existsSync } from "node:fs";
import { basename } from "node:path";
import { describe, expect, it } from "bun:test";
import { RECPART_MANIFEST, RECPART_ROOT, extractInlineJson } from "./index";

describe("frontend pathing", () => {
  it("resolves recpart crate root and manifest", () => {
    expect(basename(RECPART_ROOT)).toBe("recpart");
    expect(existsSync(RECPART_MANIFEST)).toBe(true);
  });
});

describe("json extraction", () => {
  it("returns full payload when content is pure JSON", () => {
    const input = '{"ok":true}';
    expect(extractInlineJson(input)).toBe(input);
  });

  it("extracts final inline JSON line from mixed output", () => {
    const input = "info: warmup\n{\"schema_version\":1,\"code\":\"E001\"}";
    expect(extractInlineJson(input)).toBe('{"schema_version":1,"code":"E001"}');
  });

  it("returns null when no JSON line exists", () => {
    expect(extractInlineJson("plain text output")).toBeNull();
  });
});
