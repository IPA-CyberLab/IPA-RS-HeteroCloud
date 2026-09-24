---
name: heterocloud-cli-setup
description: Install or update the HeteroCloud CLI and complete browser sign-in on Linux, macOS, or Windows. Use for HeteroCloud CLI installation or initial setup requests, not for deploying HeteroCloud services.
---

# HeteroCloud CLI setup

Complete the installation on the user's machine and verify an authenticated CLI
session. Use the endpoint origin and organization UUID supplied by the user or
their HeteroCloud console. HeteroCloud may be hosted at any domain; never assume
a particular production hostname. If either value is missing, continue the
installation and ask for only the missing value before login.

1. Detect the OS and native CPU architecture. Releases support Linux, macOS,
   and Windows on x64 and ARM64. Get the current stable release from
   `https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud/releases/latest`. Select
   the matching `heterocloud-v<version>-<os>-<arch>.tar.gz` archive, or `.zip`
   on Windows, and its adjacent `.sha256` file. Verify the archive checksum
   with the OS's SHA-256 tool before extracting or executing it. Stop on a
   mismatch or unsupported platform.
2. Install `heterocloud` (`heterocloud.exe` on Windows) into a user-writable
   directory on `PATH` when possible, and ensure future shells can find it.
   Use normal OS privilege elevation only when the chosen installation
   location requires it. Run `heterocloud --version` and confirm it reports
   the downloaded release.
3. For first-time setup, or when the existing login is invalid or belongs to
   another endpoint or organization, run
   `heterocloud --endpoint <origin> --organization-id <uuid> auth login`. For an
   explicitly supplied HTTP origin, add `--allow-insecure-http`. In a headless
   session, use `auth login --no-browser` and let the user open the displayed
   URL. The user completes the configured identity-provider login and approves
   the CLI in the browser. A successful login saves the active endpoint and
   organization, so persistent shell variables are optional.
4. After the CLI exits successfully, run `heterocloud auth status`. Check that
   it returns the expected organization, user, endpoint, and token expiry. If
   browser approval expires or status fails, fix the identified cause and
   retry once. If it still fails, report the blocker; do not report setup
   complete before status succeeds.

Do not request or generate an API key for interactive login. Do not repeat the
device URL, verification code, access token, or credential-file contents in
chat, shell history, or logs. Do not create, change, or delete cloud resources
as part of CLI setup. Report the installed binary path, version, endpoint,
organization, and authentication status without revealing credentials.
