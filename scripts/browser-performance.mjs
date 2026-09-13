#!/usr/bin/env node
/**
 * Browser-only Turfrace performance smoke capture. This script never builds,
 * installs, or starts a server: pass the URL of an already-served dist.
 */
import { access, mkdir, writeFile } from 'node:fs/promises';
import { isAbsolute, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export const DEFAULT_URL = 'http://127.0.0.1:8080';
export const DEFAULT_CHROME = '/ms-playwright/chromium-1234/chrome-linux/chrome';
export const SIXTY_FPS_BUDGET_MS = 1000 / 60;
export const THIRTY_FPS_BUDGET_MS = 1000 / 30;
export const DIAGNOSTICS_KEY = 'turfrace.last_lobby.v1';

export function parseStoredNpcCount(raw) {
  if (raw === null) return 0;
  try {
    const value = JSON.parse(raw)?.npc_count;
    return Number.isInteger(value) && value >= 0 ? value : null;
  } catch {
    return null;
  }
}

function nearestRank(sorted, quantile) {
  return sorted[Math.max(0, Math.ceil(sorted.length * quantile) - 1)];
}

export function summarizeFrameIntervals(samples) {
  if (!Array.isArray(samples) || samples.some(value =>
    typeof value !== 'number' || !Number.isFinite(value) || value < 0)) {
    throw new Error('Invalid rAF samples');
  }
  if (!samples.length) return { frames: 0, meanMs: null, p95Ms: null, p99Ms: null, maxMs: null };
  const sorted = [...samples].sort((a, b) => a - b);
  const meanMs = samples.reduce((total, value) => total + value, 0) / samples.length;
  const over60 = samples.filter(value => value > SIXTY_FPS_BUDGET_MS).length;
  const over30 = samples.filter(value => value > THIRTY_FPS_BUDGET_MS).length;
  return {
    frames: samples.length,
    meanMs,
    p95Ms: nearestRank(sorted, 0.95),
    p99Ms: nearestRank(sorted, 0.99),
    maxMs: sorted.at(-1),
    effectiveFps: meanMs > 0 ? 1000 / meanMs : null,
    over16_67ms: over60,
    over33_33ms: over30,
    over100ms: samples.filter(value => value > 100).length,
    within60fpsRatio: (samples.length - over60) / samples.length,
    within30fpsRatio: (samples.length - over30) / samples.length,
  };
}

export function parseArgs(argv) {
  const options = {
    url: DEFAULT_URL,
    seconds: 60,
    bots: 7,
    width: 1280,
    height: 720,
    output: null,
    cpuProfile: null,
    captureOnly: false,
  };
  const valueOptions = new Set(['url', 'seconds', 'bots', 'width', 'height', 'output', 'cpu-profile']);
  for (let index = 0; index < argv.length; index++) {
    const argument = argv[index];
    if (argument === '--help' || argument === '-h') return { help: true };
    if (argument === '--capture-only') { options.captureOnly = true; continue; }
    if (!argument.startsWith('--')) throw new Error(`Unknown argument: ${argument}`);
    const name = argument.slice(2);
    if (!valueOptions.has(name)) throw new Error(`Unknown option: --${name}`);
    const value = argv[++index];
    if (value === undefined || value.startsWith('--')) throw new Error(`Missing value for --${name}`);
    options[name === 'cpu-profile' ? 'cpuProfile' : name] = value;
  }
  options.seconds = Number(options.seconds);
  options.bots = Number(options.bots);
  options.width = Number(options.width);
  options.height = Number(options.height);
  if (!Number.isFinite(options.seconds) || options.seconds < 1 || options.seconds > 300) {
    throw new Error('--seconds must be a number from 1 to 300');
  }
  if (![7, 11].includes(options.bots)) throw new Error('--bots must be 7 or 11');
  if (!Number.isInteger(options.width) || options.width < 840 ||
      !Number.isInteger(options.height) || options.height < 600) {
    throw new Error('--width must be >=840 and --height must be >=600 for deterministic lobby controls');
  }
  if (!options.url || !/^https?:\/\//.test(options.url)) throw new Error('--url must be an http(s) URL');
  options.output = resolve(options.output ?? `target/browser-performance/${new Date().toISOString().replaceAll(':', '-')}`);
  if (options.cpuProfile) options.cpuProfile = resolve(options.cpuProfile);
  return options;
}

export function helpText() {
  return `Usage: node scripts/browser-performance.mjs [options]

Uses the existing served dist and installed Playwright/SwiftShader Chrome.
  --url URL             Existing server (default ${DEFAULT_URL})
  --bots 7|11           NPC count for one human (default 7)
  --seconds N           Clean rAF capture duration, 1..300 (default 60)
  --width N --height N  Browser viewport (default 1280x720)
  --output DIR          Evidence directory (default target/browser-performance/<timestamp>)
  --cpu-profile FILE    Optional separate Chromium CPU profile (not timing evidence)
  --capture-only        Capture the supplied current page; it must expose a running playable match
`;
}

/** Add the explicit opt-in flag without dropping existing query parameters. */
export function performanceUrl(rawUrl) {
  const url = new URL(rawUrl);
  url.searchParams.set('performance', '1');
  return url.href;
}

/** Validate the small, public shape supplied by the WASM ECS bridge. */
export function validateDiagnostics(value, expected = {}) {
  if (!value || typeof value !== 'object') throw new Error('Missing __turfraceDiagnostics (use a current WASM build)');
  for (const key of ['app_state', 'purpose', 'phase']) {
    if (typeof value[key] !== 'string' || value[key].length === 0) {
      throw new Error(`Invalid __turfraceDiagnostics.${key}`);
    }
  }
  if (typeof value.elapsed_seconds !== 'number' || !Number.isFinite(value.elapsed_seconds) || value.elapsed_seconds < 0) {
    throw new Error('Invalid __turfraceDiagnostics.elapsed_seconds');
  }
  for (const key of ['humans', 'npcs']) {
    if (!Number.isInteger(value[key]) || value[key] < 0) throw new Error(`Invalid __turfraceDiagnostics.${key}`);
  }
  if (value.ready !== undefined && value.ready !== null && typeof value.ready !== 'boolean') {
    throw new Error('Invalid __turfraceDiagnostics.ready');
  }
  for (const key of ['purpose', 'phase', 'humans', 'npcs']) {
    if (expected[key] !== undefined && value[key] !== expected[key]) {
      throw new Error(`ECS diagnostics mismatch: ${key}=${value[key]}, expected ${expected[key]}`);
    }
  }
  return value;
}

/** Assess simulation time independently from rAF delivery time. */
export function assessSimulationProgress(observations) {
  if (!Array.isArray(observations) || observations.some(observation =>
    !observation || !Number.isFinite(observation.observedAt) ||
    !Number.isFinite(observation.elapsed_seconds))) {
    throw new Error('Invalid ECS diagnostic observations');
  }
  if (observations.length < 2) {
    return { wallMs: null, simulationMs: null, rate: null, slowdown: true, reason: 'fewer than two ECS samples' };
  }
  const first = observations[0];
  const last = observations.at(-1);
  const wallMs = last.observedAt - first.observedAt;
  const simulationMs = (last.elapsed_seconds - first.elapsed_seconds) * 1000;
  const rate = wallMs > 0 ? simulationMs / wallMs : null;
  return {
    wallMs,
    simulationMs,
    rate,
    // Fixed simulation time should track wall time while Running. Keep this
    // diagnostic separate from frame pacing: a slow simulation is not a fake FPS pass.
    slowdown: rate === null || rate < 0.8,
    reason: rate === null ? 'non-positive observation interval' : null,
  };
}

export function withTimeout(operation, timeoutMs, label = 'operation') {
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) return Promise.reject(new Error(`Invalid timeout for ${label}`));
  let timer;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${label} timed out after ${timeoutMs}ms`)), timeoutMs);
  });
  return Promise.race([Promise.resolve(operation), timeout]).finally(() => clearTimeout(timer));
}

function addDiagnostics(page) {
  const diagnostics = { console: [], pageErrors: [], requestFailures: [] };
  page.on('console', message => {
    diagnostics.console.push({ type: message.type(), text: message.text().slice(0, 4000), location: message.location() });
  });
  page.on('pageerror', error => diagnostics.pageErrors.push({ message: String(error).slice(0, 4000), stack: error.stack }));
  page.on('requestfailed', request => diagnostics.requestFailures.push({
    url: request.url(), method: request.method(), failure: request.failure()?.errorText ?? 'unknown',
  }));
  return diagnostics;
}

async function rendererIdentity(page) {
  return withTimeout(page.evaluate(() => {
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 1;
    const gl = canvas.getContext('webgl2') ?? canvas.getContext('webgl');
    if (!gl) return { available: false, vendor: null, renderer: null, unmaskedVendor: null, unmaskedRenderer: null };
    const debug = gl.getExtension('WEBGL_debug_renderer_info');
    return {
      available: true,
      vendor: gl.getParameter(gl.VENDOR),
      renderer: gl.getParameter(gl.RENDERER),
      unmaskedVendor: debug ? gl.getParameter(debug.UNMASKED_VENDOR_WEBGL) : null,
      unmaskedRenderer: debug ? gl.getParameter(debug.UNMASKED_RENDERER_WEBGL) : null,
      context: gl instanceof WebGL2RenderingContext ? 'webgl2' : 'webgl',
    };
  }), 5000, 'renderer identity');
}

async function waitForCanvas(page) {
  await withTimeout(page.waitForSelector('canvas', { state: 'attached', timeout: 15000 }), 20000, 'canvas');
  await withTimeout(page.waitForFunction(() => document.querySelector('#loading')?.classList.contains('hidden'), null, { timeout: 15000 }), 20000, 'loading screen');
}

async function readDiagnostics(page) {
  const value = await withTimeout(page.evaluate(() => {
    const diagnostics = window.__turfraceDiagnostics;
    return diagnostics ? { ...diagnostics, observedAt: performance.now() } : null;
  }), 4000, 'ECS diagnostics read');
  return value && validateDiagnostics(value);
}

async function waitForDiagnostics(page, predicate, timeoutMs, label) {
  const deadline = Date.now() + timeoutMs;
  let last = null;
  while (Date.now() < deadline) {
    last = await readDiagnostics(page);
    if (last && predicate(last)) return last;
    await new Promise(resolve => setTimeout(resolve, Math.min(100, Math.max(1, deadline - Date.now()))));
  }
  throw new Error(`BLOCKER: ${label} (last diagnostics: ${JSON.stringify(last)})`);
}

function lobbyControlPoint(width, height) {
  // Derived from spawn_lobby_content: the plus selector is 80px left of center.
  return { x: width / 2 - 80, y: height / 2 + 210 };
}

async function storedNpcCount(page) {
  const raw = await withTimeout(page.evaluate(key => localStorage.getItem(key), DIAGNOSTICS_KEY), 4000, 'lobby storage read');
  return parseStoredNpcCount(raw);
}

async function waitForStoredNpcCount(page, expected, timeoutMs = 3000) {
  const deadline = Date.now() + timeoutMs;
  let observed = null;
  while (Date.now() < deadline) {
    observed = await storedNpcCount(page);
    if (observed === expected) return observed;
    await new Promise(resolve => setTimeout(resolve, 80));
  }
  throw new Error(`BLOCKER: localStorage ${DIAGNOSTICS_KEY}.npc_count=${observed}; expected ${expected}`);
}

async function click(page, point) {
  return withTimeout(page.mouse.click(point.x, point.y), 5000, 'browser click');
}

async function navigationObservation(page) {
  const value = await withTimeout(page.evaluate(key => {
    const diagnostics = window.__turfraceDiagnostics;
    return { diagnostics: diagnostics ? { ...diagnostics, observedAt: performance.now() } : null,
      storage: localStorage.getItem(key) };
  }, DIAGNOSTICS_KEY), 4000, 'navigation state read');
  return {
    diagnostics: value.diagnostics && validateDiagnostics(value.diagnostics),
    npcCount: parseStoredNpcCount(value.storage),
  };
}

async function navigateToMatch(page, options, evidenceDir) {
  // Configure the canvas-only lobby at its verified layout, then resize the
  // actual running match to the requested measurement viewport. Pixel controls
  // do not scale proportionally when Bevy's responsive layout changes.
  const width = 1280, height = 720;
  await withTimeout(page.setViewportSize({ width, height }), 5000, 'lobby viewport');
  await new Promise(resolve => setTimeout(resolve, 500));
  const trace = [];
  const record = async action => {
    const observation = await navigationObservation(page);
    trace.push({ action, ...observation });
    return observation;
  };
  try {
    await page.screenshot({ path: resolve(evidenceDir, 'home.png') });
    await waitForDiagnostics(page, () => true, 5000, 'WASM ECS diagnostics are unavailable');
    await waitForDiagnostics(page, diagnostics => diagnostics.app_state === 'Home', 8000, 'home state did not appear');
    await click(page, { x: width / 2, y: height * 0.53 });
    await record('click PLAY');
    await waitForDiagnostics(page, diagnostics => diagnostics.app_state === 'Lobby', 8000, 'PLAY did not transition to the lobby');
    await record('lobby rendered');

    // Enter is the KeyboardPrimary join/ready input in the lobby. Let the
    // state-driven screen rebuild before clicking its newly spawned controls.
    await withTimeout(page.keyboard.press('Enter'), 5000, 'keyboard join');
    await waitForStoredNpcCount(page, 0);
    await new Promise(resolve => setTimeout(resolve, 300));
    await record('keyboard join');

    // The NPC bar moves when the keyboard card is spawned. Its known control
    // point is still derived from the canvas viewport, but clicks are retried
    // until the authoritative persistence transition confirms the input was
    // delivered. This avoids racing Bevy's state/UI rebuild without weakening
    // any roster or persistence check.
    const plus = lobbyControlPoint(width, height);
    const transitions = [];
    for (let count = 1; count <= options.bots; count++) {
      let observed = null;
      for (let attempt = 1; attempt <= 8; attempt++) {
        await click(page, plus);
        try {
          // Persistence is written from Bevy's update, not the browser click
          // callback. Wait for this click's exact transition before retrying;
          // otherwise a delayed click can make the count jump over the target.
          await waitForStoredNpcCount(page, count, 1000);
        } catch (error) {
          observed = await record(`click NPC + (${count}) attempt ${attempt} pending`);
          if (observed.npcCount !== null && observed.npcCount > count) throw error;
          await new Promise(resolve => setTimeout(resolve, 100));
          continue;
        }
        observed = await record(`click NPC + (${count}) attempt ${attempt}`);
        break;
      }
      if (observed?.npcCount !== count) {
        throw new Error(`BLOCKER: localStorage ${DIAGNOSTICS_KEY}.npc_count=${observed?.npcCount}; expected ${count}`);
      }
      transitions.push(count);
    }
    if (transitions.length !== options.bots) {
      throw new Error(`BLOCKER: expected ${options.bots} localStorage NPC transitions, observed ${transitions.length}`);
    }
    await page.screenshot({ path: resolve(evidenceDir, 'lobby-configured.png') });

    await withTimeout(page.keyboard.press('Enter'), 5000, 'keyboard ready');
    await record('keyboard ready');
    const expected = { purpose: 'Playable', phase: 'Running', humans: 1, npcs: options.bots };
    const running = await waitForDiagnostics(page, diagnostics =>
      diagnostics.app_state === 'Playing' && diagnostics.purpose === expected.purpose &&
      diagnostics.phase === expected.phase && diagnostics.humans === expected.humans && diagnostics.npcs === expected.npcs,
    15000, 'playable match did not reach Running with the requested ECS roster');
    await record('ECS Playing/Playable/Running');
    await withTimeout(page.setViewportSize({ width: options.width, height: options.height }), 5000, 'measurement viewport');
    await new Promise(resolve => setTimeout(resolve, 500));
    await record('measurement viewport restored');
    await page.screenshot({ path: resolve(evidenceDir, 'before.png') });
    await writeFile(resolve(evidenceDir, 'navigation-trace.json'), JSON.stringify(trace, null, 2));
    return {
      gameplayStarted: true,
      roster: { humans: running.humans, npcs: running.npcs, verified: true,
        evidence: `WASM ECS diagnostics; localStorage npc_count transitions 0..${transitions.length}` },
      initialDiagnostics: running,
      navigationTrace: trace,
    };
  } catch (error) {
    // Preserve the last frame and both authoritative signals for blockers.
    await writeFile(resolve(evidenceDir, 'navigation-trace.json'), JSON.stringify(trace, null, 2));
    try { await page.screenshot({ path: resolve(evidenceDir, 'navigation-failure.png') }); } catch { /* preserve original blocker */ }
    throw error;
  }
}

/** Capture rAF with a page-side deadline and a second, Node-side deadline. */
export async function sampleRaf(page, seconds, { timeoutMs = seconds * 1000 + 4000 } = {}) {
  if (!Number.isFinite(seconds) || seconds <= 0) throw new Error('Invalid rAF duration');
  const result = await withTimeout(page.evaluate(({ durationMs, graceMs }) => new Promise(resolve => {
    const started = performance.now();
    const deadline = started + durationMs;
    let previous = null;
    let first = null;
    let frame = null;
    let finished = false;
    const initialHidden = document.hidden;
    let lastHidden = initialHidden;
    let hidden = initialHidden;
    let visibilityChanges = 0;
    const samples = [];
    const finish = (timedOut, now = performance.now()) => {
      if (finished) return;
      finished = true;
      clearTimeout(timer);
      if (frame !== null) cancelAnimationFrame(frame);
      document.removeEventListener('visibilitychange', visibilityChanged);
      resolve({
        elapsedMs: now - started,
        initialUnsampledGapMs: first === null ? null : first - started,
        finalUnsampledGapMs: now - (previous ?? started),
        hidden,
        initialHidden,
        finalHidden: document.hidden,
        visibilityChanges,
        timedOut,
        samples,
      });
    };
    const visibilityChanged = () => {
      visibilityChanges++;
      hidden ||= document.hidden;
      lastHidden = document.hidden;
    };
    document.addEventListener('visibilitychange', visibilityChanged);
    const tick = now => {
      if (finished) return;
      if (document.hidden !== lastHidden) visibilityChanged();
      if (first === null) first = now;
      else if (now <= deadline) samples.push(now - previous);
      previous = now;
      if (now >= deadline) finish(false, now);
      else frame = requestAnimationFrame(tick);
    };
    const timer = setTimeout(() => finish(true), durationMs + graceMs);
    frame = requestAnimationFrame(tick);
  }), { durationMs: seconds * 1000, graceMs: Math.max(1000, timeoutMs - seconds * 1000) }), timeoutMs + 250, 'rAF sampling');
  return result;
}

/** Drive one key at a time; no async interval callbacks can race key-up. */
export async function steerDuring(page, seconds) {
  const keys = ['ArrowRight', 'ArrowDown', 'ArrowLeft', 'ArrowUp'];
  const deadline = Date.now() + seconds * 1000;
  let index = 0;
  let held = false;
  try {
    await withTimeout(page.keyboard.down(keys[index]), 5000, 'steering key-down');
    held = true;
    while (Date.now() < deadline) {
      await new Promise(resolve => setTimeout(resolve, Math.min(2000, deadline - Date.now())));
      await withTimeout(page.keyboard.up(keys[index]), 5000, 'steering key-up');
      held = false;
      if (Date.now() >= deadline) break;
      index = (index + 1) % keys.length;
      await withTimeout(page.keyboard.down(keys[index]), 5000, 'steering key-down');
      held = true;
    }
  } finally {
    if (held) await withTimeout(page.keyboard.up(keys[index]), 5000, 'final steering key-up');
  }
}

async function pollRunningDiagnostics(page, seconds, expected) {
  const observations = [];
  const deadline = Date.now() + seconds * 1000;
  const check = diagnostics => {
    validateDiagnostics(diagnostics, expected);
    if (diagnostics.app_state !== 'Playing' || diagnostics.purpose !== 'Playable' || diagnostics.phase !== 'Running') {
      throw new Error(`BLOCKER: ECS match stopped Running (${JSON.stringify(diagnostics)})`);
    }
    observations.push(diagnostics);
  };
  while (Date.now() < deadline) {
    check(await readDiagnostics(page));
    await new Promise(resolve => setTimeout(resolve, Math.min(1000, Math.max(1, deadline - Date.now()))));
  }
  check(await readDiagnostics(page));
  return observations;
}

async function launchBrowser(playwright) {
  return playwright.chromium.launch({
    headless: true,
    executablePath: DEFAULT_CHROME,
    args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'],
  });
}

async function closeBrowser(browser) {
  try { await withTimeout(browser.close(), 5000, 'browser close'); } catch { /* failure path must not hang */ }
}

export async function runClean(playwright, options, evidenceDir) {
  const browser = await launchBrowser(playwright);
  let context;
  let page;
  try {
    context = await withTimeout(browser.newContext({ viewport: { width: options.width, height: options.height } }), 10000, 'browser context');
    page = await withTimeout(context.newPage(), 10000, 'browser page');
    const diagnostics = addDiagnostics(page);
    const run = async () => {
      const url = performanceUrl(options.url);
      await withTimeout(page.goto(url, { waitUntil: 'domcontentloaded', timeout: 20000 }), 25000, 'page navigation');
      await waitForCanvas(page);
      const renderer = await rendererIdentity(page);
      let scenario;
      let expected;
      if (options.captureOnly) {
        const current = await waitForDiagnostics(page, diagnostics =>
          diagnostics.app_state === 'Playing' && diagnostics.purpose === 'Playable' && diagnostics.phase === 'Running',
        8000, 'capture-only page is not a running playable match');
        expected = { purpose: 'Playable', phase: 'Running', humans: current.humans, npcs: current.npcs };
        scenario = { gameplayStarted: true, mode: 'capture-only', roster: { ...expected, verified: true,
          evidence: 'WASM ECS diagnostics from supplied running page' } };
        await page.screenshot({ path: resolve(evidenceDir, 'before.png') });
      } else {
        const navigated = await navigateToMatch(page, options, evidenceDir);
        scenario = navigated;
        expected = { purpose: 'Playable', phase: 'Running', humans: 1, npcs: options.bots };
      }
      await page.bringToFront();
      const timingPromise = sampleRaf(page, options.seconds);
      const diagnosticPromise = pollRunningDiagnostics(page, options.seconds, expected);
      const steeringPromise = steerDuring(page, options.seconds);
      const [timing, ecsDiagnostics] = await Promise.all([timingPromise, diagnosticPromise]);
      await steeringPromise;
      if (timing.timedOut) throw new Error('BLOCKER: rAF sampling timed out; page may be hung');
      if (timing.samples.length === 0) throw new Error('BLOCKER: rAF sampling produced zero frame intervals');
      if (timing.hidden || timing.visibilityChanges || timing.initialHidden || timing.finalHidden) {
        throw new Error('BLOCKER: page visibility changed or was hidden during the clean capture');
      }
      if (timing.finalUnsampledGapMs > 250) {
        throw new Error(`BLOCKER: clean capture has an unsampled final gap of ${timing.finalUnsampledGapMs.toFixed(1)}ms`);
      }
      const simulation = assessSimulationProgress(ecsDiagnostics);
      await page.screenshot({ path: resolve(evidenceDir, 'after.png') });
      return {
        renderer,
        scenario,
        timing: { ...summarizeFrameIntervals(timing.samples), elapsedMs: timing.elapsedMs,
          timedOut: timing.timedOut, hidden: timing.hidden, visibilityChanges: timing.visibilityChanges,
          initialUnsampledGapMs: timing.initialUnsampledGapMs,
          finalUnsampledGapMs: timing.finalUnsampledGapMs,
          diagnosticsSamples: ecsDiagnostics.length, simulation },
        ecsDiagnostics,
        diagnostics,
      };
    };
    // A page-side timer cannot fire if the renderer is wedged. This outer
    // bound ensures the harness reaches finally and closes Chromium anyway.
    return await withTimeout(run(), options.seconds * 1000 + 45000, 'clean browser run');
  } finally {
    await closeBrowser(browser);
  }
}

export async function runCpuProfile(playwright, options, profilePath) {
  const browser = await launchBrowser(playwright);
  let context;
  try {
    context = await withTimeout(browser.newContext({ viewport: { width: options.width, height: options.height } }), 10000, 'profile browser context');
    const page = await withTimeout(context.newPage(), 10000, 'profile browser page');
    const diagnostics = addDiagnostics(page);
    const client = await withTimeout(context.newCDPSession(page), 10000, 'profile CDP session');
    await withTimeout(page.goto(performanceUrl(options.url), { waitUntil: 'domcontentloaded', timeout: 20000 }), 25000, 'profile navigation');
    await waitForCanvas(page);
    const renderer = await rendererIdentity(page);
    if (!options.captureOnly) {
      const profileEvidenceDir = `${profilePath}.evidence`;
      await mkdir(profileEvidenceDir, { recursive: true });
      await navigateToMatch(page, options, profileEvidenceDir);
    } else {
      await waitForDiagnostics(page, diagnostics =>
        diagnostics.app_state === 'Playing' && diagnostics.purpose === 'Playable' && diagnostics.phase === 'Running',
      8000, 'capture-only profile page is not a running playable match');
    }
    await withTimeout(client.send('Profiler.enable'), 10000, 'profile enable');
    await withTimeout(client.send('Profiler.start'), 10000, 'profile start');
    if (options.captureOnly) await new Promise(resolve => setTimeout(resolve, options.seconds * 1000));
    else await steerDuring(page, options.seconds);
    const profile = await withTimeout(client.send('Profiler.stop'), 10000, 'profile stop');
    await writeFile(profilePath, JSON.stringify({ format: 'Chromium CPU profile', separateFromCleanTiming: true, renderer, profile: profile.profile }, null, 2));
    await writeFile(`${profilePath}.diagnostics.json`, JSON.stringify(diagnostics, null, 2));
  } finally {
    await closeBrowser(browser);
  }
}

async function loadPlaywright() {
  const candidates = [process.env.PLAYWRIGHT_MODULE, '/usr/local/bun/install/global/node_modules/playwright/index.mjs'].filter(Boolean);
  let lastError;
  for (const candidate of candidates) {
    try { return await import(candidate); } catch (error) { lastError = error; }
  }
  throw new Error(`BLOCKER: installed global Playwright could not be loaded (${lastError?.message ?? 'unknown error'})`);
}

async function main() {
  let options;
  try { options = parseArgs(process.argv.slice(2)); } catch (error) { console.error(`ERROR: ${error.message}\n\n${helpText()}`); process.exitCode = 2; return; }
  if (options.help) { console.log(helpText()); return; }
  await mkdir(options.output, { recursive: true });
  if (!isAbsolute(DEFAULT_CHROME)) throw new Error(`BLOCKER: Chromium executable path is invalid: ${DEFAULT_CHROME}`);
  try { await access(DEFAULT_CHROME); } catch { throw new Error(`BLOCKER: Chromium executable not found: ${DEFAULT_CHROME}`); }
  const playwright = await loadPlaywright();
  let clean;
  try {
    clean = await runClean(playwright, options, options.output);
    const metadata = {
      tool: 'scripts/browser-performance.mjs',
      url: options.url,
      diagnosticsUrl: performanceUrl(options.url),
      diagnosticsRequired: true,
      viewport: [options.width, options.height],
      seconds: options.seconds,
      renderer: clean.renderer,
      renderingClassification: 'software-rendered (SwiftShader/no GPU inventory); NOT hardware FPS sign-off',
      hardwareFpsSignoff: false,
      cleanTimingRun: true,
      cpuProfile: options.cpuProfile,
      scenario: clean.scenario,
      diagnosticsVerified: true,
      diagnosticsCounts: Object.fromEntries(Object.entries(clean.diagnostics).map(([key, value]) => [key, value.length])),
      budgetMs: { fps60: SIXTY_FPS_BUDGET_MS, fps30: THIRTY_FPS_BUDGET_MS },
    };
    // Persist all clean evidence before touching the optional profiler. A
    // profiler/browser failure must not erase the valid timing capture.
    await writeFile(resolve(options.output, 'timing.json'), JSON.stringify(clean.timing, null, 2));
    await writeFile(resolve(options.output, 'metadata.json'), JSON.stringify(metadata, null, 2));
    await writeFile(resolve(options.output, 'diagnostics.json'), JSON.stringify(clean.diagnostics, null, 2));
    await writeFile(resolve(options.output, 'ecs-diagnostics.json'), JSON.stringify(clean.ecsDiagnostics, null, 2));
    let profileError = null;
    if (options.cpuProfile) {
      try {
        await mkdir(resolve(options.cpuProfile, '..'), { recursive: true });
        await runCpuProfile(playwright, options, options.cpuProfile);
      } catch (error) {
        profileError = String(error.message ?? error);
        await writeFile(resolve(options.output, 'cpu-profile-error.json'), JSON.stringify({ message: profileError }, null, 2));
      }
    }
    if (profileError) metadata.cpuProfileError = profileError;
    await writeFile(resolve(options.output, 'metadata.json'), JSON.stringify(metadata, null, 2));
    console.log(JSON.stringify({ output: options.output, timing: clean.timing, scenario: clean.scenario,
      renderingClassification: metadata.renderingClassification, cpuProfileError: profileError }, null, 2));
  } catch (error) {
    await writeFile(resolve(options.output, 'blocker.json'), JSON.stringify({ message: String(error.message ?? error),
      captureOnlyCommand: `node scripts/browser-performance.mjs --capture-only --url ${options.url}` }, null, 2));
    console.error(`ERROR: ${error.message ?? error}`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) await main();
