import { test } from "node:test";
import assert from "node:assert/strict";
import { keywordIssue } from "./clipboardSensitiveRules.ts";

test("keyword editing respects UTF-8 limits, controls and empty values", () => {
  assert.equal(keywordIssue("   ", [], null), "empty");
  assert.equal(keywordIssue("文".repeat(85), [], null), null);
  assert.equal(keywordIssue("文".repeat(86), [], null), "tooLong");
  assert.equal(keywordIssue("internal\nonly", [], null), "control");
  assert.equal(keywordIssue(" internal only ", [], null), null);
});
test("duplicate detection matches host ASCII case folding and permits editing oneself", () => {
  assert.equal(keywordIssue("INTERNAL", ["internal"], null), "duplicate");
  assert.equal(keywordIssue("INTERNAL", ["internal"], 0), null);
  assert.equal(keywordIssue("内部", ["内部"], null), "duplicate");
  assert.equal(keywordIssue("Ä", ["ä"], null), null);
});
test("a full keyword list still permits editing an existing entry", () => {
  const keywords = Array.from({ length: 100 }, (_, index) => `keyword-${index}`);
  assert.equal(keywordIssue("new", keywords, null), "tooMany");
  assert.equal(keywordIssue("new", keywords, 0), null);
});
