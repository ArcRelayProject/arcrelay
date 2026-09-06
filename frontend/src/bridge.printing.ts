import { invoke, listen } from './ipc/client';
import type { UnlistenFn } from '@tauri-apps/api/event';
import type { PrinterSharingSnapshot, PrintJobActivitySnapshot } from "./types";
export const printingBridge = {
    async onPrintJobActivityError(listener: (message: string) => void): Promise<UnlistenFn> {
        return listen("print-job-activity-error", ({ payload }) => listener(payload.message));
    },
    async getPrinterSharingState(): Promise<PrinterSharingSnapshot> {
        return invoke("get_printer_sharing_state");
    },
    async onPrintJobActivity(listener: (value: PrintJobActivitySnapshot) => void): Promise<UnlistenFn> {
        return listen("print-job-activity", (event) => listener(event.payload));
    },
    async observePrintJobs(): Promise<UnlistenFn> {
        await invoke("observe_print_jobs", { enabled: true });
        return () => { void invoke("observe_print_jobs", { enabled: false }).catch(console.error); };
    },
    async getPrintJobActivity(): Promise<PrintJobActivitySnapshot> {
        return invoke("get_print_job_activity");
    },
    async refreshPrinterSharingState(): Promise<PrinterSharingSnapshot> {
        return invoke("refresh_printer_sharing_state");
    },
    async publishPrinter(localPrinterId: string): Promise<PrinterSharingSnapshot> {
        return invoke("publish_printer", { localPrinterId });
    },
    async suspendPrinterShare(shareId: string): Promise<PrinterSharingSnapshot> {
        return invoke("suspend_printer_share", { shareId });
    }
};
