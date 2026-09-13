import test from 'node:test';
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

const server = process.env.TURFRACE_BROWSER_INTEGRATION_URL;
const exec = promisify(execFile);

test('real lobby navigation restores 1080p and verifies both target rosters', {
  skip: !server && 'set TURFRACE_BROWSER_INTEGRATION_URL to an existing release server',
  timeout: 180000,
}, async () => {
  const root = await mkdtemp(join(tmpdir(), 'turfrace-browser-integration-'));
  try {
    // Serialize software-rendered browser processes on the shared node.
    for (const bots of [7, 11]) {
      const output = join(root, String(bots));
      await exec(process.execPath, [fileURLToPath(new URL('./browser-performance.mjs', import.meta.url)),
        '--url', server, '--bots', String(bots), '--seconds', '2',
        '--width', '1920', '--height', '1080', '--output', output], {
        timeout: 80000, maxBuffer: 1024 * 1024,
      });
      const metadata = JSON.parse(await readFile(join(output, 'metadata.json'), 'utf8'));
      assert.equal(metadata.diagnosticsVerified, true);
      assert.equal(metadata.scenario.roster.humans, 1);
      assert.equal(metadata.scenario.roster.npcs, bots);
      assert.equal(metadata.scenario.roster.verified, true);
      const timing = JSON.parse(await readFile(join(output, 'timing.json'), 'utf8'));
      assert.equal(timing.hidden, false);
      assert.equal(timing.timedOut, false);
      assert.ok(timing.frames > 0);
      for (const name of ['before.png', 'after.png']) {
        const png = await readFile(join(output, name));
        assert.equal(png.subarray(1, 4).toString(), 'PNG');
        assert.equal(png.readUInt32BE(16), 1920);
        assert.equal(png.readUInt32BE(20), 1080);
      }
    }
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
