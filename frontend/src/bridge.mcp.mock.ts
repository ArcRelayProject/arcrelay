import type { McpClientView, McpPermissions, McpClientConfig, ConfigurationChange } from './ipc/generated';
const clients: McpClientView[] = [
  { id: 'preview-codex', name: 'Codex', legacy: false, revoked: false, createdAt: new Date().toISOString(), permissions: { notifications: true, read: true, manage: true, enable: true, execute: false, scripts: false } },
  { id: 'legacy-notifications', name: 'Legacy notifications', legacy: true, revoked: false, createdAt: new Date().toISOString(), permissions: { notifications: true, read: false, manage: false, enable: false, execute: false, scripts: false } },
  { id: 'preview-revoked', name: 'Claude Desktop', legacy: false, revoked: true, createdAt: new Date().toISOString(), permissions: { notifications: true, read: true, manage: false, enable: false, execute: false, scripts: false } },
];
const tokens = new Map<string, string>([['preview-codex', 'preview-not-a-real-credential']]);
function find(id: string) { const c = clients.find(c => c.id === id); if (!c) throw new Error('MCP client not found'); return c; }
export const mcpMockBridge = {
  async listMcpClients(): Promise<McpClientView[]> { return structuredClone(clients); },
  async createMcpClient(name: string, permissions: McpPermissions): Promise<McpClientView> {
    if (!name.trim()) throw new Error('Enter a client name');
    const client: McpClientView = { id: crypto.randomUUID(), name: name.trim(), permissions: { ...permissions }, revoked: false, legacy: false, createdAt: new Date().toISOString() };
    clients.push(client); tokens.set(client.id, `preview-${crypto.randomUUID()}`); return structuredClone(client);
  },
  async setMcpClientPermissions(clientId: string, permissions: McpPermissions): Promise<void> { find(clientId).permissions = { ...permissions }; },
  async revokeMcpClient(clientId: string): Promise<void> { find(clientId).revoked = true; },
  async rotateMcpClientToken(clientId: string): Promise<McpClientView> { const c = find(clientId); c.revoked = false; tokens.set(clientId, `preview-${crypto.randomUUID()}`); return structuredClone(c); },
  async getMcpClientConfig(clientId: string): Promise<McpClientConfig> {
    const c = find(clientId); if (c.revoked) throw new Error('Client revoked');
    const token = tokens.get(clientId) ?? 'preview-not-a-real-credential';
    const endpoint = 'http://127.0.0.1:8766/mcp';
    const codex = `[mcp_servers.arcrelay]\ncommand = "/Applications/ArcRelay.app/Contents/MacOS/arcrelay-desktop"\nargs = ["--mcp-stdio"]\nenv = { ARCRELAY_MCP_TOKEN = "${token}" }\n`;
    const claudeDesktop = JSON.stringify({ mcpServers: { arcrelay: { command: '/Applications/ArcRelay.app/Contents/MacOS/arcrelay-desktop', args: ['--mcp-stdio'], env: { ARCRELAY_MCP_TOKEN: token } } } }, null, 2);
    const httpJson = JSON.stringify({ mcpServers: { arcrelay: { type: 'http', url: endpoint, headers: { Authorization: `Bearer ${token}` } } } }, null, 2);
    return { clientId, endpoint, codex, claudeDesktop, httpJson, codexHttp: `[mcp_servers.arcrelay]\nurl = "${endpoint}"\nhttp_headers = { Authorization = "Bearer ${token}" }`, installPrompt: `请添加 ArcRelay MCP，保留已有配置，不要在回复里重复令牌。\nCodex:\n${codex}\nClaude Desktop:\n${claudeDesktop}` };
  },
  async listMcpConfigurationChanges(_limit = 20): Promise<ConfigurationChange[]> { return []; },
};
