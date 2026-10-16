import type { CollectionRequest, CollectionResponse } from "../../ipc/generated";
export async function readCollections() {
  const records: CollectionResponse["records"] = [];
  let after = "";
  while (records.length < 10000) {
    const result = await collectionRequest({ type: "list", after });
    records.push(...result.records);
    if (!result.records.length) break;
    after = result.records[result.records.length - 1].id;
  }
  return records;
}
export async function collectionRequest(request: CollectionRequest): Promise<CollectionResponse> {
  if ("__TAURI_INTERNALS__" in window) {
    const { invoke } = await import("../../ipc/client");
    return invoke("collection_request", { request });
  }
  const { previewRequest } = await import("./preview");
  return previewRequest(request);
}
