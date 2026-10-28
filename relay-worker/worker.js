import { mergeRecords } from '../src/sync-model.js';
const headers = {'Access-Control-Allow-Origin':'*','Access-Control-Allow-Headers':'Authorization,Content-Type','Access-Control-Allow-Methods':'POST,OPTIONS','Content-Type':'application/json','Cache-Control':'no-store'};
const json = (value, status=200) => new Response(JSON.stringify(value),{status,headers});
export function validRecord(id, record) {
  if (!/^[a-zA-Z0-9-]{1,80}$/.test(id) || !record || !Number.isSafeInteger(record.clock) || record.clock < 1 || !/^[a-zA-Z0-9-]{1,80}$/.test(record.actor)) return false;
  if (!['devices','presets'].includes(record.kind)) return false;
  const value = record.value;
  if (value === null) return record.mergedInto === undefined || /^[a-zA-Z0-9-]{1,80}$/.test(record.mergedInto);
  if (!value || typeof value.name !== 'string' || !value.name.trim() || value.name.length > 100) return false;
  if (record.kind === 'presets') return ['color','white'].includes(value.mode) && /^#[0-9a-f]{6}$/i.test(value.value) && Number.isFinite(value.brightness) && value.brightness >= 0 && value.brightness <= 1 && Object.keys(value).every(k => ['name','mode','value','brightness'].includes(k));
  return ['classic','h6005'].includes(value.profile) && typeof value.enabled === 'boolean' && value.addresses && Object.entries(value.addresses).every(([platform,address]) => ['macos','android'].includes(platform) && typeof address === 'string' && /^[a-zA-Z0-9:-]{1,100}$/.test(address)) && (value.hint === undefined || /^[0-9A-F]{4}$/.test(value.hint)) && Object.keys(value).every(k => ['name','profile','enabled','addresses','hint'].includes(k));
}
export class Room {
  constructor(state) { this.storage=state.storage; }
  async control(request) {
    const text=await request.text();
    if(text.length>2000)return json({error:'Too large'},413);
    let input;try{input=JSON.parse(text);}catch{return json({error:'Invalid JSON'},400);}
    return this.storage.transaction(async storage=>{
      const now=Date.now();
      if(input.type==='request') {
        if(!/^[a-zA-Z0-9-]{1,80}$/.test(input.id) || typeof input.on!=='boolean' || ![null,300].includes(input.offAfterSeconds))return json({error:'Invalid command'},400);
        const previous=await storage.get('control');
        if(previous && now-previous.created<25000)return json({error:'A command is already pending'},409);
        await storage.put('control',{id:input.id,on:input.on,offAfterSeconds:input.offAfterSeconds,created:now});
        return json({ok:true});
      }
      if(input.type==='claim') {
        const command=await storage.get('control');
        if(!command || command.claimed || now-command.created>15000)return json({command:null});
        await storage.put('control',{...command,claimed:true});
        return json({command});
      }
      const command=await storage.get('control');
      if(input.type==='result') {
        if(command?.id!==input.id || !command.claimed || typeof input.ok!=='boolean' || typeof input.message!=='string')return json({error:'Invalid result'},400);
        await storage.put('control',{...command,result:{ok:input.ok,message:input.message.slice(0,500)}});
        return json({ok:true});
      }
      if(input.type==='status')return json({result:command?.id===input.id ? command.result ?? null : null});
      return json({error:'Unknown command'},400);
    });
  }
  async fetch(request) {
    if(new URL(request.url).pathname==='/control')return this.control(request);
    if (Number(request.headers.get('Content-Length')) > 256000) return json({error:'Too large'},413);
    const text = await request.text();
    if (text.length > 256000) return json({error:'Too large'},413);
    let incoming;
    try { incoming=JSON.parse(text); } catch { return json({error:'Invalid JSON'},400); }
    if (!incoming.records || Object.keys(incoming).some(key => !['records'].includes(key)) || Object.entries(incoming.records).some(([id,r]) => !validRecord(id,r))) return json({error:'Invalid records'},400);
    return this.storage.transaction(async storage => {
      const records=mergeRecords(await storage.get('records') ?? {}, incoming.records);
      if (Object.keys(records).length > 512 || JSON.stringify(records).length > 120000) return json({error:'Room is full'},413);
      await storage.put('records',records);
      return json({records});
    });
  }
}
export default {
  async fetch(request, env) {
    if (request.method === 'OPTIONS') return new Response(null,{headers});
    if (request.method !== 'POST' || !['/sync','/control'].includes(new URL(request.url).pathname)) return json({error:'Not found'},404);
    const token=request.headers.get('Authorization')?.replace(/^Bearer /,'');
    if (!/^[a-f0-9]{64}$/.test(token ?? '')) return json({error:'Pairing code required'},401);
    const digest=await crypto.subtle.digest('SHA-256',new TextEncoder().encode(token));
    const name=Array.from(new Uint8Array(digest),b=>b.toString(16).padStart(2,'0')).join('');
    return env.ROOMS.get(env.ROOMS.idFromName(name)).fetch(request);
  }
};
