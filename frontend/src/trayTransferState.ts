import type { TransferView } from './types.ts';

export function trayTransferProgress(task: Pick<TransferView, 'status' | 'totalBytes' | 'completedBytes'>) {
  if (task.status === 'completed') return 100;
  if (task.totalBytes <= 0) return 0;
  // Byte completion alone is not a receipt: the backend must confirm completion.
  return Math.max(0, Math.min(100, Math.floor(task.completedBytes / task.totalBytes * 100)));
}

export function trayTransferStatus(status: TransferView['status']) {
  const labels: Record<TransferView['status'], string> = {
    preparing: '正在准备', awaitingApproval: '等待对方接收', connecting: '正在连接',
    transferring: '正在传输', paused: '传输已暂停', completed: '文件已送达',
    rejected: '对方已拒绝', cancelled: '发送已取消', failed: '发送未完成',
  };
  return labels[status];
}

export function trayTransferBytes(bytes: number) {
  if (bytes < 1024) return `${Math.max(0, bytes)} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}
