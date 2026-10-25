import {execFileSync} from 'node:child_process';
import {appendFileSync} from 'node:fs';
const run=(command,args)=>execFileSync(command,args,{encoding:'utf8'}).trim();
const sha=run('git',['rev-parse','origin/main']);
const published=JSON.parse(run('gh',['release','list','--limit','100','--json','tagName,isDraft'])).filter(r=>!r.isDraft).map(r=>r.tagName);
const valid=tag=>/^v\d+\.\d+\.\d+$/.test(tag);
const pointing=run('git',['tag','--points-at',sha]).split('\n').filter(valid);
if(pointing.some(tag=>published.includes(tag))) {
 console.log('No new commit since the last published release.');
 appendFileSync(process.env.GITHUB_OUTPUT,'dispatch=false\n');
} else {
 // Never tag code whose CI has not succeeded.
 for(const workflow of ['ci.yml','android.yml']) {
  const runs=JSON.parse(run('gh',['run','list','--workflow',workflow,'--commit',sha,'--limit','50','--json','conclusion']));
  if(!runs.some(r=>r.conclusion==='success'))throw Error(`Waiting for successful ${workflow} verification of main`);
 }
 let tag=pointing[0];
 if(!tag) {
  const tags=run('git',['tag','--list','--sort=-version:refname']).split('\n').filter(valid);
  if(!tags.length) tag='v0.1.0';
  else {
   let [major,minor,patch]=tags[0].slice(1).split('.').map(Number);
   if(++patch>999){patch=0;minor++;}if(minor>999){minor=0;major++;}
   tag=`v${major}.${minor}.${patch}`;
  }
  run('git',['tag',tag,sha]);run('git',['push','origin',`refs/tags/${tag}`]);
 }
 appendFileSync(process.env.GITHUB_OUTPUT,`dispatch=true\ntag=${tag}\n`);
}
