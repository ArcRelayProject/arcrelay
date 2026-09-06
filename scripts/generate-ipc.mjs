import { spawnSync } from 'node:child_process';
const result = spawnSync('cargo', ['test', '-p', 'arcrelay-desktop', '--bin', 'arcrelay-desktop', 'ipc_contract_matches_rust', '--', '--nocapture'], {
  stdio: 'inherit', env: { ...process.env, ARCRELAY_GENERATE_IPC: '1' },
});
if (result.error) throw result.error;
process.exit(result.status ?? 1);
