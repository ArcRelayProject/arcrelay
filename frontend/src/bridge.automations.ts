import { invoke, listen } from "./ipc/client";
import type {
  AutomationDefinition,
  AutomationEvent,
  AutomationActivity,
  AutomationTrigger,
} from "./ipc/generated";
export const automationsBridge = {
  previewAutomationSchedule: (trigger: AutomationTrigger) =>
    invoke("preview_automation_schedule", { trigger }),
  openAutomationScreenPermission: () =>
    invoke("open_automation_screen_permission"),
  pickAutomationPath: (directory: boolean) =>
    invoke("pick_automation_path", { directory }),
  listAutomations: () => invoke("list_automations"),
  saveAutomation: (definition: AutomationDefinition) =>
    invoke("save_automation", { definition }),
  setAutomationEnabled: (automationId: string, enabled: boolean) =>
    invoke("set_automation_enabled", { automationId, enabled }),
  deleteAutomation: (automationId: string) =>
    invoke("delete_automation", { automationId }),
  runAutomation: (automationId: string) =>
    invoke("run_automation", { automationId }),
  testAutomation: (
    definition: AutomationDefinition,
    event: AutomationEvent | null = null,
  ) => invoke("test_automation", { definition, event }),
  automationCapabilities: () => invoke("automation_capabilities"),
  checkAutomation: (definition: AutomationDefinition) =>
    invoke("check_automation", { definition }),
  listAutomationActivities: (automationId: string | null = null, limit = 100) =>
    invoke("list_automation_activities", { automationId, limit }),
  confirmAutomationActivity: (activityId: string) =>
    invoke("confirm_automation_activity", { activityId }),
  cancelAutomationActivity: (activityId: string) =>
    invoke("cancel_automation_activity", { activityId }),
  clearAutomationActivities: () => invoke("clear_automation_activities"),
  onAutomationConfiguration: (listener: () => void) => listen("automation-configuration", listener),
  onAutomationActivity: (listener: (activity: AutomationActivity) => void) =>
    listen("automation-activity", (event) => listener(event.payload)),
};
