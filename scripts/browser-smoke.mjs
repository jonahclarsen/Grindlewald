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
  } else {
   await page.locator('[data-page="automations-page"]').click();
   await page.locator('#add-schedule').click();
  }
  assert.deepEqual(errors,[]);
  await context.close();
 }
} finally {await browser.close();server.close();}
