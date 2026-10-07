// Actual deployed console and custom-host OIDC login using owned test identities.
import {createRequire} from 'node:module';
import {readFile,writeFile} from 'node:fs/promises';
const require=createRequire(new URL('../../apps/console/package.json',import.meta.url));
const {chromium,devices,expect,request}=require('@playwright/test');
const [root,endpoint]=process.argv.slice(2);
const fixture=JSON.parse(await readFile(`${root}/fixture.json`,'utf8'));
const state=JSON.parse(await readFile(`${root}/domains-live-state.json`,'utf8'));
const tenant=fixture.tenants[0],oidc=fixture.oidc_test;
if(!tenant.slug.startsWith('vpc-e2e-')||!state.hostname.startsWith('custom-domain-e2e-'))throw new Error('Owned fixture required');
const browser=await chromium.launch();
const base=`${new URL(endpoint).origin}/api/v1/organizations/${tenant.organization_id}/flash/services/${state.service_id}`;
const admin=await request.newContext({extraHTTPHeaders:{Authorization:`Bearer ${tenant.api_key}`}});
const report={checks:[],passed:false,hostname:state.hostname};
async function record(name){report.checks.push({name,passed:true});await writeFile(`${root}/domains-browser-report.json`,JSON.stringify(report,null,2));console.log(`PASS ${name}`);}
async function service(){const r=await admin.get(base,{maxRedirects:0});if(!r.ok())throw new Error(`Service API HTTP ${r.status()}`);const v=await r.json();if(v.organization_id!==tenant.organization_id||v.name!=='custom-domain-e2e')throw new Error('Fixture scope changed');return v;}
async function auth(value){const s=await service();const r=await admin.put(base,{data:{name:s.name,spec:{...s.spec,exposure:{...s.spec.exposure,authentication:value}}},maxRedirects:0});if(!r.ok())throw new Error(`OIDC setup HTTP ${r.status()}`);}
try{
 for(const [name,options] of [['desktop',devices['Desktop Chrome']],['mobile',devices['Pixel 7']]]){
  const context=await browser.newContext(options);
  await context.addCookies([{name:'hc_session',value:fixture.browser_session,url:new URL(endpoint).origin,httpOnly:true,secure:true,sameSite:'Lax'}]);
  const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.name));
  await page.goto(`${endpoint}/flash/services/${state.service_id}`);
  await expect(page.getByRole('heading',{name:'custom-domain-e2e',exact:true})).toBeVisible({timeout:15000});
  await expect(page.getByRole('link',{name:`https://${state.hostname}`,exact:true})).toBeVisible();
  await page.getByRole('button',{name:'編集',exact:true}).click();
  await expect(page.getByRole('textbox',{name:'独自ドメイン',exact:true})).toBeVisible();
  const dialog=page.getByRole('dialog');await expect(dialog.getByText(state.hostname,{exact:true})).toBeVisible();
  if(errors.length)throw new Error('Console runtime errors');
  await record(`deployed_console_domains_${name}`);await context.close();
 }
 const configuration={issuer_url:oidc.issuer_url,client_id:oidc.client_id,client_secret_ref:'domain-oidc-secret',scopes:['openid','profile','email']};
 await auth(configuration);
 await expect.poll(async()=>{const r=await admin.put(`${base}/load-balancer/secrets/domain-oidc-secret`,{data:{value:oidc.client_secret},maxRedirects:0});if(![204,503].includes(r.status()))throw new Error(`OIDC secret write HTTP ${r.status()}`);return r.status();},{timeout:45000,intervals:[1000]}).toBe(204);
 await expect.poll(async()=>{const r=await admin.get(`${base}/domains`);return (await r.json()).items.find(d=>d.id===state.domain_id)?.phase;},{timeout:180000,intervals:[2000]}).toBe('ready');
 for(const [name,options] of [['desktop',devices['Desktop Chrome']],['mobile',devices['Pixel 7']]]){
  const context=await browser.newContext(options);const page=await context.newPage();
  const target=`https://${state.hostname}/index.html?domain_oidc=${name}`;await page.goto(target);
  await page.locator('#username').fill(oidc.login_username??oidc.username);await page.locator('#password').fill(oidc.password);await page.locator('#kc-login').click();
  await expect(page.getByRole('heading',{name:'Welcome to nginx!'})).toBeVisible({timeout:30000});
  if(new URL(page.url()).hostname!==state.hostname)throw new Error('OIDC returned to another hostname');
  const cookies=await context.cookies(`https://${state.hostname}`);const authCookies=cookies.filter(c=>c.name.startsWith('HcAccessToken-')||c.name.startsWith('HcIdToken-'));
  if(!authCookies.length||authCookies.some(c=>!c.httpOnly||!c.secure||c.domain!==state.hostname))throw new Error('OIDC cookie scope differs');
  await record(`custom_domain_real_oidc_${name}`);await context.close();
 }
 await auth(null);await expect.poll(async()=> (await service()).state,{timeout:180000,intervals:[2000]}).toBe('ready');
 await record('oidc_removed_and_public_access_restored');report.passed=true;await writeFile(`${root}/domains-browser-report.json`,JSON.stringify(report,null,2));
}finally{await browser.close();await admin.dispose();}
