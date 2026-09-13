import test from 'node:test';
import assert from 'node:assert/strict';
import { diagnosticSnapshot, summarize, start, rendererIdentity } from './web-performance-probe.mjs';

test('renderer identity reports masked, unmasked and unavailable contexts', () => {
  assert.deepEqual(rendererIdentity(null), { available: false });
  assert.deepEqual(rendererIdentity({ getContext() { throw new Error('unavailable'); } }), { available: false });
  for (const unmasked of [false, true]) {
    const canvas = { getContext(kind) {
      assert.equal(kind, 'webgl2');
      return {
        VENDOR: 'vendor', RENDERER: 'renderer',
        getExtension: () => unmasked ? { UNMASKED_VENDOR_WEBGL: 'raw-vendor', UNMASKED_RENDERER_WEBGL: 'raw-renderer' } : null,
        getParameter: key => key,
      };
    } };
    assert.deepEqual(rendererIdentity(canvas), {
      available: true, unmasked,
      vendor: unmasked ? 'raw-vendor' : 'vendor', renderer: unmasked ? 'raw-renderer' : 'renderer',
    });
  }
});

test('nearest rank, thresholds, empty/single windows and input immutability', () => {
  assert.equal(summarize([]), null);
  assert.equal(summarize([8]).p95Ms, 8);
  assert.equal(summarize([0]).effectiveFps, null);
  const samples = Array.from({ length: 20 }, (_, i) => 20 - i);
  const result = summarize(samples);
  assert.equal(result.p95Ms, 19);
  assert.equal(result.meanMs, 10.5);
  assert.equal(result.maxMs, 20);
  assert.equal(result.p99Ms, 20);
  assert.equal(result.effectiveFps, 1000 / 10.5);
  assert.equal(result.over16_67ms, 4);
  assert.equal(result.within60fpsRatio, 0.8);
  assert.equal(result.allFramesMeet60fps, false);
  assert.equal(result.allFramesMeet30fps, true);
  assert.equal(result.p95Meets60fps, false);
  assert.equal(result.p95Meets30fps, true);
  assert.equal(samples[0], 20);
  assert.equal(summarize([10, 40, 110]).over33ms, 2);
  assert.equal(summarize([10, 40, 110]).over100ms, 1);
  assert.equal(summarize([10, 40, 110]).over16_67ms, 2);
});

test('summarize rejects non-finite or negative frame intervals', () => {
  for (const samples of [[NaN], [Infinity], [-1], ['16']]) {
    assert.throws(() => summarize(samples), /Invalid frame samples/);
  }
});

test('diagnostic snapshots whitelist bounded frame timing numbers', () => {
  const result = diagnosticSnapshot(() => ({
    app_state: 'Playing', elapsed_seconds: 2, humans: 1, npcs: 7,
    frame_timing: {
      generation: 3, rendered_frames: 120, total_main_app_ms_sum: 2400,
      fixed_ms_sum: 900, fixed_tick_count: 120, remainder_ms_sum: 1500,
      over16_67ms: 4, over33_33ms: 1, max_fixed_ticks_per_frame: 2,
      slowest_total_main_app_ms: 50, slowest_fixed_ms: 30,
      slowest_fixed_tick_count: 2, slowest_max_fixed_tick_ms: 18,
      slowest_remainder_ms: 20, unbounded_payload: { nested: true },
    },
    arbitrary: 'must not be copied',
  }));
  assert.equal(result.frame_timing.rendered_frames, 120);
  assert.equal(result.frame_timing.slowest_fixed_tick_count, 2);
  assert.equal(result.arbitrary, undefined);
  assert.equal(result.frame_timing.unbounded_payload, undefined);

  const malformed = diagnosticSnapshot(() => ({
    frame_timing: {
      rendered_frames: Infinity, fixed_ms_sum: NaN,
      slowest_remainder_ms: -1, ignored: 4,
    },
  }));
  assert.equal(malformed, null);
  assert.equal(diagnosticSnapshot(() => ({ frame_timing: ['not', 'an object'] })), null);
});

test('invalid durations and labels fail before browser access', () => {
  for (const options of [{ seconds: 0 }, { seconds: Infinity }, { seconds: 3601 },
    { windowSeconds: 0 }, { windowSeconds: 61 }, { label: null }, { label: 'a'.repeat(201) }]) {
    assert.throws(() => start(options), /Invalid capture options/);
  }
});

test('capture streams complete/partial windows and cleans up listeners on stop', async () => {
  let now = 0, callback, observerInstance;
  const records = [], requestUrls = [], cancelled = [];
  const document = Object.assign(new EventTarget(), { hidden: false, baseURI: 'https://turfrace.localc/proxy/8080/' });
  const window = new EventTarget();
  class FakePerformanceObserver {
    constructor(observerCallback) { this.callback = observerCallback; observerInstance = this; }
    observe(options) { this.options = options; }
    disconnect() { this.disconnected = true; }
  }
  const replacements = {
    performance: { now: () => now }, document, window, PerformanceObserver: FakePerformanceObserver,
    navigator: { userAgent: 'test-browser', hardwareConcurrency: 2 },
    innerWidth: 1280, innerHeight: 720, devicePixelRatio: 1,
    requestAnimationFrame: fn => { callback = fn; return 1; },
    cancelAnimationFrame: id => cancelled.push(id),
    fetch: async (url, options) => { requestUrls.push(String(url)); records.push(JSON.parse(options.body)); return { ok: true }; },
  };
  const originals = new Map(Object.keys(replacements).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]));
  let capture, releaseUploads;
  try {
    for (const [key, value] of Object.entries(replacements)) {
      Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
    }
    const diagnostics = { app_state: 'Playing', purpose: 'Playable', phase: 'Running', elapsed_seconds: 0, humans: 1, npcs: 7, ready: true,
      frame_timing: { generation: 1, rendered_frames: 12, total_main_app_ms_sum: 240, fixed_ms_sum: 120,
        fixed_tick_count: 12, remainder_ms_sum: 120, over16_67ms: 2, over33_33ms: 1,
        max_fixed_ticks_per_frame: 2, slowest_total_main_app_ms: 40, slowest_fixed_ms: 20,
        slowest_fixed_tick_count: 2, slowest_max_fixed_tick_ms: 12, slowest_remainder_ms: 20 } };
    capture = start({ seconds: 2, windowSeconds: 1, diagnostics: () => diagnostics });
    assert.equal(observerInstance.options.type, 'longtask');
    observerInstance.callback({ getEntries: () => [
      { startTime: -100, duration: 90 }, { startTime: 0, duration: 75 },
    ] });
    assert.throws(() => start(), /already active/);
    for (const time of [10, 20]) { now = time; callback(now); }
    document.hidden = true;
    document.dispatchEvent(new Event('visibilitychange'));
    document.hidden = false;
    document.dispatchEvent(new Event('visibilitychange'));
    for (const time of [1020, 1030, 1040]) { now = time; callback(now); }
    diagnostics.phase = 'Paused';
    now = 1240;
    await capture.stop();
    const windows = records.filter(record => record.type === 'window');
    assert.equal(windows.length, 2);
    assert.equal(windows[0].frames, 2);
    assert.equal(windows[0].maxMs, 1000);
    assert.equal(windows[0].callbackArrival.maxMs, 1000);
    assert.equal(windows[0].p95Meets30fps, false);
    assert.equal(windows[0].hidden, true);
    assert.equal(windows[0].visibilityChanges, 2);
    assert.equal(windows[0].longTaskCount, 1);
    assert.equal(windows[0].longTaskTotalMs, 75);
    assert.equal(windows[0].longTaskMaxMs, 75);
    assert.equal(windows[0].diagnostics.phase, 'Running');
    assert.equal(windows[0].diagnostics.humans, 1);
    assert.equal(windows[0].diagnostics.frame_timing.max_fixed_ticks_per_frame, 2);
    assert.equal(typeof records[0].captureId, 'string');
    assert.ok(records[0].captureId.length > 0);
    assert.ok(records.every(record => record.captureId === records[0].captureId));
    assert.equal(records[0].diagnostics.humans, 1);
    assert.equal(records[0].diagnostics.npcs, 7);
    assert.equal(records[0].diagnostics.phase, 'Running');
    assert.equal(requestUrls[0], './__performance/record');
    assert.equal(windows[1].frames, 2);
    assert.equal(windows[1].meanMs, 10);
    assert.equal(windows[1].callbackArrival.meanMs, 10);
    assert.equal(windows[1].jsHeapBytes, null);
    assert.equal(windows[1].diagnostics.phase, 'Paused');
    assert.equal(records.at(-1).type, 'complete');
    assert.equal(records.at(-1).unsampledTailMs, 200);
    assert.equal(globalThis.__turfracePerformanceCapture, undefined);
    assert.equal(observerInstance.disconnected, true);
    assert.deepEqual(cancelled, [1]);
    window.dispatchEvent(new Event('error'));
    await capture.stop();
    assert.equal(records.length, 6, 'stop must be idempotent and remove event listeners');

    // Saturate the ordinary queue with a stalled server, then stop. Terminal
    // records must survive even when an ordinary event is explicitly dropped.
    const send = globalThis.fetch;
    const gate = new Promise(resolve => { releaseUploads = resolve; });
    globalThis.fetch = async (...args) => { await gate; return send(...args); };
    capture = start({ seconds: 2, windowSeconds: 1, diagnostics: () => diagnostics });
    now += 10; callback(now);
    now += 10; callback(now);
    for (let i = 0; i < 16; i++) window.dispatchEvent(new Event('error'));
    const stopping = capture.stop();
    releaseUploads();
    await stopping;
    assert.equal(records.length, 24);
    assert.notEqual(records.at(-1).captureId, records[0].captureId);
    assert.ok(records.slice(6).every(record => record.captureId === records.at(-1).captureId));
    assert.equal(records.at(-2).type, 'window');
    assert.equal(records.at(-2).frames, 1);
    assert.equal(records.at(-1).type, 'complete');
    assert.equal(records.at(-1).droppedRecords, 1);
  } finally {
    releaseUploads?.();
    await capture?.stop();
    for (const [key, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else delete globalThis[key];
    }
  }
});

test('callback-arrival stats detect a stall hidden by advancing rAF timestamps and reset by actual time', async () => {
  let now = 0, callback;
  const records = [];
  const document = Object.assign(new EventTarget(), { hidden: false });
  const window = new EventTarget();
  const replacements = {
    performance: { now: () => now }, document, window, PerformanceObserver: undefined,
    navigator: { userAgent: 'test-browser', hardwareConcurrency: 2 },
    innerWidth: 1280, innerHeight: 720, devicePixelRatio: 1,
    requestAnimationFrame: fn => { callback = fn; return 1; },
    cancelAnimationFrame: () => {},
    fetch: async (_url, options) => { records.push(JSON.parse(options.body)); return { ok: true }; },
  };
  const originals = new Map(Object.keys(replacements).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]));
  let capture;
  try {
    for (const [key, value] of Object.entries(replacements)) {
      Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
    }
    capture = start({ seconds: 3, windowSeconds: 1 });

    // Every supplied rAF timestamp advances by one 60-Hz frame. The arrival
    // clock has a 200 ms gap between the second and third callbacks.
    const arrivals = [10, 20, 220, ...Array.from({ length: 78 }, (_, i) => 230 + i * 10)];
    for (const [index, arrival] of arrivals.entries()) {
      now = arrival;
      callback((index + 1) * (1000 / 60));
    }
    now = 1010;
    callback((arrivals.length + 1) * (1000 / 60));
    now = 1020;
    callback((arrivals.length + 2) * (1000 / 60));

    now = 1200;
    await capture.stop();
    const startRecord = records.find(record => record.type === 'start');
    assert.match(startRecord.timing.legacyRafTimestamp, /rAF-supplied timestamps/);
    assert.match(startRecord.timing.callbackArrival, /performance\.now\(\)/);
    assert.match(startRecord.timing.presentation, /neither metric measures GPU/);
    const windows = records.filter(record => record.type === 'window');
    assert.equal(windows.length, 2);
    assert.ok(windows[0].maxMs < 17, 'legacy timestamp clock remains regular');
    assert.equal(windows[0].callbackArrival.maxMs, 200);
    assert.equal(windows[0].startMs, 0);
    assert.equal(windows[0].endMs, 1000);
    assert.equal(windows[1].startMs, 1000);
    assert.equal(windows[1].endMs, 1200);
    assert.equal(windows[1].frames, 2);
    assert.equal(windows[1].callbackArrival.frames, 2);
    assert.equal(windows[1].callbackArrival.meanMs, 10);
    assert.equal(records.at(-1).unsampledTailMs, 180);
  } finally {
    await capture?.stop();
    for (const [key, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else delete globalThis[key];
    }
  }
});
