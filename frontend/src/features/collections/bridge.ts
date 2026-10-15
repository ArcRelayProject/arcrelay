import type { CollectionRequest, CollectionResponse } from "../../ipc/generated";
export async function collectionRequest(request: CollectionRequest): Promise<CollectionResponse> {
  if ("__TAURI_INTERNALS__" in window) {
    const { invoke } = await import("../../ipc/client");
    return invoke("collection_request", { request });
  }
  const { previewRequest } = await import("./preview");
  return previewRequest(request);
}
