const now = Date.now();
import { type UnlistenFn } from "@tauri-apps/api/event";
import type { ClipboardCursor, ClipboardHistory, ClipboardItem, ClipboardImageOcr, ClipboardKind, ClipboardLabel, NearbyClipboardPeer, ClipboardPasteMode, ContinuousPasteItemInput } from "./types";
import type { ClipboardSortPreference } from "../types";
import type { ClipboardTimeline, ClipboardTimelinePosition } from "./types";
const mockImageSource = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(`
  <svg xmlns="http://www.w3.org/2000/svg" width="1920" height="1080" viewBox="0 0 1920 1080">
    <defs><linearGradient id="bg" x1="0" y1="0" x2="1" y2="1"><stop stop-color="#17192d"/><stop offset="1" stop-color="#31367a"/></linearGradient></defs>
    <rect width="1920" height="1080" rx="48" fill="url(#bg)"/>
    <text x="220" y="430" fill="#fff" font-family="ui-monospace, monospace" font-size="110" font-weight="700">ArcRelay</text>
    <text x="220" y="650" fill="#d9dcff" font-family="ui-monospace, monospace" font-size="110">选择图片文字</text>
  </svg>
`)}`;
const mockImageOcr: ClipboardImageOcr = {
    text: "ArcRelay\n选择图片文字",
    modelVersion: "mock-ctc-alignment-v1",
    updatedAtMs: now,
    blocks: [
        { text: "ArcRelay", confidence: 0.98, left: 220, top: 320, width: 880, height: 130, points: null, characters: [] },
        { text: "选择图片文字", confidence: 0.97, left: 220, top: 540, width: 660, height: 130, points: null, characters: [] },
    ],
};
const mockLabels: ClipboardLabel[] = [
    { id: "mock-work", name: "工作", color: "#3B82F6", revision: 1, updated_by_device_id: "mock", deleted: false },
    { id: "mock-project", name: "项目", color: "#22C55E", revision: 1, updated_by_device_id: "mock", deleted: false },
    { id: "mock-temporary", name: "临时", color: "#F59E0B", revision: 1, updated_by_device_id: "mock", deleted: false },
    { id: "mock-code", name: "代码", color: "#8B5CF6", revision: 1, updated_by_device_id: "mock", deleted: false },
];
const mockItems: ClipboardItem[] = [
    {
        id: 101,
        kind: "image",
        preview: "图片 · 1,920 × 1,080",
        sourceApp: "截图工具",
        sourceDeviceName: "此电脑",
        syncId: "mock-101",
        firstCapturedAtMs: now - 45000,
        capturedAtMs: now - 45000,
        updatedAtMs: now - 45000,
        sizeBytes: 91249,
        characterCount: null,
        itemCount: 1,
        width: 1920,
        height: 1080,
        sensitive: false,
        favorite: false,
        labels: [],
        available: true,
        textSyntax: "plain",
    },
    {
        id: 102,
        kind: "text",
        preview: "https://www.arcrelay.app/clipboard",
        sourceApp: "Safari",
        sourceDeviceName: "此电脑",
        syncId: "mock-102",
        firstCapturedAtMs: now - 2 * 60000,
        capturedAtMs: now - 2 * 60000,
        updatedAtMs: now - 2 * 60000,
        sizeBytes: 115120,
        characterCount: 37,
        itemCount: 1,
        width: null,
        height: null,
        sensitive: false,
        favorite: false,
        labels: [],
        available: true,
        textSyntax: "url",
    },
    {
        id: 103,
        kind: "html",
        preview: "设备列表 · 陈子的 MacBook Pro · iPhone 15 Pro · 在线",
        safeHtml: "<h3>设备列表</h3><ul><li><strong>陈子的 MacBook Pro</strong> · 在线</li><li>iPhone 15 Pro · 在线</li></ul>",
        sourceApp: "Microsoft Edge",
        sourceDeviceName: "办公室 Mac",
        syncId: "mock-103",
        firstCapturedAtMs: now - 5 * 60000,
        capturedAtMs: now - 5 * 60000,
        updatedAtMs: now - 5 * 60000,
        sizeBytes: 115120,
        characterCount: 31,
        itemCount: 1,
        width: null,
        height: null,
        sensitive: false,
        favorite: true,
        labels: [mockLabels[0]],
        available: true,
        textSyntax: "plain",
    },
    {
        id: 104,
        kind: "image",
        preview: "图片 · 430 × 932",
        sourceApp: "ChatGPT",
        sourceDeviceName: "此电脑",
        syncId: "mock-104",
        firstCapturedAtMs: now - 12 * 60000,
        capturedAtMs: now - 12 * 60000,
        updatedAtMs: now - 12 * 60000,
        sizeBytes: 278620,
        characterCount: null,
        itemCount: 1,
        width: 430,
        height: 932,
        sensitive: false,
        favorite: false,
        labels: [],
        available: true,
        textSyntax: "plain",
    },
    {
        id: 105,
        kind: "text",
        preview: "git commit -m \"feat: 优化剪贴板插入体验\"\ngit push origin main",
        sourceApp: "终端",
        sourceDeviceName: "此电脑",
        syncId: "mock-105",
        firstCapturedAtMs: now - 18 * 60000,
        capturedAtMs: now - 18 * 60000,
        updatedAtMs: now - 18 * 60000,
        sizeBytes: 338,
        characterCount: 53,
        itemCount: 1,
        width: null,
        height: null,
        sensitive: false,
        favorite: false,
        labels: [],
        available: true,
        textSyntax: { code: { language: "shell" } },
    },
    {
        id: 106,
        kind: "text",
        preview: "Which option should I build: 1, 2, or 3?",
        sourceApp: "ChatGPT",
        sourceDeviceName: "此电脑",
        syncId: "mock-106",
        firstCapturedAtMs: now - 24 * 60 * 60000,
        capturedAtMs: now - 24 * 60 * 60000,
        updatedAtMs: now - 24 * 60 * 60000,
        sizeBytes: 99,
        characterCount: 43,
        itemCount: 1,
        width: null,
        height: null,
        sensitive: false,
        favorite: false,
        labels: [],
        available: true,
        textSyntax: "plain",
    },
];
function mockHistory(search: string, kind: ClipboardKind | null, favoriteOnly: boolean, labelIds: string[] = []): ClipboardHistory {
    const query = search.trim().toLocaleLowerCase("zh-CN");
    const entries = mockItems.filter((item) => (!kind || item.kind === kind || (kind === "text" && item.kind === "html")) &&
        (!favoriteOnly || item.favorite) &&
        (labelIds.length === 0 || labelIds.every((labelId) => item.labels.some((label) => label.id === labelId))) &&
        (!query || `${item.preview} ${item.sourceApp ?? ""}`.toLocaleLowerCase("zh-CN").includes(query)));
    return { revision: 1, entries, nextCursor: null, totalCount: entries.length };
}
export const clipboardBridge = {
    timeline: async (position: ClipboardTimelinePosition, sortBy: ClipboardSortPreference, limit = 30): Promise<ClipboardTimeline | null> => {
        const timestamp = (item: ClipboardItem) => sortBy === "createdAt" ? item.firstCapturedAtMs : item.updatedAtMs;
        const sorted = [...mockItems].sort((a, b) => timestamp(b) - timestamp(a) || b.id - a.id);
        const cursorFor = (item: ClipboardItem): ClipboardCursor => ({ sortAtMs: timestamp(item), id: item.id });
        const after = (item: ClipboardItem, cursor: ClipboardCursor) => timestamp(item) > cursor.sortAtMs || (timestamp(item) === cursor.sortAtMs && item.id > cursor.id);
        const before = (item: ClipboardItem, cursor: ClipboardCursor) => timestamp(item) < cursor.sortAtMs || (timestamp(item) === cursor.sortAtMs && item.id < cursor.id);
        let entries: ClipboardItem[];
        let anchor: ClipboardCursor | null = null;
        if (position.type === "around") {
            const index = sorted.findIndex(item => item.id === position.id);
            if (index < 0) return null;
            anchor = cursorFor(sorted[index]);
            entries = sorted.slice(Math.max(0, index - limit), index + limit + 1);
        } else {
            entries = sorted.filter(item => position.type === "newer" ? after(item, position.cursor) : before(item, position.cursor));
            entries = position.type === "newer" ? entries.slice(-limit) : entries.slice(0, limit);
        }
        const first = entries[0];
        const last = entries.at(-1);
        return {
            revision: 1, entries, anchor,
            newerCursor: first && sorted.some(item => after(item, cursorFor(first))) ? cursorFor(first) : null,
            olderCursor: last && sorted.some(item => before(item, cursorFor(last))) ? cursorFor(last) : null,
            totalCount: sorted.length,
        };
    },
    history: async (options: {
        search: string;
        kind: ClipboardKind | null;
        favoriteOnly: boolean;
        labelIds?: string[];
        cursor: ClipboardCursor | null;
        limit?: number;
    }) => mockHistory(options.search, options.kind, options.favoriteOnly, options.labelIds),
    thumbnail: (id: number) => Promise.resolve(id === 101 ? mockImageSource : null),
    imagePreview: (id: number) => Promise.resolve(id === 101 ? mockImageSource : null),
    imageOcr: (id: number) => Promise.resolve(id === 101 ? mockImageOcr : null),
    htmlPreview: (item: ClipboardItem) => Promise.resolve(item.safeHtml ?? null),
    copy: (id: number) => Promise.resolve(),
    copyText: (content: string) => navigator.clipboard?.writeText(content) ?? Promise.resolve(),
    pasteText: (content: string) => navigator.clipboard?.writeText(content) ?? Promise.resolve(),
    paste: (id: number) => Promise.resolve(),
    pasteAs: (id: number, mode: ClipboardPasteMode) => Promise.resolve(),
    remove: (id: number) => Promise.resolve(),
    removeMany: (ids: number[]) => Promise.resolve(ids.length),
    setFavorite: (id: number, favorite: boolean) => Promise.resolve(),
    labels: () => Promise.resolve(mockLabels),
    createLabel: (name: string, color: string) => Promise.resolve({ id: crypto.randomUUID(), name, color, revision: 1, updated_by_device_id: "mock", deleted: false }),
    updateLabel: (labelId: string, name: string, color: string) => Promise.resolve(),
    deleteLabel: (labelId: string) => Promise.resolve(),
    setLabels: (id: number, labelIds: string[]) => Promise.resolve(),
    setLabelMembership: (id: number, labelId: string, attached: boolean) => Promise.resolve(),
    setContextMenuOpen: (open: boolean) => Promise.resolve(),
    editText: (id: number, content: string) => Promise.resolve(),
    textContent: (item: ClipboardItem) => Promise.resolve(item.preview),
    textSegments: async (item: ClipboardItem): Promise<import("./textSegments").TextSliceModel> => {
        return (await import("./textSegments.mock")).mockModel(item.preview);
    },
    joinSegments: async (id: number, version: string, ids: string[]): Promise<string> => {
        return mockItems.find((item) => item.id === id)?.preview ?? "";
    },
    mergeDevices: () => Promise.resolve(3),
    nearbyPeers: async (): Promise<NearbyClipboardPeer[]> => {
        return [{ id: "mock-peer", name: "办公室 Mac", paired: true }];
    },
    sendFiles: (id: number, peerId: string) => Promise.resolve("mock-transfer"),
    clear: () => Promise.resolve(),
    pasteRecords: (ids: number[]) => Promise.resolve(ids.length),
    startContinuousPaste: (items: ContinuousPasteItemInput[]) => Promise.resolve({ current: 0, total: items.length, active: items.length > 0, nextPreview: items[0]?.preview ?? null, triggerShortcut: "Ctrl+V" }),
    hide: () => Promise.resolve(),
    visible: () => Promise.resolve(true),
    pinned: () => Promise.resolve(false),
    setPinnedWindow: (pinned: boolean) => Promise.resolve(),
    startDragging: () => Promise.resolve(),
    onChanged: async (handler: () => void): Promise<UnlistenFn> => () => undefined,
    onOcrChanged: async (handler: (id: number) => void): Promise<UnlistenFn> => () => undefined,
    onShown: async (handler: () => void): Promise<UnlistenFn> => () => undefined,
    onHidden: async (handler: () => void): Promise<UnlistenFn> => () => undefined,
    onPinChanged: async (handler: (pinned: boolean) => void): Promise<UnlistenFn> => () => undefined,
    onContinuousPasteError: async (handler: (error: string) => void): Promise<UnlistenFn> => () => undefined
};
