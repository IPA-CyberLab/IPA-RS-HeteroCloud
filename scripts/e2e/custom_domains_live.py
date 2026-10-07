#!/usr/bin/env python3
"""Owned live-domain fixture, authenticated API checks, HTTPS and DNS cleanup.

Use --work-dir containing vpc_fixture.py's private fixture.json. Kubernetes
access is only used to read the configured DNS credential in memory and inspect
this fixture's resources. Never emits credentials, cookies or certificate keys.
"""
import argparse,base64,json,os,secrets,subprocess,time,urllib.error,urllib.parse,urllib.request
from pathlib import Path

class Test:
    def __init__(self,a):
        self.a=a;self.root=a.work_dir;self.fixture=json.loads((self.root/'fixture.json').read_text());self.tenant=self.fixture['tenants'][0]
        self.state_file=self.root/'domains-live-state.json';self.state=json.loads(self.state_file.read_text()) if self.state_file.exists() else {'dns_records':[],'service_id':None,'domain_id':None,'hostname':'custom-domain-e2e-'+secrets.token_hex(6)+'.'+a.zone_name}
        self.kub=[a.kubectl,'--kubeconfig',str(a.kubeconfig),'--request-timeout=12s'];self.token=None
    def save(self):
        self.state_file.write_text(json.dumps(self.state));self.state_file.chmod(0o600)
    def call(self,base,path,method='GET',body=None,key=None,statuses=(200,)):
        headers={'User-Agent':'HeteroCloud-Custom-Domains-E2E/1.0'};data=None
        if key:headers['Authorization']='Bearer '+key
        if body is not None:data=json.dumps(body).encode();headers['Content-Type']='application/json'
        try:
            with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(urllib.request.Request(base.rstrip('/')+path,data=data,headers=headers,method=method),timeout=20) as r:status=r.status;raw=r.read(1024*1024)
        except urllib.error.HTTPError as r:status=r.code;raw=r.read(1024*1024)
        if status not in statuses:raise RuntimeError(f'{method} {path}: unexpected HTTP {status}')
        return json.loads(raw) if raw else {},status
    def api(self,path,method='GET',body=None,other=False,statuses=(200,)):
        tenant=self.fixture['tenants'][int(other)];return self.call(self.a.endpoint,'/api/v1/organizations/'+tenant['organization_id']+path,method,body,tenant['api_key'],statuses)[0]
    def cf(self,path,method='GET',body=None):
        if self.token is None:
            secret=json.loads(subprocess.check_output(self.kub+['-n','heterocloud-dns','get','secret','heterocloud-dns-provider','-o','json']))
            self.token=base64.b64decode(secret['data']['CF_API_TOKEN']).decode().strip()
        d,_=self.call('https://api.cloudflare.com/client/v4',path,method,body,self.token,(200,))
        if not d['success']:raise RuntimeError('DNS-provider operation failed')
        return d['result']
    def zone(self):
        zones=self.cf('/zones?'+urllib.parse.urlencode({'name':self.a.zone_name}));assert len(zones)==1;return zones[0]['id']
    def setup(self):
        if not self.state['service_id']:
            spec={'region':'heteronet-global','image':'docker.io/nginxinc/nginx-unprivileged@sha256:ed04ec1ff34502c339ee5c3ae3f855442398edc1d05591e2b98981dcbbd20b1e','replicas':1,'cpu_millis':100,'memory_mib':128,'ephemeral_storage_gib':1,'ports':[{'name':'http','protocol':'tcp','container_port':8080}],'exposure':{'type':'public','traffic_mode':'forwarded','endpoint_mode':'web'},'env':{},'command':[],'args':[],'metadata':{}}
            service=self.api('/flash/services','POST',{'project_id':self.tenant['project_id'],'name':'custom-domain-e2e','spec':spec},statuses=(202,201));self.state['service_id']=service['id'];self.save()
        sid=self.state['service_id'];path='/flash/services/'+sid+'/domains'
        for invalid in ['*.example.org','127.0.0.1','console.heteronetwork.internal','https://app.example.org','app.example.org:443']:
            self.api(path,'POST',{'hostname':invalid},statuses=(400,))
        self.api(path,'POST',{'hostname':self.state['hostname']},other=True,statuses=(403,404))
        domain=self.api(path,'POST',{'hostname':self.state['hostname']},statuses=(202,));self.state['domain_id']=domain['id'];self.save()
        print(json.dumps({'registered':True,'service_id':sid,'hostname':self.state['hostname']}),flush=True)
    def dns(self):
        sid=self.state['service_id'];deadline=time.monotonic()+180
        while time.monotonic()<deadline:
            items=self.api('/flash/services/'+sid+'/domains')['items'];d=next(x for x in items if x['id']==self.state['domain_id'])
            if d.get('cname_target'):break
            time.sleep(3)
        else:raise RuntimeError('Domain provider did not publish its DNS target')
        assert d['phase']=='pending_dns',d['phase']
        if not self.state['dns_records']:
            zid=self.zone();record=self.cf('/zones/'+zid+'/dns_records','POST',{'type':'CNAME','name':self.state['hostname'],'content':d['cname_target'],'ttl':60,'proxied':False});self.state['dns_records'].append({'zone_id':zid,'id':record['id'],'name':record['name']});self.state['cname_target']=d['cname_target'];self.save()
        print(json.dumps({'dns_only_cname_created':True,'hostname':self.state['hostname'],'target':d['cname_target']}),flush=True)
    def verify(self):
        sid=self.state['service_id'];deadline=time.monotonic()+self.a.timeout;last=None
        while time.monotonic()<deadline:
            items=self.api('/flash/services/'+sid+'/domains')['items'];d=next(x for x in items if x['id']==self.state['domain_id'])
            if d['phase']!=last:print(json.dumps({'phase':d['phase'],'hostname':self.state['hostname']}),flush=True);last=d['phase']
            if d['phase']=='ready':break
            time.sleep(5)
        else:raise RuntimeError('Custom domain readiness deadline exceeded')
        for url in ['https://'+self.state['hostname'],'https://'+d['cname_target']]:
            with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(url,timeout=15) as r:assert r.status==200 and b'Welcome to nginx!' in r.read(65536)
        for ip in self.a.origins.split(','):
            r=subprocess.run(['curl','--noproxy','*','--fail','--silent','--show-error','--max-time','15','--resolve',self.state['hostname']+':443:'+ip,'https://'+self.state['hostname']],capture_output=True)
            assert r.returncode==0 and b'Welcome to nginx!' in r.stdout,'Origin HTTPS validation failed'
        self.state['https_verified']=True;self.save();print(json.dumps({'https_verified':True,'default_url_verified':True,'origins_verified':len(self.a.origins.split(',')),'hostname':self.state['hostname']}),flush=True)
    def cleanup(self):
        sid=self.state['service_id']
        if sid and self.state['domain_id']:
            self.api('/flash/services/'+sid+'/domains/'+self.state['domain_id'],'DELETE',statuses=(202,404));deadline=time.monotonic()+180
            while time.monotonic()<deadline:
                if not self.api('/flash/services/'+sid+'/domains')['items']:break
                time.sleep(3)
            else:raise RuntimeError('Domain finalizer cleanup exceeded deadline')
        for record in self.state['dns_records']:
            current=self.cf('/zones/'+record['zone_id']+'/dns_records/'+record['id'])
            assert current['name']==self.state['hostname'] and current['type']=='CNAME','DNS fixture identity changed'
            self.cf('/zones/'+record['zone_id']+'/dns_records/'+record['id'],'DELETE')
        self.state['dns_records']=[];self.save()
        if sid:
            # The real OIDC browser test uses this fixture-owned reference only.
            names=self.api('/flash/services/'+sid+'/secrets')['items']
            if 'domain-oidc-secret' in names:
                self.api('/flash/services/'+sid+'/load-balancer/secrets/domain-oidc-secret','DELETE',statuses=(204,404))
            self.api('/flash/services/'+sid,'DELETE',statuses=(202,404))
        print('Owned custom-domain alias, DNS and service cleanup requested',flush=True)

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('action',choices=['setup','dns','verify','cleanup']);p.add_argument('--work-dir',type=Path,required=True);p.add_argument('--endpoint',required=True);p.add_argument('--zone-name',required=True);p.add_argument('--origins',required=True);p.add_argument('--kubectl',default='kubectl');p.add_argument('--kubeconfig',type=Path,required=True);p.add_argument('--timeout',type=int,default=600);a=p.parse_args();getattr(Test(a),a.action)()
if __name__=='__main__':main()
