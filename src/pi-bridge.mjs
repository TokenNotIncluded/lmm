// Calls the selected Pi 1.2.0 installation's native package and settings APIs.
// This bridge owns no OAuth credentials and never invokes a model.
import { readFileSync, lstatSync, realpathSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join, resolve, isAbsolute } from 'node:path';
import { pathToFileURL } from 'node:url';
let input = '';
for await (const chunk of process.stdin) input += chunk;
const request = JSON.parse(input);
try {
  const manifest = JSON.parse(readFileSync(join(request.sdk, 'package.json'), 'utf8'));
  if (manifest.name !== '@earendil-works/pi-coding-agent' || manifest.version !== '1.2.0') throw Error();
  for (const path of [request.cwd, request.agentDir, join(request.cwd, '.pi'), join(request.cwd, '.pi', 'settings.json')]) {
    try { if (lstatSync(path).isSymbolicLink()) throw Error('unsafe_path'); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
  const { FileSettingsStorage, SettingsManager } = await import(pathToFileURL(join(request.sdk, 'dist/core/settings-manager.js')));
  const { DefaultPackageManager } = await import(pathToFileURL(join(request.sdk, 'dist/core/package-manager.js')));
  const storage = new FileSettingsStorage(request.cwd, request.agentDir);
  const official = (source, base) => {
    if (typeof source !== 'string') return false;
    if (/^npm:@tokennotincluded\/pi-lmm-provider(?:@|$)/.test(source) ||
        /^git:github.com\/TokenNotIncluded\/pi-lmm-provider(?:@|$)/i.test(source)) return true;
    if (/^(?:npm:|git:|https:|ssh:|github:)/i.test(source)) return false;
    try { return JSON.parse(readFileSync(join(isAbsolute(source) ? source : resolve(base, source), 'package.json'),'utf8')).name === '@tokennotincluded/pi-lmm-provider'; }
    catch { return false; }
  };
  let global;
  storage.withLock('global', (raw) => { global = raw ? JSON.parse(raw) : {}; });
  if (!Array.isArray(global.packages ?? [])) throw Error('invalid_settings');
  const globalConflict = (global.packages ?? []).some((item) => official(typeof item === 'string' ? item : item?.source, request.agentDir));
  let current;
  storage.withLock('project', (raw) => { current = raw ? JSON.parse(raw) : {}; });
  const inspect = (settings) => {
    const packages = settings.packages ?? [];
    if (!Array.isArray(packages)) throw Error('invalid_settings');
    const matching = packages.filter((item) => item === request.source);
    const others = packages.filter((item) => official(typeof item === 'string' ? item : item?.source, join(request.cwd,'.pi')) && item !== request.source);
    return { packages, matching, others };
  };
  const before = inspect(current);
  const manager = new DefaultPackageManager({cwd: request.cwd, agentDir: request.agentDir,
    settingsManager: SettingsManager.create(request.cwd, request.agentDir)});
  const hasCode = () => {
    const path=manager.getInstalledPath(request.source,'project');
    if (!path) return false;
    try {
      if (JSON.parse(readFileSync(join(path,'package.json'),'utf8')).name !== '@tokennotincluded/pi-lmm-provider') return false;
      const pin = /^git:github\.com\/TokenNotIncluded\/pi-lmm-provider@([a-f0-9]{40})$/.exec(request.source);
      if (request.source.startsWith('git:')) {
        if (!pin) return false;
        const git = (args) => execFileSync('git',['-C',path,...args],{encoding:'utf8',stdio:['ignore','pipe','ignore'],timeout:5000,maxBuffer:1024*1024}).trim();
        if (realpathSync(git(['rev-parse','--show-toplevel'])) !== realpathSync(path)
            || git(['rev-parse','HEAD']) !== pin[1]
            || git(['status','--porcelain','--untracked-files=all']) !== '') return false;
      }
      return true;
    } catch { return false; }
  };
  if (request.action === 'inspect') {
    console.log(JSON.stringify({ configured: before.matching.length === 1, installed: before.matching.length === 1 && hasCode(), conflicting: globalConflict || before.others.length > 0 }));
  } else if (request.action === 'add') {
    if (globalConflict || before.others.length || before.matching.length > 1) throw Error('source_conflict');
    if (!hasCode()) {
      if (manager.getInstalledPath(request.source,'project')) throw Error('unverified_cached_code');
      // Installation deliberately does not persist the stale settings snapshot.
      await manager.install(request.source, {local: true});
    }
    if (!hasCode()) throw Error('unverified_installed_code');
    let added = false;
    storage.withLock('global', (globalRaw) => {
      const liveGlobal = globalRaw ? JSON.parse(globalRaw) : {};
      if (!Array.isArray(liveGlobal.packages ?? []) || (liveGlobal.packages ?? []).some((item) => official(typeof item === 'string' ? item : item?.source, request.agentDir))) throw Error('source_conflict');
    storage.withLock('project', (raw) => {
      const latest = raw ? JSON.parse(raw) : {};
      const state = inspect(latest);
      if (!hasCode()) throw Error('unverified_installed_code');
      if (state.others.length || state.matching.length > 1) throw Error('source_conflict');
      if (state.matching.length) return undefined;
      latest.packages = [...state.packages, request.source];
      added = true;
      return JSON.stringify(latest, null, 2);
    });
    });
    if (!hasCode()) throw Error('missing_package');
    console.log(JSON.stringify({configured:true,installed: true, added}));
  } else if (request.action === 'remove') {
    let removed = false;
    storage.withLock('project', (raw) => {
      const latest = raw ? JSON.parse(raw) : {};
      const state = inspect(latest);
      if (state.matching.length > 1 || state.packages.some((item) => typeof item === 'object' && item?.source === request.source)) throw Error('source_changed');
      if (!state.matching.length) return undefined;
      latest.packages = state.packages.filter((item) => item !== request.source);
      removed = true;
      return JSON.stringify(latest, null, 2);
    });
    // Downloaded code is retained: it may be referenced outside this project.
    console.log(JSON.stringify({configured:false,installed: false, removed}));
  } else throw Error('invalid_action');
} catch {
  // Neither Pi exceptions nor configuration contents may reach CLI diagnostics.
  console.error('Pi native adapter failed; configuration may need inspection.');
  process.exitCode = 3;
}
