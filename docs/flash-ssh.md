# Flash SSH

Open **Edit Flash service**, enable SSH, and save the service. In the same
editor, paste an OpenSSH public key or look up a GitHub username and select
one of the returned fingerprints. Register the key, choose it from the
dropdown, and allow it for that service. Key changes take effect immediately;
the SSH switch takes effect when the service changes are saved. SSH is off
by default, including for services created before this setting existed.
When SSH is the only changed setting, the running container is not restarted.
Registering or assigning a key does not enable SSH. GitHub imports
are snapshots; removing a key on GitHub does not remove it from HeteroCloud.
The server fetches GitHub keys again at registration and refuses keys that no
longer belong to the named account. Never paste a private key.

A registered key represents the user who registered it. Anyone holding the
corresponding private key can exercise that user's `flash:ExecInstance`
permission on the specifically assigned service. A user must remain active in
the organization and keep that permission. Removing the assignment, deleting
the key, suspending the user or withdrawing the permission prevents new SSH
sessions; established channels are rechecked approximately every 30 seconds.
Disabling SSH also closes established channels at the next check.

The service page shows the SSH host-key fingerprint and the commands for the
deployment. With the CLI installed, the following uses HTTPS and needs no
dedicated external SSH port:

```sh
heterocloud --endpoint https://cloud.example.com --organization-id "$ORG_ID" \
  flash ssh "$SERVICE_ID" -i ~/.ssh/id_ed25519
```

Omit `-i` to use the local SSH agent or OpenSSH's default identities. Standard
OpenSSH host-key verification applies. Compare the first connection's
fingerprint with the service page. When the operator has configured a public
direct TCP endpoint, standard OpenSSH can connect without a VPN:

```sh
ssh -p 2222 -l "$ORG_ID+$SERVICE_ID" ssh.flash.example.com
```

Use the hostname and port shown on the service page for your installation.
The username for a direct connection is `ORG_ID+SERVICE_ID`, not the container's
Unix username.

The platform connects to an existing running replica without installing or
starting `sshd` in the image. Interactive shells and remote commands require
`/bin/sh`; commands run with the workload container's existing identity and
permissions. A service with multiple running replicas is pinned to one for the
life of a connection. Local TCP forwarding is limited to loopback addresses
inside that replica and require `nc`, `python3`, or `bash` in its image. SFTP and modern `scp` require an `sftp-server` executable
in the image. SSH agent and X11 forwarding are not enabled.

`flashSsh.enabled` in the chart requires one persistent Ed25519 host-key Secret
shared by API replicas. `flashSsh.tcp.enabled` optionally starts a direct TCP
listener. For a public host-networked listener set `flashSsh.tcp.listenAddress`
to `0.0.0.0`, configure a public DNS name in `flashSsh.tcp.advertisedHost`,
and allow its TCP port through the firewall. Deploy the per-service SSH gate
to every API replica before exposing the listener. Protect and back up the host key: replacing it changes the
fingerprint and requires clients to verify the replacement. SSH exec channels
are bounded to eight hours; infrastructure proxies must allow at least that
long for the dedicated WebSocket route.
