export function toggleSelectedId(selectedIds: number[], id: number) {
  return selectedIds.includes(id)
    ? selectedIds.filter((selectedId) => selectedId !== id)
    : [...selectedIds, id];
}

export function appendSelectedIds(selectedIds: number[], ids: number[]) {
  const next = [...selectedIds];
  for (const id of ids) {
    if (!next.includes(id)) next.push(id);
  }
  return next;
}

export function selectionOrder(selectedIds: number[], id: number) {
  const index = selectedIds.indexOf(id);
  return index < 0 ? null : index + 1;
}
