import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');

test('the actual app mounts the staged workspace and incoming requests do not navigate', () => {
  const app = source('./App.svelte');
  assert.match(app, /loadTransferPage = createCachedLoader\(async \(\) => \(await import\("\.\/TransferWorkspace\.svelte"\)\)/);
  assert.doesNotMatch(app, /if \(page !== "workflows"\) page = "transfer"/);
});

test('native file drops call staging only and no recipient is implicitly selected', () => {
  const workspace = source('./TransferWorkspace.svelte');
  const dropHandler = workspace.split('onDragDropEvent(event => {')[1]?.split('})).catch(report)')[0];
  assert.ok(dropHandler);
  assert.match(dropHandler, /addPaths\(event\.payload\.paths\)/);
  assert.doesNotMatch(dropHandler, /sendTransfer|sendTo|peer/);
  assert.doesNotMatch(workspace, /peers\[0\]|devices\[0\]|selectedPeer|selectedDevice/);
});

test('the metadata command is registered and uses a local blocking worker only', () => {
  const command = source('../../src/commands/transfer.rs').split('pub async fn inspect_transfer_files')[1]?.split('#[arcrelay_desktop_ipc::command]')[0];
  assert.ok(command);
  assert.match(command, /spawn_blocking\(move \|\| draft::inspect_paths\(paths\)\)/);
  assert.doesNotMatch(command, /backend|transfer_service|send_transfer/);
  assert.match(source('../../src/main.rs'), /commands::inspect_transfer_files,/);
  assert.match(source('./bridge.transfers.ts'), /invoke\("inspect_transfer_files", \{ paths \}\)/);
});

test('notification clicks route to the exact incoming request', () => {
  const app = source('./App.svelte');
  const bridge = source('./bridge.transfers.ts');
  const workspace = source('./TransferWorkspace.svelte');
  assert.match(bridge, /listen\("open-transfer-request"/);
  assert.match(bridge, /invoke\("take_pending_transfer_request"\)/);
  assert.match(app, /openTransferRequest\(transferId\)/);
  assert.match(app, /openRequest=\{transferOpenRequest\}/);
  assert.match(workspace, /openPanel\(\{ type: 'incoming', id: openRequest\.transferId \}\)/);
});

test('incoming requests expose cancellation and close stale drawers', () => {
  const workspace = source('./TransferWorkspace.svelte');
  assert.match(workspace, /class="icon-button dismiss-request"/);
  assert.match(workspace, /taskAction\(request, 'cancel'\)/);
  assert.match(workspace, /previousRequest\?\.status === 'awaitingApproval'/);
  assert.match(workspace, /nextRequest\?\.status !== 'awaitingApproval'/);
});
