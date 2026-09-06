import type { BootstrapState } from './types.ts';
export type DesktopStateUpdate = Omit<BootstrapState, 'actions'> & { actions?: BootstrapState['actions'] };

/** Catalog and runtime events may cross in flight; each owns its revision. */
export class DesktopProjection {
  private runtime: Omit<BootstrapState, 'actions'> | null = null;
  private actions: BootstrapState['actions'] | null = null;
  private actionRevision = -1;
  apply(update: DesktopStateUpdate): BootstrapState | null {
    let changed = false;
    if (!this.runtime || update.revision > this.runtime.revision) {
      const { actions: _actions, ...runtime } = update;
      this.runtime = runtime;
      changed = true;
    }
    if (update.actions && update.revision > this.actionRevision) {
      this.actions = update.actions;
      this.actionRevision = update.revision;
      changed = true;
    }
    if (!changed || !this.runtime || !this.actions) return null;
    return { ...this.runtime, actions: this.actions, revision: Math.max(this.runtime.revision, this.actionRevision) };
  }
}
