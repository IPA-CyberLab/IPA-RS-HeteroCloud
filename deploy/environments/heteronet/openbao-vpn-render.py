#!/usr/bin/env python3
"""Render a private OpenBao Caddyfile from the actively synced public TLS pair."""

import argparse
import ipaddress
import os
from pathlib import Path
import re
import stat
import tempfile


EXTRA = Path('/etc/heteronetwork/public-gateway-extra.Caddyfile')
OUTPUT = Path('/run/heteronetwork-openbao-vpn/Caddyfile')
CERT_LINE = re.compile(
    r'^\s*tls (/etc/heteronetwork/flash-web-certs/([0-9a-f]{64})/tls\.crt) '
    r'(/etc/heteronetwork/flash-web-certs/\2/tls\.key)\s*$', re.MULTILINE
)
BEGIN = '# BEGIN managed flash-web TLS\n'
END = '# END managed flash-web TLS\n'


def render(extra: str, vpn_ip: str) -> str:
    address = ipaddress.ip_address(vpn_ip)
    if address.version != 4 or vpn_ip not in ('10.250.0.10', '10.250.0.11'):
        raise ValueError('VPN listener must be a commissioned gateway address')
    if extra.count(BEGIN) != 1 or extra.count(END) != 1:
        raise ValueError('managed TLS block is missing or ambiguous')
    managed = extra.split(BEGIN, 1)[1].split(END, 1)[0]
    if managed.count('(heterocloud_public_tls) {') != 1:
        raise ValueError('public TLS snippet is missing or ambiguous')
    matches = CERT_LINE.findall(managed)
    if len(matches) != 1:
        raise ValueError('public TLS certificate pair is missing or ambiguous')
    cert, _, key = matches[0]
    for raw in (cert, key):
        path = Path(raw)
        metadata = path.stat()
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_mode & 0o022:
            raise ValueError('public TLS file has unsafe permissions')
    return f'''{{
    admin off
    auto_https disable_redirects
}}

https://secrets.heterocloud.mizuame.app {{
    bind {vpn_ip}
    tls {cert} {key}
    header Strict-Transport-Security "max-age=31536000; includeSubDomains"
    reverse_proxy http://openbao-vpn-proxy.envoy-gateway-system.svc.cluster.local:18083 {{
        header_up Host secrets.heterocloud.mizuame.app
        lb_try_duration 3s
        lb_try_interval 100ms
        health_uri /v1/sys/health?standbyok=true
        health_headers {{
            Host secrets.heterocloud.mizuame.app
        }}
        health_interval 2s
        health_timeout 2s
        health_status 2xx
        transport http {{
            resolvers 10.96.0.10
        }}
    }}
}}
'''


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument('--vpn-ip', required=True)
    args = parser.parse_args()
    contents = render(EXTRA.read_text(encoding='utf-8'), args.vpn_ip)
    OUTPUT.parent.mkdir(mode=0o750, parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', dir=OUTPUT.parent,
                                     prefix='.Caddyfile.', delete=False) as stream:
        temporary = Path(stream.name)
        try:
            stream.write(contents)
            stream.flush()
            os.fchmod(stream.fileno(), 0o640)
            os.fsync(stream.fileno())
        except BaseException:
            temporary.unlink(missing_ok=True)
            raise
    os.replace(temporary, OUTPUT)


if __name__ == '__main__':
    main()
