// Explicit integration check against a real, separately installed Pi 1.2.0 SDK.
import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,mkdirSync,mkdtempSync,symlinkSync,unlinkSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {spawnSync,spawn} from 'node:child_process';
const sdk=resolve(process.argv[2]);
const output=resolve(process.argv[3]);
const bridge=readFileSync(new URL('../src/pi-bridge.mjs',import.meta.url),'utf8');
const root=mkdtempSync(join(output,'native-fixture-'));
const cwd=join(root,'project'), agentDir=join(root,'agent'), source=join(root,'provider');
for (const path of [cwd,agentDir,source,join(cwd,'.pi')]) mkdirSync(path,{recursive:true});
writeFileSync(join(source,'package.json'),JSON.stringify({name:'@tokennotincluded/pi-lmm-provider',version:'0.1.0-test',pi:{extensions:[]}}));
const settings=join(cwd,'.pi/settings.json');
writeFileSync(settings,JSON.stringify({packages:['npm:other-package@1.0.0'],defaultProvider:'other',theme:'dark'}));
function invoke(action) {
 const response=spawnSync(process.execPath,['--input-type=module','-e',bridge],{input:JSON.stringify({sdk,cwd,agentDir,source,action}),encoding:'utf8'});
 assert.equal(response.status,0,response.stderr);
 return JSON.parse(response.stdout);
}
assert.deepEqual(invoke('inspect'),{configured:false,installed:false,conflicting:false});
assert.equal(invoke('add').added,true);
let current=JSON.parse(readFileSync(settings));
assert.deepEqual(current.packages,['npm:other-package@1.0.0',source]);
assert.equal(current.defaultProvider,'other');
assert.equal(invoke('add').added,false);
// Changes made after installation must survive restoration.
current.packages.push({source:'npm:later-plugin@1.0.0',extensions:[]});
current.defaultModel='later-choice';current.theme='light';
writeFileSync(settings,JSON.stringify(current));
assert.equal(invoke('remove').removed,true);
current=JSON.parse(readFileSync(settings));
assert.deepEqual(current.packages,['npm:other-package@1.0.0',{source:'npm:later-plugin@1.0.0',extensions:[]}]);
assert.equal(current.defaultModel,'later-choice');assert.equal(current.theme,'light');
assert.equal(invoke('remove').removed,false);
// Concurrent native writers must all survive, without replacing an old packages array.
const nativeWriter = `import {FileSettingsStorage} from ${JSON.stringify(new URL('file://' + join(sdk,'dist/core/settings-manager.js')).href)};
const [cwd,agentDir,id]=process.argv.slice(1);
new FileSettingsStorage(cwd,agentDir).withLock('project',raw=>{
 const latest=JSON.parse(raw);latest.packages.push('npm:concurrent-'+id+'@1.0.0');
 return JSON.stringify(latest);
});`;
const adapterRace = new Promise((done,fail)=>{
 const child=spawn(process.execPath,['--input-type=module','-e',bridge],{stdio:['pipe','pipe','pipe']});
 child.stdin.end(JSON.stringify({sdk,cwd,agentDir,source,action:'add'}));
 child.on('error',fail);child.on('exit',code=>code===0?done():fail(Error('adapter concurrent add failed')));
});
await Promise.all([adapterRace,...Array.from({length:4},(_,id)=>new Promise((done,fail)=>{
 const child=spawn(process.execPath,['--input-type=module','-e',nativeWriter,cwd,agentDir,String(id)],{stdio:'pipe'});
 child.on('error',fail);child.on('exit',code=>code===0?done():fail(Error('native concurrent writer failed')));
}))]);
current=JSON.parse(readFileSync(settings));
for(let id=0;id<4;id++) assert.ok(current.packages.includes('npm:concurrent-'+id+'@1.0.0'));
assert.ok(current.packages.includes(source));
assert.equal(invoke('remove').removed,true);
current=JSON.parse(readFileSync(settings));
// A configured global LMM provider must block creating a second project provider.
writeFileSync(join(agentDir,'settings.json'),JSON.stringify({packages:['npm:@tokennotincluded/pi-lmm-provider@alpha']}));
assert.equal(invoke('inspect').conflicting,true);
const conflict=spawnSync(process.execPath,['--input-type=module','-e',bridge],{input:JSON.stringify({sdk,cwd,agentDir,source,action:'add'}),encoding:'utf8'});
assert.equal(conflict.status,3);assert.deepEqual(JSON.parse(readFileSync(settings)),current);
writeFileSync(join(agentDir,'settings.json'),'{}');
// A user changed our entry into an object: refuse to remove their new settings.
current.packages.push({source,extensions:['user-choice']});writeFileSync(settings,JSON.stringify(current));
const failed=spawnSync(process.execPath,['--input-type=module','-e',bridge],{input:JSON.stringify({sdk,cwd,agentDir,source,action:'remove'}),encoding:'utf8'});
assert.equal(failed.status,3);assert.deepEqual(JSON.parse(readFileSync(settings)),current);
// Malformed configuration and symlink targets fail without disclosing their contents.
const privateText='{ "apiKey": "DO_NOT_PRINT_PRIVATE_CONFIG", broken';
writeFileSync(settings,privateText);
const invalid=spawnSync(process.execPath,['--input-type=module','-e',bridge],{input:JSON.stringify({sdk,cwd,agentDir,source,action:'inspect'}),encoding:'utf8'});
assert.equal(invalid.status,3);assert.ok(!(invalid.stdout+invalid.stderr).includes('DO_NOT_PRINT_PRIVATE_CONFIG'));
assert.equal(readFileSync(settings,'utf8'),privateText);
unlinkSync(settings);const unrelated=join(root,'unrelated.json');writeFileSync(unrelated,'PRESERVE');symlinkSync(unrelated,settings);
const linked=spawnSync(process.execPath,['--input-type=module','-e',bridge],{input:JSON.stringify({sdk,cwd,agentDir,source,action:'add'}),encoding:'utf8'});
assert.equal(linked.status,3);assert.equal(readFileSync(unrelated,'utf8'),'PRESERVE');
unlinkSync(settings);writeFileSync(settings,JSON.stringify(current));
console.log(JSON.stringify({status:'passed',sdk_version:'1.2.0',fixture:root,checks:['real_native_package_install','native_transaction_add','repeat_idempotent','later_edits_preserved','user_entry_change_refused','concurrent_native_writers_preserved','global_duplicate_rejected','malformed_settings_private','symlink_target_preserved']}));
