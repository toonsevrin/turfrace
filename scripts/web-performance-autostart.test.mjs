import test from 'node:test';
import assert from 'node:assert/strict';
import {
  isPlayableRunning,
  performanceEnabled,
  rosterLabel,
  startAutoCapture,
} from './web-performance-autostart.mjs';

test('automatic capture requires the explicit performance query', () => {
  assert.equal(performanceEnabled('?performance'), true);
  assert.equal(performanceEnabled('?seed=42'), false);
  assert.equal(performanceEnabled('?foo=performance'), false);
});

test('autostart waits for a playable running match with at least one human', () => {
  const base = { app_state: 'Playing', purpose: 'Playable', phase: 'Running', humans: 1, npcs: 7 };
  assert.equal(isPlayableRunning(base), true);
  for (const field of ['app_state', 'purpose', 'phase']) {
    assert.equal(isPlayableRunning({ ...base, [field]: 'wrong' }), false);
  }
  assert.equal(isPlayableRunning({ ...base, humans: 0 }), false);
  assert.equal(rosterLabel(base), 'auto-1h-7n');
});

test('autostart does not touch gameplay and starts only after diagnostics become ready', async () => {
  const originalLocation = Object.getOwnPropertyDescriptor(globalThis, 'location');
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, 'window');
  const originalDocument = Object.getOwnPropertyDescriptor(globalThis, 'document');
  const diagnostics = { app_state: 'Playing', purpose: 'Playable', phase: 'Running', humans: 0, npcs: 7 };
  const window = { __turfraceDiagnostics: diagnostics };
  const overlay = { style: {}, dataset: {}, setAttribute() {}, textContent: '' };
  const document = {
    body: { append() {} },
    createElement() { return overlay; },
  };
  let options = null;
  try {
    Object.defineProperty(globalThis, 'location', { value: { search: '?performance' }, configurable: true });
    Object.defineProperty(globalThis, 'window', { value: window, configurable: true });
    Object.defineProperty(globalThis, 'document', { value: document, configurable: true });
    const controller = startAutoCapture({ pollMs: 5, statusPollMs: 5, probe: value => {
      options = value;
      window.__turfracePerformanceCapture = { stop() {} };
      return window.__turfracePerformanceCapture;
    } });
    await new Promise(resolve => setTimeout(resolve, 10));
    assert.equal(options, null);
    diagnostics.humans = 1;
    await new Promise(resolve => setTimeout(resolve, 15));
    assert.equal(options.label, 'auto-1h-7n');
    assert.equal(options.seconds, 180);
    diagnostics.frame_timing = { rendered_frames: 12, slowest_total_main_app_ms: 40 };
    assert.deepEqual(options.diagnostics().frame_timing, diagnostics.frame_timing);
    assert.equal(window.__turfraceDiagnostics, diagnostics);
    controller.stop();
  } finally {
    for (const [key, descriptor] of [['location', originalLocation], ['window', originalWindow], ['document', originalDocument]]) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else delete globalThis[key];
    }
  }
});
