// Import from the diagnostic server in the game tab's DevTools console.
const FRAME_60_MS = 1000 / 60;
const FRAME_30_MS = 1000 / 30;

function nearestRank(sorted, quantile) {
  return sorted[Math.max(0, Math.ceil(sorted.length * quantile) - 1)];
}

export function summarize(samples) {
  if (!Array.isArray(samples) || samples.some(ms =>
    typeof ms !== 'number' || !Number.isFinite(ms) || ms < 0)) {
    throw new Error('Invalid frame samples');
  }
  if (!samples.length) return null;
  const sorted = [...samples].sort((a, b) => a - b);
  const totalMs = samples.reduce((a, b) => a + b, 0);
  const meanMs = totalMs / samples.length;
  const over16_67ms = samples.filter(ms => ms > FRAME_60_MS).length;
  const over33_33ms = samples.filter(ms => ms > FRAME_30_MS).length;
  return {
    frames: samples.length,
    meanMs,
    p95Ms: nearestRank(sorted, 0.95),
    p99Ms: nearestRank(sorted, 0.99),
    maxMs: sorted.at(-1),
    effectiveFps: meanMs > 0 ? 1000 / meanMs : null,
    over16_67ms,
    over33_33ms,
    // Keep the old names for consumers of the initial probe format.
    over33ms: over33_33ms,
    over100ms: samples.filter(ms => ms > 100).length,
    within60fpsRatio: (samples.length - over16_67ms) / samples.length,
    within30fpsRatio: (samples.length - over33_33ms) / samples.length,
    allFramesMeet60fps: over16_67ms === 0,
    allFramesMeet30fps: over33_33ms === 0,
    p95Meets60fps: nearestRank(sorted, 0.95) <= FRAME_60_MS,
    p95Meets30fps: nearestRank(sorted, 0.95) <= FRAME_30_MS,
  };
}

const FRAME_TIMING_DURATION_LIMIT_MS = 1e12;
const FRAME_TIMING_COUNT_LIMIT = 1e9;

function copyBoundedNumber(target, source, key, limit = FRAME_TIMING_DURATION_LIMIT_MS) {
  if (typeof source[key] === 'number' && Number.isFinite(source[key]) &&
      source[key] >= 0 && source[key] <= limit) target[key] = source[key];
}

function copyBoundedInteger(target, source, key, limit = FRAME_TIMING_COUNT_LIMIT) {
  if (Number.isInteger(source[key]) && source[key] >= 0 && source[key] <= limit) target[key] = source[key];
}

function copyFrameTiming(value, target) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return;
  copyBoundedInteger(target, value, 'generation', Number.MAX_SAFE_INTEGER);
  copyBoundedInteger(target, value, 'rendered_frames');
  copyBoundedNumber(target, value, 'total_main_app_ms_sum');
  copyBoundedNumber(target, value, 'fixed_ms_sum');
  copyBoundedInteger(target, value, 'fixed_tick_count');
  copyBoundedNumber(target, value, 'remainder_ms_sum');
  copyBoundedInteger(target, value, 'over16_67ms');
  copyBoundedInteger(target, value, 'over33_33ms');
  copyBoundedInteger(target, value, 'max_fixed_ticks_per_frame');
  copyBoundedNumber(target, value, 'slowest_total_main_app_ms');
  copyBoundedNumber(target, value, 'slowest_fixed_ms');
  copyBoundedInteger(target, value, 'slowest_fixed_tick_count');
  copyBoundedNumber(target, value, 'slowest_max_fixed_tick_ms');
  copyBoundedNumber(target, value, 'slowest_remainder_ms');
  copyBoundedInteger(target, value, 'slowest_frame_number');
}

export function diagnosticSnapshot(readDiagnostics) {
  if (typeof readDiagnostics !== 'function') return null;
  try {
    const value = readDiagnostics();
    if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
    // Keep metadata bounded and tied to the ECS bridge's public shape. This
    // also prevents a page-owned object from becoming an unbounded record.
    const snapshot = {};
    for (const key of ['app_state', 'purpose', 'phase']) {
      if (typeof value[key] === 'string' && value[key].length <= 64) snapshot[key] = value[key];
    }
    if (Number.isFinite(value.elapsed_seconds) && value.elapsed_seconds >= 0 && value.elapsed_seconds <= 1e9) {
      snapshot.elapsed_seconds = value.elapsed_seconds;
    }
    for (const key of ['humans', 'npcs']) {
      if (Number.isInteger(value[key]) && value[key] >= 0 && value[key] <= 256) snapshot[key] = value[key];
    }
    if (typeof value.ready === 'boolean') snapshot.ready = value.ready;
    const frameTiming = {};
    copyFrameTiming(value.frame_timing, frameTiming);
    if (Object.keys(frameTiming).length) snapshot.frame_timing = frameTiming;
    return Object.keys(snapshot).length ? snapshot : null;
  } catch {
    return null;
  }
}

export function rendererIdentity(canvas) {
  try {
    const gl = canvas?.getContext?.('webgl2');
    if (!gl) return { available: false };
    const debug = gl.getExtension('WEBGL_debug_renderer_info');
    return {
      available: true,
      vendor: gl.getParameter(debug?.UNMASKED_VENDOR_WEBGL ?? gl.VENDOR),
      renderer: gl.getParameter(debug?.UNMASKED_RENDERER_WEBGL ?? gl.RENDERER),
      unmasked: !!debug,
    };
  } catch { return { available: false }; }
}

export function start({ seconds = 180, windowSeconds = 5, label = 'manual-match', diagnostics } = {}) {
  if (!Number.isFinite(seconds) || seconds < 1 || seconds > 3600 ||
      !Number.isFinite(windowSeconds) || windowSeconds < 1 || windowSeconds > 60 ||
      typeof label !== 'string' || label.length > 200) throw new Error('Invalid capture options');
  if (globalThis.__turfracePerformanceCapture) throw new Error('A capture is already active');
  const started = performance.now();
  // Distinguish concurrent tabs and repeated runs in the shared recorder.
  const captureId = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  // The first callback is a time-to-first-rAF/arrival measurement, not a frame
  // interval. Excluding it avoids charging startup jitter to the first window.
  // Keep the supplied timestamp clock independent from callback-entry arrival
  // time so a stalled callback cannot be hidden by rAF timestamp progression.
  let previousTimestamp = null, previousArrival = null, windowStart = started;
  let samples = [], callbackArrivalSamples = [], frame, timer, stopped = false;
  let windowHadHidden = document.hidden, visibilityChanges = 0;
  let longTaskCount = 0, longTaskTotalMs = 0, longTaskMaxMs = 0;
  let pending = Promise.resolve(), queued = 0, droppedRecords = 0, eventCount = 0;
  const removers = [];
  let longTaskObserver = null;
  const emit = (record, terminal = false) => {
    const entry = { captureId, elapsedMs: performance.now() - started, droppedRecords, ...record };
    // Reserve at most two extra slots for the final partial window and
    // completion; congestion must not silently erase the terminal evidence.
    if (!terminal && queued >= 16) {
      droppedRecords++;
      if (droppedRecords === 1) console.error('Performance server backlog: dropping records, see subsequent droppedRecords count');
      return;
    }
    queued++;
    console.log('TURFRACE_PERFORMANCE', JSON.stringify(entry));
    // Serialize small records, not a memory-growing per-frame trace. Persist
    // each window outside the browser so earlier evidence survives a crash.
    pending = pending.then(async () => {
      // AbortSignal.timeout is not present in every supported browser. Keep
      // the upload bounded there too, otherwise stopping after a dead server
      // can wait forever and obscure the run's outcome.
      const abortSignal = globalThis.AbortSignal?.timeout?.(5000);
      const controller = abortSignal ? null : globalThis.AbortController ? new AbortController() : null;
      let timeout;
      const timeoutError = new Promise((_, reject) => {
        timeout = setTimeout(() => {
          controller?.abort();
          reject(new Error('performance record upload timed out'));
        }, 5000);
      });
      try {
        const request = fetch('./__performance/record', {
          method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(entry),
          ...(abortSignal || controller ? { signal: abortSignal ?? controller.signal } : {}),
        });
        const response = await Promise.race([request, timeoutError]);
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
      } finally {
        clearTimeout(timeout);
      }
    }).catch(error => console.error('Performance record was not persisted:', error))
      .finally(() => { queued--; });
  };
  const listen = (target, name, handler, capture = false) => {
    const bounded = event => {
      eventCount++;
      if (eventCount <= 100) handler(event);
      else if (eventCount === 101) emit({ type: 'event-limit', message: 'Further error/visibility events omitted after 100' });
    };
    target.addEventListener(name, bounded, capture);
    removers.push(() => target.removeEventListener(name, bounded, capture));
  };
  const flush = (now, terminal = false) => {
    const stats = summarize(samples);
    const callbackArrival = summarize(callbackArrivalSamples);
    if (stats) emit({ type: 'window', startMs: windowStart - started, endMs: now - started,
      // A window is eligible for an FPS claim only if it was visible for its
      // entire interval. A hidden-at-flush flag alone misses a hide/show pair.
      hidden: windowHadHidden || document.hidden,
      visibilityChanges,
      longTaskCount, longTaskTotalMs, longTaskMaxMs,
      ...stats,
      callbackArrival,
      jsHeapBytes: performance.memory?.usedJSHeapSize ?? null,
      wasmMemoryBytes: null, gpuMemoryBytes: null,
      ...(typeof diagnostics === 'function' ? { diagnostics: diagnosticSnapshot(diagnostics) } : {}) }, terminal);
    samples = []; callbackArrivalSamples = []; windowStart = now;
    windowHadHidden = document.hidden;
    visibilityChanges = 0;
    longTaskCount = 0; longTaskTotalMs = 0; longTaskMaxMs = 0;
  };
  const stop = async () => {
    if (stopped) return pending;
    stopped = true;
    cancelAnimationFrame(frame); clearTimeout(timer);
    removers.forEach(remove => remove());
    longTaskObserver?.disconnect();
    const stoppedAt = performance.now();
    flush(stoppedAt, true);
    // A timer can run before rAF after a long main-thread stall. Preserve the
    // uncovered tail rather than treating the last healthy rAF as completion.
    emit({ type: 'complete', unsampledTailMs: stoppedAt - (previousArrival ?? started) }, true);
    delete globalThis.__turfracePerformanceCapture;
    await pending;
  };
  globalThis.__turfracePerformanceCapture = { stop };
  if (globalThis.PerformanceObserver) {
    try {
      longTaskObserver = new PerformanceObserver(list => {
        for (const entry of list.getEntries()) {
          if (entry.startTime < started) continue;
          longTaskCount++;
          longTaskTotalMs += entry.duration;
          longTaskMaxMs = Math.max(longTaskMaxMs, entry.duration);
        }
      });
      longTaskObserver.observe({ type: 'longtask', buffered: true });
    } catch (error) {
      console.warn('Long-task profiling is unavailable:', error);
      longTaskObserver = null;
    }
  }
  emit({ type: 'start', label, seconds, windowSeconds, userAgent: navigator.userAgent,
    renderer: rendererIdentity(document.querySelector?.('canvas')),
    wasmArtifacts: (performance.getEntriesByType?.('resource') ?? [])
      .filter(entry => /\.wasm(?:[?#]|$)/.test(entry.name))
      .map(entry => entry.name.split('/').at(-1)).slice(-4),
    viewport: [innerWidth, innerHeight], devicePixelRatio, hardwareConcurrency: navigator.hardwareConcurrency,
    hidden: document.hidden, longTaskObserver: longTaskObserver ? 'active' : 'unavailable',
    timing: {
      legacyRafTimestamp: 'rAF-supplied timestamps (legacy callback-timestamp intervals; not actual arrival)',
      callbackArrival: 'performance.now() sampled at callback entry (actual callback-arrival intervals)',
      presentation: 'not measured; neither metric measures GPU execution or display/compositor presentation',
    },
    scenario: 'manually configured; label is not authoritative seed/roster evidence',
    ...(typeof diagnostics === 'function' ? { diagnostics: diagnosticSnapshot(diagnostics) } : {}) });
  listen(window, 'error', event => emit({ type: 'error', message: String(event.message ?? 'resource error').slice(0, 4000) }));
  listen(window, 'unhandledrejection', event => emit({ type: 'unhandledrejection', message: String(event.reason).slice(0, 4000) }));
  listen(document, 'webglcontextlost', event => emit({ type: 'webglcontextlost', message: String(event.statusMessage ?? '').slice(0, 4000) }), true);
  listen(document, 'visibilitychange', () => {
    visibilityChanges++;
    windowHadHidden ||= document.hidden;
    emit({ type: 'visibilitychange', hidden: document.hidden });
  });
  const tick = now => {
    const arrivalNow = performance.now();
    if (previousTimestamp !== null) samples.push(now - previousTimestamp);
    previousTimestamp = now;
    if (previousArrival !== null) callbackArrivalSamples.push(arrivalNow - previousArrival);
    previousArrival = arrivalNow;
    if (arrivalNow - windowStart >= windowSeconds * 1000) flush(arrivalNow);
    if (arrivalNow - started >= seconds * 1000) { void stop(); return; }
    frame = requestAnimationFrame(tick);
  };
  frame = requestAnimationFrame(tick);
  timer = setTimeout(() => { void stop(); }, seconds * 1000);
  return globalThis.__turfracePerformanceCapture;
}
