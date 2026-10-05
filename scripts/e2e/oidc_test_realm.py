#!/usr/bin/env python3
"""Create/remove only owned Keycloak test resources over authorized SSH.

An administrator password is read on the server; it never leaves the server.
The sudo password is taken from stdin and never logged or placed in arguments.
"""
import argparse
import json
from pathlib import Path
import shlex
import subprocess
import sys

REMOTE = r'''
import json,sys,urllib.request,urllib.parse,urllib.error
from pathlib import Path
request=json.load(sys.stdin)
test=request['test'];realm=test['realm']
assert all(c.isalnum() or c=='-' for c in realm)
shared=test.get('shared_realm',False)
if shared:
 assert realm!='master' and test['client_id'].startswith('hc-oidc-e2e-') and test['username'].startswith('hc-oidc-e2e-user-')
else: assert realm.startswith('hc-oidc-e2e-')
class NoRedirect(urllib.request.HTTPRedirectHandler):
 def redirect_request(self,*args,**kwargs): return None
opener=urllib.request.build_opener(urllib.request.ProxyHandler({}),NoRedirect())
base='http://127.0.0.1:18080'
def call(method,path,body=None,token=None):
 headers={'Content-Type':'application/json'}
 if token: headers['Authorization']='Bearer '+token
 data=None if body is None else json.dumps(body).encode()
 if path.endswith('/token'):
  headers['Content-Type']='application/x-www-form-urlencoded'
  data=urllib.parse.urlencode(body).encode()
 req=urllib.request.Request(base+path,data=data,headers=headers,method=method)
 try:
  with opener.open(req,timeout=15) as response:
   data=response.read();return json.loads(data) if data else None
 except urllib.error.HTTPError as error:
  if method=='DELETE' and error.code==404: return None
  raise RuntimeError('Identity-provider operation failed: HTTP '+str(error.code)) from None
password=Path('/etc/heteronetwork/keycloak/bootstrap-admin.password').read_text().strip()
token=call('POST','/realms/master/protocol/openid-connect/token',{'grant_type':'password','client_id':'admin-cli','username':'admin','password':password})['access_token']
if shared:
 prefix='/admin/realms/'+realm
 clients=call('GET',prefix+'/clients?'+urllib.parse.urlencode({'clientId':test['client_id']}),token=token)
 users=call('GET',prefix+'/users?'+urllib.parse.urlencode({'username':test['username'],'exact':'true'}),token=token)
 assert len(clients)<=1 and len(users)<=1
 assert all(c['clientId']==test['client_id'] and c.get('attributes',{}).get('hc-e2e-nonce')==test['nonce'] for c in clients)
 email=test['username']+'@example.invalid'
 assert all(u['username']==test['username'] and u['email']==email for u in users)
 if request['action']=='remove':
  for client in clients: call('DELETE',prefix+'/clients/'+client['id'],token=token)
  for user in users: call('DELETE',prefix+'/users/'+user['id'],token=token)
  print('PASS owned OIDC test client and user removed from shared realm')
 else:
  if not clients:
   call('POST',prefix+'/clients',{'clientId':test['client_id'],'enabled':True,'publicClient':False,'secret':test['client_secret'],'protocol':'openid-connect','standardFlowEnabled':True,'directAccessGrantsEnabled':False,'redirectUris':[test['callback_url']],'webOrigins':[],'attributes':{'hc-e2e-nonce':test['nonce']}},token)
  if not users:
   call('POST',prefix+'/users',{'username':test['username'],'enabled':True,'emailVerified':True,'email':email,'firstName':'OIDC','lastName':'E2E','requiredActions':[],'credentials':[{'type':'password','value':test['password'],'temporary':False}]},token)
  print('PASS owned OIDC test client and user created in shared realm')
elif request['action']=='remove':
 call('DELETE','/admin/realms/'+realm,token=token)
 print('PASS temporary OIDC test realm removed')
else:
 call('POST','/admin/realms',{'realm':realm,'enabled':True,'sslRequired':'external','registrationAllowed':False},token)
 call('POST','/admin/realms/'+realm+'/clients',{'clientId':test['client_id'],'enabled':True,'publicClient':False,'secret':test['client_secret'],'protocol':'openid-connect','standardFlowEnabled':True,'directAccessGrantsEnabled':False,'redirectUris':[test['callback_url']],'webOrigins':[]},token)
 call('POST','/admin/realms/'+realm+'/users',{'username':test['username'],'enabled':True,'emailVerified':True,'email':'oidc-e2e@example.invalid','firstName':'OIDC','lastName':'E2E','requiredActions':[],'credentials':[{'type':'password','value':test['password'],'temporary':False}]},token)
 print('PASS isolated real OIDC realm, client and test user created')
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["create", "remove"])
    parser.add_argument("--fixture", required=True, type=Path)
    parser.add_argument("--ssh-host", required=True)
    parser.add_argument("--ssh-key", required=True)
    parser.add_argument("--known-hosts", required=True)
    args = parser.parse_args()
    assert args.fixture.stat().st_mode & 0o077 == 0
    test = json.loads(args.fixture.read_text())["oidc_test"]
    sudo_password = sys.stdin.readline().rstrip("\r\n")
    payload = json.dumps({"action": args.action, "test": test})
    result = subprocess.run([
        "ssh", "-i", args.ssh_key, "-o", "StrictHostKeyChecking=yes",
        "-o", "UserKnownHostsFile=" + args.known_hosts, args.ssh_host,
        "sudo -k -S -p '' python3 -c " + shlex.quote(REMOTE),
    ], input=sudo_password + "\n" + payload, text=True, capture_output=True)
    if result.returncode:
        raise RuntimeError("Temporary OIDC realm operation failed; inspect server diagnostics privately")
    print(result.stdout.strip())


if __name__ == "__main__":
    main()
