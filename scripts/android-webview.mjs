// Real Android WebView + Rust persistence, accessed through CI's debug socket.
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
const adb=(...args)=>execFileSync('adb',args,{encoding:'utf8'}).trim();
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
let socketName;
for(let attempt=0;attempt<60;attempt++) {
 socketName=adb('shell','cat','/proc/net/unix').match(/@(webview_devtools_remote_\d+)/)?.[1];
 if(socketName)break;await pause(500);
}
assert.ok(socketName,'WebView debug socket unavailable');
const port=adb('forward','tcp:0',`localabstract:${socketName}`);
let target;
for(let attempt=0;attempt<60;attempt++) {
 try {target=(await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find(t=>t.type==='page');}catch{}
 if(target)break;await pause(500);
}
assert.ok(target,'WebView page unavailable');
const socket=new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve,reject)=>{socket.addEventListener('open',resolve,{once:true});socket.addEventListener('error',reject,{once:true});});
let sequence=0;const pending=new Map();
socket.addEventListener('message',event=>{const value=JSON.parse(event.data);if(value.id){const p=pending.get(value.id);pending.delete(value.id);value.error?p.reject(Error(value.error.message)):p.resolve(value.result);}});
const call=(method,params={})=>new Promise((resolve,reject)=>{const id=++sequence;pending.set(id,{resolve,reject});socket.send(JSON.stringify({id,method,params}));});
const evaluate=async expression=>{const result=await call('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(result.exceptionDetails)throw Error(JSON.stringify(result.exceptionDetails));return result.result.value;};
async function until(expression) {
 for(let attempt=0;attempt<80;attempt++){if(await evaluate(expression))return;await pause(500);}
 throw Error('WebView condition timed out: '+expression);
}
try {
 await until(`document.querySelector('#mobile-settings')?.hidden === false`);
 assert.equal(await evaluate(`document.querySelectorAll('nav [data-page]').length`),2);
 assert.equal(await evaluate(`document.querySelector('#automations-page')===null`),true);
 await evaluate(`document.querySelector('[data-page="settings-page"]').click()`);
 if(process.argv[2]==='prepare') {
  await evaluate(`document.querySelector('#sync-create').click()`);
  await until(`document.querySelector('#sync-status').textContent==='Synced'`);
  await evaluate(`document.querySelector('#add-preset').click()`);
  await pause(500);
  await evaluate(`document.querySelector('#preset-editor [data-expand-editor]:last-child')?.click()`);
  await until(`!!document.querySelector('[data-preset-field="name"]')`);
  await evaluate(`(()=>{const input=document.querySelector('[data-preset-field="name"]');input.value='Android CI retained';input.dispatchEvent(new Event('change',{bubbles:true}));input.blur();})()`);
 }
 await until(`window.__TAURI__.core.invoke('get_settings').then(s=>s.presets.some(p=>p.name==='Android CI retained'))`);
 assert.equal(await evaluate(`document.querySelector('#sync-code').value.startsWith('grindlewald1:')`),true);
 await until(`document.querySelector('#sync-status').textContent==='Synced'`);
 await evaluate(`document.querySelector('[data-page="controller-page"]').click()`);
 assert.equal(await evaluate(`document.documentElement.scrollWidth<=innerWidth`),true);
 console.log('PASS: Android Control/Settings, live sync, and native preset persistence');
} finally {socket.close();adb('forward','--remove',`tcp:${port}`);}
