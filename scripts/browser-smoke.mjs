// CI-only browser journey, with synthetic data and actual app UI.
import {chromium} from '/tmp/browser-tests/node_modules/playwright/index.mjs';
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import assert from 'node:assert/strict';
const server=createServer(async(req,res)=>{
 try {
  const path=new URL(req.url,'http://localhost').pathname;
  if(path.includes('..'))throw Error();
  const body=await readFile('src'+(path==='/'?'/index.html':path));
  res.setHeader('Content-Type',path.endsWith('.js')?'text/javascript':path.endsWith('.css')?'text/css':'text/html');res.end(body);
 } catch {res.statusCode=404;res.end();}
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const browser=await chromium.launch();
try {
 for(const mobile of [false,true]) {
  const context=await browser.newContext({viewport:mobile?{width:393,height:852}:{width:480,height:650},...(mobile?{userAgent:'Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 Chrome/130.0 Mobile Safari/537.36',isMobile:true,hasTouch:true}:{})});
  const page=await context.newPage();const errors=[];page.on('pageerror',error=>errors.push(error.message));
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  await page.locator('#quick-presets button').first().waitFor();
  await page.locator('[data-page="settings-page"]').click();
  await page.locator('#add-preset').click();
  await page.locator('#preset-editor [data-expand-editor]').last().click();
  await page.locator('[data-preset-field="name"]').last().fill('CI palette');
  await page.locator('[data-preset-field="name"]').last().press('Tab');
  await page.locator('[data-page="controller-page"]').click();
  await page.locator('#quick-presets button').filter({hasText:'CI palette'}).click();
  await page.locator('[data-page="settings-page"]').click();
  await page.locator('#discover-button').click();
  await page.locator('[data-add-discovered]').first().click();
  if(mobile) {
   assert.equal(await page.locator('[data-page="automations-page"]').count(),0);
   assert.equal(await page.locator('#automations-page').count(),0);
   assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
   assert.equal(await page.evaluate(()=>{const nav=document.querySelector('nav').getBoundingClientRect();return [...document.querySelectorAll('nav button')].every(button=>button.getBoundingClientRect().bottom<=nav.bottom);}),true);
  } else {
   await page.locator('[data-page="automations-page"]').click();
   await page.locator('#add-schedule').click();
  }
  assert.deepEqual(errors,[]);
  await context.close();
 }

 const contexts=[];
 const open=async mobile=>{
  const context=await browser.newContext({viewport:{width:393,height:852},...(mobile?{userAgent:'Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 Chrome/130.0 Mobile Safari/537.36',isMobile:true,hasTouch:true}:{})});
  contexts.push(context);
  await context.addInitScript(({mobile})=>{
   const defaults={devices:mobile?[]:[{name:'CI light',identifier:'SYNTHETIC-DESKTOP',profile:'h6005',enabled:true}],presets:mobile?[]:[{name:'CI evening',mode:'color',value:'#ff4500',brightness:0.4}],schedules:[],color:'#ff4500',white:'#ffd5ad',mode:'color',brightness:0.4,connectionHoldSeconds:6,breathingIntervalMs:350,breathingColorStep:1,whiteBreathingSweepSeconds:30};
   window.__TAURI__={core:{invoke:async(command,args={})=>{
    if(command==='get_settings')return JSON.parse(localStorage.getItem('native-settings')??JSON.stringify(defaults));
    if(command==='save_settings'){localStorage.setItem('native-settings',JSON.stringify(args.settings));return;}
    if(command==='connection_status')return {effectActive:false};
    if(command==='privileged_service_status')return {installed:false,healthy:false,current:false,message:'Not installed'};
    if(command==='discover_lights')return [{name:'Govee H6005',identifier:'SYNTHETIC-ANDROID'}];
    if(command==='home_network_status')return {fingerprint:null};
    if(command==='mobile_action')return args.action==='version'?'0.1.0':'';
    if(command==='set_floodlights'){localStorage.setItem('floodlight-request',JSON.stringify(args));return 'Floodlights on';}
    return 'Updated';
   }},event:{listen:async()=>()=>{}}};
  },{mobile});
  const page=await context.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  await page.locator('[data-page="settings-page"]').click();
  return page;
 };
 const desktop=await open(false), phone=await open(true);
 await desktop.locator('#sync-create').click();
 await desktop.locator('#sync-status').filter({hasText:'Synced'}).waitFor({timeout:30000});
 const code=await desktop.locator('#sync-code').inputValue();
 await phone.locator('#sync-code').fill(code);
 await phone.locator('#sync-join').click();
 await phone.locator('#sync-status').filter({hasText:'Synced'}).waitFor({timeout:30000});
 await phone.locator('#device-editor').filter({hasText:'CI light'}).waitFor();
 await phone.locator('#discover-button').click();
 await phone.locator('[data-pair-light]').click();
 await phone.locator('#device-editor').filter({hasText:'SYNTHETIC-ANDROID'}).waitFor();
 await phone.context().setOffline(true);
 await phone.locator('#add-preset').click();
 await phone.locator('#preset-editor [data-expand-editor]').last().click();
 await phone.locator('[data-preset-field="name"]').last().fill('Offline palette');
 await phone.locator('[data-preset-field="name"]').last().press('Tab');
 await phone.locator('[data-page="controller-page"]').click();
 await phone.context().setOffline(false);
 await desktop.locator('#preset-editor').filter({hasText:'Offline palette'}).waitFor({timeout:30000});
 await desktop.reload();
 await desktop.locator('[data-page="settings-page"]').click();
 await desktop.locator('#device-editor').filter({hasText:'SYNTHETIC-DESKTOP'}).waitFor();
 await desktop.locator('#preset-editor').filter({hasText:'Offline palette'}).waitFor();
 await desktop.locator('[data-page="controller-page"]').click();
 await phone.locator('[data-floodlights="true"]:not([data-floodlights-off-after])').click();
 await phone.locator('#status').filter({hasText:/^Floodlights on$/}).waitFor({timeout:30000});
 assert.equal(await desktop.evaluate(()=>JSON.parse(localStorage.getItem('floodlight-request')).on),true);
 for(const context of contexts)await context.close();
} finally {await browser.close();server.close();}
