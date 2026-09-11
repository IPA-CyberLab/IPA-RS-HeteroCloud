# Test-Only TLS Material

These synthetic fixtures are unrelated to any deployed certificate or account.
The server key is intentionally public test data. Never use it for a service.
Certificates are valid from 2020-01-01 until 2040-01-01 and must be replaced
before expiry. The server certificate covers only the loopback IP 127.0.0.1;
localhost is deliberately not covered, for the hostname-rejection test.
