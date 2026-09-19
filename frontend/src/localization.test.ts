import assert from "node:assert/strict";
import test from "node:test";
import { readdirSync, readFileSync } from "node:fs";
import {
  catalogs,
  languageOptions,
  resolveLanguage,
  t,
  translate,
  localeFor,
} from "./localization.ts";
import { actionDisplayName, actionDisplayGroup } from "./features/actions/actionLocalization.ts";
import { friendlyDeviceName } from "./features/input-sharing/inputSharingUi.ts";
import type { RuntimeSnapshot } from "./types.ts";
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
  const placeholders = (value: string) =>
    [...value.matchAll(/\{([^{}]+)\}/g)].map((match) => match[1]).sort();
  for (const { value } of languageOptions.filter((option) => option.value !== "zhCn")) {
    assert.deepEqual(Object.keys(catalogs[value]).sort(), keys, value);
    for (const key of keys) {
      assert.ok(catalogs[value][key].trim(), `${value}: ${key}`);
      assert.deepEqual(
        placeholders(catalogs[value][key]),
        placeholders(catalogs.enUs[key]),
        `${value}: ${key}`,
      );
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
  const trigger = {
    type: "application" as const,
    event: "started" as const,
    apps: [{ id: "id", name: "我的应用", path: "/app" }],
  };
  assert.equal(triggerSummary(trigger, "enUs"), "When 我的应用: Started");
});

test("frontend language options match the generated Rust language contract", () => {
  const contract = readFileSync(new URL("./ipc/generated.ts", import.meta.url), "utf8");
  const declaration = contract.match(/export type LanguagePreference = ([^;]+);/);
  assert.ok(declaration, "Rust LanguagePreference must be exported");
  const variants = [...declaration[1].matchAll(/"([^"]+)"/g)].map((match) => match[1]);
  assert.deepEqual(
    variants.sort(),
    ["system", ...languageOptions.map((option) => option.value)].sort(),
  );
});

test("built-in preset metadata is translated while custom names and groups remain unchanged", () => {
  const rust = readFileSync(
    new URL("../../src/application/preset_catalog.rs", import.meta.url),
    "utf8",
  );
  const presets = [
    ...rust.matchAll(
      /preset\(\s*"([^"]+)",\s*"([^"]+)",\s*"([^"]+)",\s*"[^"]+",\s*"[^"]+",\s*"([^"]+)"/g,
    ),
  ].map((match) => ({ id: match[1], name: match[2], description: match[3], group: match[4] }));
  presets.push(
    ...[...rust.matchAll(/"(path\.[^"]+)",\s*"([^"]+)",\s*"([^"]+)"/g)].map((match) => ({
      id: match[1],
      name: match[2],
      description: match[3],
      group: "文件",
    })),
  );
  assert.equal(presets.length, 24);
  for (const preset of presets) {
    const action = { ...preset, source_preset_id: preset.id };
    for (const { value } of languageOptions.filter((option) => option.value !== "zhCn")) {
      for (const key of [preset.name, preset.group, preset.description]) {
        assert.ok(catalogs[value][key], `${value}: ${preset.id}: ${key}`);
      }
      assert.equal(actionDisplayName(action, value), translate(preset.name, value));
      assert.equal(actionDisplayGroup(action, value), translate(preset.group, value));
      assert.equal(actionDisplayName({ ...action, name: "保存" }, value), "保存");
      assert.equal(actionDisplayGroup({ ...action, group: "常用" }, value), "常用");
      assert.equal(actionDisplayName({ ...action, source_preset_id: null }, value), preset.name);
    }
  }
});

test("input sharing UI strings are covered by every catalog", () => {
  const folder = new URL("./features/input-sharing/", import.meta.url);
  const keys = new Set<string>();
  for (const file of readdirSync(folder, { recursive: true, encoding: "utf8" })) {
    if (!file.endsWith(".svelte")) continue;
    const contents = readFileSync(new URL(file, folder), "utf8");
    for (const match of contents.matchAll(/"((?:[^"\\]|\\.)*)"/g)) {
      const value = JSON.parse(`"${match[1]}"`);
      if (/[\u4e00-\u9fff]/.test(value)) keys.add(value);
    }
  }
  for (const key of keys) {
    for (const { value } of languageOptions.filter((option) => option.value !== "zhCn")) {
      assert.ok(catalogs[value][key], `${value}: ${key}`);
    }
  }
});

test("input sharing templates preserve peer names and use complete localized messages", () => {
  const snapshot = {
    serviceInstanceId: "local",
    nearbyPeers: [{ serviceInstanceId: "remote", displayName: "保存" }],
    configuration: { layout: { displays: {} } },
  } as unknown as RuntimeSnapshot;
  assert.equal(friendlyDeviceName("local", snapshot, "enUs"), "This device");
  assert.equal(friendlyDeviceName("remote", snapshot, "enUs"), "保存");
  assert.equal(friendlyDeviceName("unknown", snapshot, "deDe"), "Remotegerät unknown");
  assert.equal(
    t("{screens} 块屏幕 · {edges} 条跨屏边缘 · {devices} 台远程设备在线", "enUs", {
      screens: 3,
      edges: 1,
      devices: 1,
    }),
    "Screens: 3 · Screen edges: 1 · Remote devices online: 1",
  );
  assert.equal(
    t("等待{part0}重新连接", "enUs", { part0: "我的设备 {part0}" }),
    "Waiting for 我的设备 {part0} to reconnect",
  );
});
