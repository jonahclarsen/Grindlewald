import {SyncModel} from './sync-model.js';
const ENDPOINT = 'https://grindlewald-sync.cf-cuicn.workers.dev';
const STORAGE = 'grindlewald-sync-v1';
export async function createSync({platform, getSettings, applySettings, executeFloodlights, onChange = () => {},
  load = async () => JSON.parse(localStorage.getItem(STORAGE) ?? 'null'),
  store = async value => localStorage.setItem(STORAGE, JSON.stringify(value)),
  copy = text => navigator.clipboard.writeText(text),
}) {
  const stored = await load() ?? {};
  let token=stored.token ?? '';
  let model=new SyncModel(platform, stored.model);
  let busy=false, timer;
  const status=document.querySelector('#sync-status');
  const code=document.querySelector('#sync-code');
  let saveChain=Promise.resolve(), lastPersisted=JSON.stringify(stored);
  const persist=()=>{
    const snapshot=JSON.stringify({token,model:model.state});
    saveChain=saveChain.catch(()=>{}).then(async()=>{
      if(snapshot===lastPersisted)return;
      await store(JSON.parse(snapshot)); lastPersisted=snapshot;
    });
    return saveChain;
  };
  const display=()=>{ code.value=token ? `grindlewald1:${token}` : ''; document.querySelector('#sync-disconnect').hidden=!token; };
  const request=async(path, body)=>{
    if (!token) throw Error('Pair your devices in Settings first');
    const response=await fetch(`${ENDPOINT}/${path}`,{method:'POST',headers:{Authorization:`Bearer ${token}`,'Content-Type':'application/json'},body:JSON.stringify(body),signal:AbortSignal.timeout(12000)});
    if (!response.ok) throw Error(`Sync unavailable (${response.status})`);
    return response.json();
  };
  const capture=async()=>{ if (token) {model.capture(getSettings());await persist();} };
  const apply=async()=>{
    const before=getSettings(); const next=model.materialize(before);
    if (JSON.stringify([before.devices,before.presets]) !== JSON.stringify([next.devices,next.presets])) {
      await applySettings(next); onChange();
    }
    await persist();
  };
  async function exchange() {
    if (!token || busy) return;
    // Do not replace an editor while someone is typing in it.
    if (document.hasFocus() && document.activeElement?.matches('input,select,textarea')) return;
    busy=true;
    try {
      await capture();
      const result=await request('sync',{records:model.state.records});
      await capture(); // Preserve edits made during the network request.
      model.merge(result.records); await apply();
      status.textContent='Synced';
      if (platform==='macos') {
        const {command}=await request('control',{type:'claim'});
        if (command) {
          let result;
          try { result={ok:true,message:await executeFloodlights(command)}; }
          catch (error) {result={ok:false,message:String(error)};}
          await request('control',{type:'result',id:command.id,...result});
        }
      }
    } catch { status.textContent='Offline · changes saved here'; }
    finally { busy=false; }
  }
  async function connect(join) {
    if (busy) return;
    busy=true;
    const previous={token,model};
    try {
      const value=code.value.trim().replace(/^grindlewald1:/,'');
      if (join && !/^[a-f0-9]{64}$/.test(value)) throw Error('Paste the pairing code from your other device');
      token=join ? value : Array.from(crypto.getRandomValues(new Uint8Array(32)),b=>b.toString(16).padStart(2,'0')).join('');
      model=new SyncModel(platform);
      const {records}=await request('sync',{records:{}});
      if (join) {
        model.merge(records);
        // Match existing lights by name; keep this device's Bluetooth identity.
        const local=getSettings();
        const remote=model.materialize(local);
        for (const kind of ['devices','presets']) {
          for (const item of local[kind]) {
            const matching=remote[kind].find(value=>value.name===item.name);
            if (!matching) remote[kind].push(item);
            else if (kind==='devices' && !item.identifier.startsWith('UNPAIRED-')) {
              const id=model.state.ids.devices[matching.identifier];
              const record=model.state.records[id];
              model.write(id,'devices',{...record.value,addresses:{...record.value.addresses,[platform]:item.identifier}});
              const oldIdentifier=matching.identifier;
              matching.identifier=item.identifier;
              const baseline=model.state.baseline.devices.find(value=>value.identifier===oldIdentifier);
              if(baseline) baseline.identifier=item.identifier;
              delete model.state.ids.devices[oldIdentifier];
              model.state.ids.devices[item.identifier]=id;
            }
          }
        }
        model.capture(remote);
        await applySettings(model.materialize(remote)); onChange();
      } else model.capture(getSettings());
      const result=await request('sync',{records:model.state.records});model.merge(result.records);await apply();
      await persist();display();status.textContent='Synced';
    } catch(error) {token=previous.token;model=previous.model;status.textContent=String(error.message ?? error);}
    finally {busy=false;}
  }
  document.querySelector('#sync-create').onclick=()=>connect(false);
  document.querySelector('#sync-join').onclick=()=>connect(true);
  document.querySelector('#sync-copy').onclick=async()=>{
    try {await copy(code.value);status.textContent='Code copied';}
    catch {status.textContent='Could not copy the code';}
  };
  document.querySelector('#sync-disconnect').onclick=async()=>{token='';await persist();display();status.textContent='Sync off';};
  display();status.textContent=token?'Connecting…':'Pair once to share lights and presets';
  const schedule=()=>{clearTimeout(timer);timer=setTimeout(async()=>{await exchange();schedule();},platform==='macos'?5000:8000);};
  schedule();exchange();
  window.addEventListener('online',exchange);
  window.addEventListener('focus',exchange);
  document.addEventListener('visibilitychange',()=>{if(!document.hidden) exchange();});
  return {
    changed(){return capture();},
    renamePreset(oldName,name){if(token)model.renamePreset(oldName,name);},
    async pair(identifier,replacement){await applySettings(model.pair(getSettings(),identifier,replacement));await persist();onChange();exchange();},
    async floodlights(args){
      const id=crypto.randomUUID();
      await request('control',{type:'request',id,on:args.on,offAfterSeconds:args.offAfterSeconds ?? null});
      for(let attempt=0;attempt<20;attempt++) {
        await new Promise(resolve=>setTimeout(resolve,1000));
        const {result}=await request('control',{type:'status',id});
        if(result){if(!result.ok)throw Error(result.message);return result.message;}
      }
      throw Error('Mac unavailable. Open Grindlewald on your paired Mac and try again.');
    },
  };
}
