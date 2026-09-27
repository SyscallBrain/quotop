# Security policy

quotop reads API keys, so security reports are taken seriously.

## Reporting a vulnerability

Please **do not open a public issue** for security problems. Instead, report
them privately through GitHub:
[**Report a vulnerability**](https://github.com/SyscallBrain/quotop/security/advisories/new)
(the *Security* tab of the repository → *Report a vulnerability*).

Useful things to include:

- what the problem is and its impact (for example, a key appearing on screen,
  in the cache, in `--json` output or in an error message);
- the steps or configuration to reproduce it;
- the quotop version (`quotop --version`) and your operating system.

**Never include real API keys** in a report — use fake values.

You can expect a first reply within a week. Once the problem is confirmed, a fix
is prepared and released, and you are credited in the release notes unless you
prefer otherwise.

## Supported versions

Only the latest release receives security fixes.

## Scope

In scope: anything that could expose a key (screen, files, logs, output, error
messages), writing to files quotop should not touch, spending a user's quota
without being asked, or making requests to hosts other than the services'
official APIs.

Out of scope: problems in the third-party services themselves, and issues that
require an attacker who already controls your user account or your terminal.
