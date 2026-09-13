// Opt-in page-side capture. The server only injects this module for an index
// URL carrying ?performance; this module repeats that check as a safe guard.
export const CAPTURE_SECONDS = 180;
export const DIAGNOSTICS_POLL_MS = 250;
export const STATUS_POLL_MS = 500;

export function performanceEnabled(search = globalThis.location?.search ?? '') {
  return new URLSearchParams(search).has('performance');
}

export function isPlayableRunning(diagnostics) {
  return !!diagnostics && diagnostics.app_state === 'Playing' &&
    diagnostics.purpose === 'Playable' && diagnostics.phase === 'Running' &&
    Number.isInteger(diagnostics.humans) && diagnostics.humans >= 1 &&
    Number.isInteger(diagnostics.npcs) && diagnostics.npcs >= 0;
}

export function rosterLabel(diagnostics) {
  return `auto-${diagnostics.humans}h-${diagnostics.npcs}n`;
}

function readDiagnostics() {
  const value = globalThis.window?.__turfraceDiagnostics;
  if (!value || typeof value !== 'object') return null;
  // Copy only the bridge fields. Apart from documenting the contract, this
  // keeps each periodic metadata record small and excludes page-owned data.
  const snapshot = {
    app_state: value.app_state,
    purpose: value.purpose,
    phase: value.phase,
    elapsed_seconds: value.elapsed_seconds,
    humans: value.humans,
    npcs: value.npcs,
  };
  if (typeof value.ready === 'boolean') snapshot.ready = value.ready;
  // Pass timing through to the probe's bounded numeric whitelist; do not
  // serialize or copy arbitrary page-owned nested data here.
  snapshot.frame_timing = value.frame_timing;
  return snapshot;
}

function makeOverlay() {
  if (!globalThis.document?.body) return null;
  const element = document.createElement('div');
  element.id = 'turfrace-performance-status';
  Object.assign(element.style, {
    position: 'fixed', top: '8px', right: '8px', zIndex: '2147483647',
    padding: '5px 8px', border: '1px solid currentColor', borderRadius: '4px',
    background: 'rgba(0, 0, 0, .78)', color: 'white', font: '12px sans-serif',
    pointerEvents: 'none', userSelect: 'none',
  });
  element.setAttribute('aria-live', 'polite');
  document.body.append(element);
  return element;
}

function setStatus(overlay, status, detail = '') {
  if (!overlay) return;
  overlay.textContent = `Performance ${status}${detail ? ` (${detail})` : ''}`;
  overlay.dataset.status = status;
}

export function startAutoCapture({
  pollMs = DIAGNOSTICS_POLL_MS,
  statusPollMs = STATUS_POLL_MS,
  probe = null,
} = {}) {
  if (!performanceEnabled()) return null;
  const overlay = makeOverlay();
  setStatus(overlay, 'waiting');
  let capture = null;
  let finished = false;
  let pollTimer;
  let statusTimer;
  let starting = false;
  const stopPolling = () => {
    clearInterval(pollTimer);
    clearInterval(statusTimer);
  };
  const checkCompletion = () => {
    if (capture && !globalThis.window?.__turfracePerformanceCapture && !finished) {
      finished = true;
      stopPolling();
      setStatus(overlay, 'completed');
    }
  };
  const tryStart = async () => {
    if (capture || starting || finished) return;
    const diagnostics = readDiagnostics();
    if (!isPlayableRunning(diagnostics)) return;
    starting = true;
    try {
      const label = rosterLabel(diagnostics);
      const startProbe = probe ?? (await import('./probe.mjs')).start;
      capture = startProbe({
        seconds: CAPTURE_SECONDS,
        windowSeconds: 5,
        label,
        // The probe attaches one current ECS snapshot to each five-second
        // window. It never sends input or otherwise mutates gameplay.
        diagnostics: readDiagnostics,
      });
      setStatus(overlay, 'recording', `${label}, 180s`);
      statusTimer = setInterval(checkCompletion, statusPollMs);
    } catch (error) {
      finished = true;
      stopPolling();
      setStatus(overlay, 'error', String(error?.message ?? error).slice(0, 160));
      console.error('Turfrace automatic performance capture failed:', error);
    } finally {
      starting = false;
    }
  };
  pollTimer = setInterval(tryStart, pollMs);
  tryStart();
  return { overlay, stop: stopPolling };
}

if (performanceEnabled()) {
  if (globalThis.document?.body) startAutoCapture();
  else globalThis.addEventListener?.('DOMContentLoaded', () => startAutoCapture(), { once: true });
}
