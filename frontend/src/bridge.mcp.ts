import { invoke } from './ipc/client';
import type { McpPermissions } from './ipc/generated';
export const mcpBridge = {
  listMcpClients: () => invoke('list_mcp_clients'),
  createMcpClient: (name: string, permissions: McpPermissions) => invoke('create_mcp_client', { name, permissions }),
  setMcpClientPermissions: (clientId: string, permissions: McpPermissions) => invoke('set_mcp_client_permissions', { clientId, permissions }),
  revokeMcpClient: (clientId: string) => invoke('revoke_mcp_client', { clientId }),
  rotateMcpClientToken: (clientId: string) => invoke('rotate_mcp_client_token', { clientId }),
  getMcpClientConfig: (clientId: string) => invoke('get_mcp_client_config', { clientId }),
  listMcpConfigurationChanges: (limit = 20) => invoke('list_mcp_configuration_changes', { limit }),
};
