import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import test from "node:test";
import { selectKey, selectedOption, type SelectOption } from "./selectOptions.ts";

test("select values preserve empty strings, booleans and numbers without collisions", () => {
  const values = ["", false, true, 0, 1, 1.25, 224, "false", "true", "0", "1", "number:1"];
  const options = values.map((value) => ({ value, label: String(value) }));
  assert.equal(new Set(values.map(selectKey)).size, values.length);
  for (const value of values) {
    assert.notEqual(selectKey(value), "", "empty is reserved for no selection in Bits UI");
    assert.equal(selectedOption(options, selectKey(value))?.value, value);
  }
});

test("option identity survives reordering and localization, and rejects removed options", () => {
  const options: SelectOption[] = [
    { value: "updatedAt", label: "最近使用优先" },
    { value: "createdAt", label: "创建时间优先", disabled: true },
  ];
  const key = selectKey("createdAt");
  assert.equal(selectedOption(options, key)?.disabled, true);
  assert.equal(selectedOption([...options].reverse().map((option) => ({ ...option, label: "Translated" })), key)?.value, "createdAt");
  assert.equal(selectedOption(options.slice(0, 1), key), undefined);
  assert.equal(selectedOption(options, "unknown"), undefined);
});

test("desktop entry points do not reintroduce OS-rendered select controls", () => {
  function check(directory: URL) {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = new URL(`${entry.name}${entry.isDirectory() ? "/" : ""}`, directory);
      if (entry.isDirectory()) check(path);
      else if (entry.name.endsWith(".svelte")) {
        assert.doesNotMatch(readFileSync(path, "utf8"), /<select\b/, path.pathname);
      }
    }
  }
  check(new URL("./", import.meta.url));
});
