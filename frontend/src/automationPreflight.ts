import type { AutomationDefinition, AutomationIssue, Capability } from "./ipc/generated.ts";

export class AutomationPreflightCache {
  private context = "";
  private entries = new Map<string, { key: string; issues: AutomationIssue[] }>();
  async check(
    definitions: AutomationDefinition[],
    capabilities: Capability[],
    actionsKey: string,
    batch: (
      definitions: AutomationDefinition[],
      capabilities: Capability[],
    ) => Promise<AutomationIssue[][]>,
  ) {
    const context = JSON.stringify(capabilities) + actionsKey;
    if (context !== this.context) {
      this.entries.clear();
      this.context = context;
    }
    const keys = new Map(definitions.map((d) => [d.id, JSON.stringify(d)]));
    const missing = definitions.filter((d) => this.entries.get(d.id)?.key !== keys.get(d.id));
    for (let offset = 0; offset < missing.length; offset += 128) {
      const chunk = missing.slice(offset, offset + 128);
      const checked = await batch(chunk, capabilities);
      if (context !== this.context) return {};
      for (const [index, definition] of chunk.entries())
        this.entries.set(definition.id, {
          key: keys.get(definition.id)!,
          issues: checked[index] ?? [],
        });
    }
    for (const id of this.entries.keys()) if (!keys.has(id)) this.entries.delete(id);
    return Object.fromEntries(definitions.map((d) => [d.id, this.entries.get(d.id)?.issues ?? []]));
  }
}
