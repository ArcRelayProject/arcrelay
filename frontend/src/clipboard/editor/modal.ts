/** Native dialogs supply focus trapping, inert background and focus restoration. */
export function modal(node: HTMLDialogElement) {
  node.showModal();
  return {
    destroy() {
      node.close();
    },
  };
}
