import { chromium } from 'playwright';
import { readFileSync, existsSync, statSync } from 'node:fs';
const B='http://154.219.107.44:31250', D=new URL('../dist',import.meta.url).pathname;
const types={js:'text/javascript',css:'text/css',html:'text/html',json:'application/json',svg:'image/svg+xml'};
const b=await chromium.launch({executablePath:'/root/.cache/ms-playwright/chromium-1243/chrome-linux64/chrome'});
const ctx=await b.newContext();const p=await ctx.newPage();
await ctx.route(B+'/**',async r=>{
  const u=new URL(r.request().url());
  if(u.pathname.startsWith('/api')||u.pathname.startsWith('/mcp')) return r.continue();
  let f=D+u.pathname; 
  if(!existsSync(f)||statSync(f).isDirectory()){ if(r.request().resourceType()!=='document') return r.continue(); f=D+'/index.html'; }
  let body=readFileSync(f); const ext=f.split('.').pop();
  if(ext==='html') body=body.toString().replaceAll('__CSP_NONCE__','');
  r.fulfill({body,contentType:types[ext]??'application/octet-stream'});
});
p.on('console',m=>{if(m.type()==='error')console.log('ERR',m.text().slice(0,900))});
await p.goto(B+'/login');
await p.fill('input[name=username]','myth');await p.fill('input[name=password]','7pSE9fSwNT8fC36');
await p.click('button[type=submit]');await p.waitForTimeout(4000);
for(const path of ['/','/torrents','/upload','/settings']){await p.goto(B+path);await p.waitForTimeout(2500);console.log(path,(await p.locator('main').first().innerText().catch(()=>'')).replace(/\s+/g,' ').slice(0,80));}
await b.close();
