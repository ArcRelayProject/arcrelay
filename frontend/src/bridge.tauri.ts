import { mcpBridge } from './bridge.mcp';
import { systemBridge } from './bridge.system';
import { inputBridge } from './bridge.input';
import { transfersBridge } from './bridge.transfers';
import { printingBridge } from './bridge.printing';
import { filesBridge } from './bridge.files';
import { actionsBridge } from './bridge.actions';
import { automationsBridge } from './bridge.automations';
export const bridge = { ...systemBridge, ...mcpBridge, ...inputBridge, ...transfersBridge, ...printingBridge, ...filesBridge, ...actionsBridge, ...automationsBridge };
