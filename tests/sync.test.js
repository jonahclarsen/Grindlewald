import test from 'node:test';
import assert from 'node:assert/strict';
import {SyncModel,mergeRecords} from '../src/sync-model.js';
import worker,{Room} from '../relay-worker/worker.js';
const settings=()=>({devices:[{name:'Test lamp',identifier:'SYNTHETIC-MAC-ID',profile:'h6005',enabled:true}],presets:[{name:'Evening',mode:'color',value:'#ff0000',brightness:0.4}],schedules:[{shellCommand:'private-local-command'}]});
let sequence=0;
const model=(platform,stored)=>new SyncModel(platform,stored,()=>`test-${++sequence}`);
test('pairing preserves local automation and cross-platform light addresses',()=>{
 const desktop=model('macos'); desktop.capture(settings());
 const phone=model('android'); phone.merge(desktop.state.records);
 let mobile=phone.materialize({devices:[],presets:[],schedules:[]});
 assert.match(mobile.devices[0].identifier,/^UNPAIRED-/);
 mobile=phone.pair(mobile,mobile.devices[0].identifier,'SYNTHETIC-ANDROID-ID');
 desktop.merge(phone.state.records);
 const local=desktop.materialize(settings());
 assert.equal(local.devices[0].identifier,'SYNTHETIC-MAC-ID');
 assert.equal(local.schedules[0].shellCommand,'private-local-command');
 assert.equal(mobile.devices[0].identifier,'SYNTHETIC-ANDROID-ID');
 assert.ok(!JSON.stringify(desktop.state.records).includes('shellCommand'));
});
test('offline edits, deletes, retries and restart converge without resurrection',()=>{
 const a=model('macos'); a.capture(settings());
 const b=model('android'); b.merge(a.state.records); const bs=b.materialize({devices:[],presets:[]});
 const as=a.materialize(settings()); as.presets=[]; a.capture(as);
 bs.devices[0].name='Renamed'; b.capture(bs);
 const restarted=model('macos',a.state); restarted.merge(b.state.records); b.merge(restarted.state.records);
 assert.deepEqual(restarted.state.records,b.state.records);
 assert.equal(b.materialize(bs).presets.length,0);
 assert.equal(restarted.materialize(as).devices[0].name,'Renamed');
 assert.deepEqual(mergeRecords(b.state.records,b.state.records),b.state.records);
});
test('relay exchanges two clients and rejects automations or unauthenticated requests',async()=>{
 const values=new Map(); const storage={get:async k=>values.get(k),put:async(k,v)=>values.set(k,v),transaction:async fn=>fn(storage)};
 const room=new Room({storage});const env={ROOMS:{idFromName:x=>x,get:()=>room}};
 assert.equal((await worker.fetch(new Request('https://example.test/sync',{method:'POST'}),env)).status,401);
 const a=model('macos');a.capture(settings());
 const request=records=>new Request('https://example.test/sync',{method:'POST',headers:{Authorization:'Bearer '+'a'.repeat(64)},body:JSON.stringify({records})});
 assert.equal((await worker.fetch(request(a.state.records),env)).status,200);
 const response=await (await worker.fetch(request({}),env)).json();assert.deepEqual(response.records,a.state.records);
 const invalid=structuredClone(a.state.records);Object.values(invalid)[0].value.shellCommand='no';
 assert.equal((await worker.fetch(request(invalid),env)).status,400);
});
test('remote control is restricted to floodlights and claimed at most once',async()=>{
 const values=new Map();const storage={get:async k=>values.get(k),put:async(k,v)=>values.set(k,v),transaction:async fn=>fn(storage)};
 const room=new Room({storage});
 const call=body=>room.fetch(new Request('https://example.test/control',{method:'POST',body:JSON.stringify(body)}));
 assert.equal((await call({type:'request',id:'synthetic',on:true,offAfterSeconds:300})).status,200);
 assert.equal((await (await call({type:'claim'})).json()).command.on,true);
 assert.equal((await (await call({type:'claim'})).json()).command,null);
 await call({type:'result',id:'synthetic',ok:true,message:'On'});
 assert.deepEqual((await (await call({type:'status',id:'synthetic'})).json()).result,{ok:true,message:'On'});
 assert.equal((await call({type:'shell',command:'anything'})).status,400);
});
test('concurrent pairing and an offline rename retain both platform addresses',()=>{
 const a=model('macos');a.capture(settings());const b=model('android');b.merge(a.state.records);
 const local=a.materialize(settings());let remote=b.materialize({devices:[],presets:[]});
 local.devices[0].name='Renamed offline';a.capture(local);
 remote=b.pair(remote,remote.devices[0].identifier,'SYNTHETIC-ANDROID');
 const ab=mergeRecords(a.state.records,b.state.records),ba=mergeRecords(b.state.records,a.state.records);
 assert.deepEqual(ab,ba);
 const addresses=Object.values(ab).find(r=>r.kind==='devices').value.addresses;
 assert.deepEqual(addresses,{android:'SYNTHETIC-ANDROID',macos:'SYNTHETIC-MAC-ID'});
});
test('deleting a duplicate offline addition does not reveal the hidden duplicate',()=>{
 const a=model('macos'),b=model('android');a.capture(settings());b.capture(settings());a.merge(b.state.records);
 const merged=a.materialize(settings());assert.equal(merged.presets.length,1);
 merged.presets=[];a.capture(merged);assert.equal(a.materialize(merged).presets.length,0);
});
test('remote renames update local automation references without sharing automations',()=>{
 const initial=settings();initial.schedules=[{preset:'Evening',lights:['Test lamp'],shellCommand:'private'}];
 const desktop=model('macos');desktop.capture(initial);
 const phone=model('android');phone.merge(desktop.state.records);const mobile=phone.materialize({devices:[],presets:[],schedules:[]});
 phone.renamePreset('Evening','Night');mobile.presets[0].name='Night';mobile.devices[0].name='Room';phone.capture(mobile);
 desktop.merge(phone.state.records);const updated=desktop.materialize(initial);
 assert.deepEqual(updated.schedules,[{preset:'Night',lights:['Room'],shellCommand:'private'}]);
 assert.ok(!JSON.stringify(phone.state.records).includes('shellCommand'));
});
test('unpaired identifiers survive native uppercase normalization and restart',()=>{
 const a=model('macos');a.capture(settings());const b=model('android');b.merge(a.state.records);
 const mobile=b.materialize({devices:[],presets:[]});const count=Object.keys(b.state.records).length;
 mobile.devices.forEach(device=>{device.identifier=device.identifier.toUpperCase();});
 const restored=model('android',b.state);restored.capture(mobile);
 assert.equal(Object.keys(restored.state.records).length,count);
 assert.equal(restored.materialize(mobile).devices.length,1);
});
test('one light added separately on each platform merges by its advertised address suffix',()=>{
 const macSettings={devices:[{name:'ihoment_H6005_1A2B',identifier:'SYNTHETIC-MAC-UUID',profile:'h6005',enabled:true}],presets:[],schedules:[{lights:['ihoment_H6005_1A2B']}]};
 const desktop=model('macos');desktop.capture(macSettings);
 const phone=model('android');phone.capture({devices:[{name:'Room',identifier:'00:11:22:33:1A:2B',profile:'h6005',enabled:false}],presets:[]});
 desktop.merge(phone.state.records);phone.merge(desktop.state.records);
 desktop.combineDuplicates();phone.combineDuplicates();
 desktop.merge(phone.state.records);phone.merge(desktop.state.records);
 assert.deepEqual(desktop.state.records,phone.state.records);
 const mac=desktop.materialize(macSettings),android=phone.materialize({devices:[],presets:[]});
 assert.deepEqual(mac.devices,[{name:'Room',profile:'h6005',enabled:true,identifier:'SYNTHETIC-MAC-UUID'}]);
 assert.deepEqual(android.devices,[{name:'Room',profile:'h6005',enabled:true,identifier:'00:11:22:33:1A:2B'}]);
 assert.deepEqual(mac.schedules,[{lights:['Room']}]);
 assert.equal(desktop.combineDuplicates(),false);
});
test('discovery pairs a renamed light synced from the other platform',()=>{
 const desktop=model('macos');const local={devices:[{name:'Desk',identifier:'SYNTHETIC-MAC-UUID',profile:'h6005',enabled:true}],presets:[]};
 desktop.capture(local);desktop.materialize(local);
 assert.equal(desktop.observe([{name:'ihoment_H6005_1A2B',identifier:'SYNTHETIC-MAC-UUID'}]),true);
 const phone=model('android');phone.merge(desktop.state.records);phone.materialize({devices:[],presets:[]});
 assert.equal(phone.observe([{name:'ihoment_H6005_FFFF',identifier:'00:11:22:33:FF:FF'},{name:'ihoment_H6005_1A2B',identifier:'00:11:22:33:1A:2B'}]),true);
 assert.deepEqual(phone.materialize({devices:[],presets:[]}).devices,[{name:'Desk',profile:'h6005',enabled:true,identifier:'00:11:22:33:1A:2B'}]);
});
test('different lights sharing an address suffix on one platform stay separate',()=>{
 const a=model('android');a.capture({devices:[{name:'One',identifier:'00:11:22:33:1A:2B',profile:'h6005',enabled:true},{name:'Two',identifier:'99:11:22:33:1A:2B',profile:'h6005',enabled:true}],presets:[]});
 assert.equal(a.combineDuplicates(),false);
 assert.equal(a.materialize({devices:[],presets:[]}).devices.length,2);
});
test('relay accepts hardware hints and merge tombstones only in their expected shape',async()=>{
 const {validRecord}=await import('../relay-worker/worker.js');
 const value={name:'Room',profile:'h6005',enabled:true,addresses:{android:'00:11:22:33:1A:2B'}};
 assert.ok(validRecord('a',{kind:'devices',clock:1,actor:'x',value:{...value,hint:'1A2B'}}));
 assert.ok(!validRecord('a',{kind:'devices',clock:1,actor:'x',value:{...value,hint:'<script>'}}));
 assert.ok(validRecord('a',{kind:'devices',clock:1,actor:'x',value:null,mergedInto:'b'}));
 assert.ok(!validRecord('a',{kind:'devices',clock:1,actor:'x',value:null,mergedInto:'../b'}));
});
