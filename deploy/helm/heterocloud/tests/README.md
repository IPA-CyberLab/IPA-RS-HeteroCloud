# Host Alias Render Checks

Prerequisites: Helm 3, Python 3, and the test dependencies below.

```sh
python3 -m venv /tmp/heterocloud-chart-tests
/tmp/heterocloud-chart-tests/bin/pip install -r deploy/helm/heterocloud/tests/requirements.txt
/tmp/heterocloud-chart-tests/bin/python deploy/helm/heterocloud/tests/test_host_aliases.py
helm lint deploy/helm/heterocloud
```

The top-level `hostAliases` value accepts a list of `{ip, hostnames}` objects.
Each IP must be an IPv4 or IPv6 address; each nonempty hostname list contains
lowercase DNS names. Both API and enabled owner-console pods receive the same
list. Worker pods, DNS policies, and other pod settings are unchanged.

The default is `[]`, which omits the pod field. Supply environment-specific
addresses through an external values override, not chart defaults. The checks
use documentation-only addresses and `helm template`; they do not contact a
cluster or deploy resources.
