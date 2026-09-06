import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { catalogs, languageOptions, resolveLanguage, t, translate, localeFor } from "./localization.ts";
import { triggerSummary, stepSummary, weekSummary } from "./automation.ts";

test("system language respects preference order, regional aliases and fallback", () => {
  assert.equal(resolveLanguage("system", ["it-IT", "fr-CA", "de-DE"]), "frFr");
  assert.equal(resolveLanguage("system", ["pt_PT"]), "ptBr");
  assert.equal(resolveLanguage("system", ["zh-Hans-SG"]), "zhCn");
  assert.equal(resolveLanguage("system", []), "enUs");
  assert.equal(resolveLanguage("system", ["ar-SA"]), "enUs");
  for (const option of languageOptions) {
    assert.equal(resolveLanguage("system", [option.locale]), option.value);
    assert.equal(resolveLanguage(option.value, ["it-IT"]), option.value);
    assert.equal(localeFor(option.value), option.locale);
  }
});

test("every catalog has the same messages and preserves interpolation tokens", () => {
  const keys = Object.keys(catalogs.enUs).sort();
  const placeholders = (value: string) => [...value.matchAll(/\{([^{}]+)\}/g)].map(match => match[1]).sort();
  for (const { value } of languageOptions.filter(option => option.value !== "zhCn")) {
    assert.deepEqual(Object.keys(catalogs[value]).sort(), keys, value);
    for (const key of keys) {
      assert.ok(catalogs[value][key].trim(), `${value}: ${key}`);
      assert.deepEqual(placeholders(catalogs[value][key]), placeholders(catalogs.enUs[key]), `${value}: ${key}`);
    }
  }
});

test("interpolation preserves user names and never expands inserted placeholders", () => {
  const name = '用户 {count} <b> & "$1"';
  assert.equal(t("已添加“{name}”", "deDe", { name, count: 42 }), `„${name}“ hinzugefügt`);
  assert.equal(translate("unrecognized error", "jaJp"), "unrecognized error");
  assert.equal(translate("Save", "deDe"), "Speichern");
  assert.equal(translate("保存", "zhCn"), "保存");
  assert.equal(translate("版本 1.2.3", "ptBr"), "Versão 1.2.3");
  assert.equal(translate("版本 2 · 3 个节点", "deDe"), "Version 2 · Knoten: 3");
});

test("automation summaries localize labels and days while preserving user data", () => {
  assert.equal(stepSummary({ type: "delay", durationSeconds: 120 }, [], "deDe"), "2 Min. warten");
  assert.equal(weekSummary([6, 7], "frFr"), "Week-end");
  assert.match(weekSummary([1, 3], "enUs"), /Mon.*Wed/);
  const trigger = { type: "application" as const, event: "started" as const, apps: [{ id: "id", name: "我的应用", path: "/app" }] };
  assert.equal(triggerSummary(trigger, "enUs"), "When 我的应用: Started");
});


test("frontend language options match the generated Rust language contract", () => {
  const contract = readFileSync(new URL("./ipc/generated.ts", import.meta.url), "utf8");
  const declaration = contract.match(/export type LanguagePreference = ([^;]+);/);
  assert.ok(declaration, "Rust LanguagePreference must be exported");
  const variants = [...declaration[1].matchAll(/"([^"]+)"/g)].map(match => match[1]);
  assert.deepEqual(variants.sort(), ["system", ...languageOptions.map(option => option.value)].sort());
});
