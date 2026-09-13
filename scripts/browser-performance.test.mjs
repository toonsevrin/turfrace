import test from 'node:test';
import assert from 'node:assert/strict';
import {
  assessSimulationProgress,
  parseArgs,
  performanceUrl,
  parseStoredNpcCount,
  sampleRaf,
  summarizeFrameIntervals,
  validateDiagnostics,
  SIXTY_FPS_BUDGET_MS,
} from './browser-performance.mjs';

test('CLI bounds duration, roster and capture-only options', () => {
  const options = parseArgs(['--url', 'http://127.0.0.1:8080', '--seconds', '12', '--bots', '11', '--capture-only']);
  assert.equal(options.seconds, 12);
  assert.equal(options.bots, 11);
  assert.equal(options.captureOnly, true);
  for (const args of [['--seconds', '0'], ['--seconds', '301'], ['--bots', '8'], ['--url', 'file:///dist']]) {
    assert.throws(() => parseArgs(args));
  }
});

test('performance URL preserves the route and opts diagnostics in explicitly', () => {
  const result = new URL(performanceUrl('http://localhost:8080/game?seed=42#match'));
  assert.equal(result.searchParams.get('seed'), '42');
  assert.equal(result.searchParams.get('performance'), '1');
  assert.equal(result.hash, '#match');
});

test('stored lobby NPC parsing distinguishes default, valid, and malformed state', () => {
  assert.equal(parseStoredNpcCount(null), 0);
  assert.equal(parseStoredNpcCount('{"npc_count":11}'), 11);
  assert.equal(parseStoredNpcCount('{"npc_count":-1}'), null);
  assert.equal(parseStoredNpcCount('not JSON'), null);
});

test('metadata validation rejects missing, malformed, and mismatched ECS diagnostics', () => {
  const valid = { app_state: 'Playing', purpose: 'Playable', phase: 'Running', elapsed_seconds: 2, humans: 1, npcs: 7, ready: true };
  assert.deepEqual(validateDiagnostics(valid, { purpose: 'Playable', phase: 'Running', humans: 1, npcs: 7 }), valid);
  for (const invalid of [null, {}, { ...valid, elapsed_seconds: NaN }, { ...valid, humans: -1 }, { ...valid, ready: 'yes' }]) {
    assert.throws(() => validateDiagnostics(invalid));
  }
  assert.throws(() => validateDiagnostics(valid, { npcs: 11 }), /mismatch/);
});

test('simulation progress reports a slowdown separately from frame pacing', () => {
  const result = assessSimulationProgress([
    { observedAt: 0, elapsed_seconds: 0 },
    { observedAt: 1000, elapsed_seconds: 0.5 },
  ]);
  assert.equal(result.rate, 0.5);
  assert.equal(result.slowdown, true);
  assert.equal(assessSimulationProgress([{ observedAt: 1, elapsed_seconds: 1 }]).slowdown, true);
});

test('rAF capture exposes zero/hidden results and bounds a hung page evaluation', async () => {
  const result = await sampleRaf({ evaluate: async () => ({
    samples: [], timedOut: false, hidden: true, initialHidden: true, finalHidden: true,
    visibilityChanges: 1, initialUnsampledGapMs: null, finalUnsampledGapMs: 0, elapsedMs: 1,
  }) }, 1);
  assert.equal(result.samples.length, 0);
  assert.equal(result.hidden, true);
  await assert.rejects(() => sampleRaf({ evaluate: () => new Promise(() => {}) }, 0.001, { timeoutMs: 10 }), /timed out/);
});

test('rAF summary reports percentile and frame-budget counts without mutating samples', () => {
  const samples = [8, 17, 34, 110];
  const result = summarizeFrameIntervals(samples);
  assert.equal(result.p95Ms, 110);
  assert.equal(result.p99Ms, 110);
  assert.equal(result.maxMs, 110);
  assert.equal(result.over16_67ms, 3);
  assert.equal(result.over33_33ms, 2);
  assert.equal(result.over100ms, 1);
  assert.equal(samples[0], 8);
  assert.equal(SIXTY_FPS_BUDGET_MS > 16 && SIXTY_FPS_BUDGET_MS < 17, true);
  assert.deepEqual(summarizeFrameIntervals([]), { frames: 0, meanMs: null, p95Ms: null, p99Ms: null, maxMs: null });
});
