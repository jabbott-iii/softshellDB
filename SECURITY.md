# Security Policy

## Supported versions

softshellDB is in its design phase; there is no release yet. Security fixes
land on `main` and reach users in the next release. There are no
maintenance branches before 1.0 (`docs/05-release.md`).

| Version | Supported |
| :- | :- |
| `main` | :white_check_mark: |
| The newest release, once there is one | :white_check_mark: |
| Earlier commits and releases | :x: |

## Reporting a vulnerability

Report vulnerabilities privately, through GitHub's private vulnerability
reporting:

1. On the repository's **Security** tab, select **Report a
   vulnerability**, or go straight to
   <https://github.com/jabbott-iii/softshellDB/security/advisories/new>.
2. Fill in the form. Only you, the maintainer, and the collaborators the
   maintainer adds to the report can see it.

Do not report a vulnerability in a public issue, discussion, pull request
or commit message.

Include as much of this as you can:

- The commit (`git rev-parse HEAD`) or release.
- The OS and its version, the CPU architecture and the filesystem.
- The input that triggers it: attach a small database directory, file,
  request or query to the private report rather than linking to it.
- The steps to reproduce it, or a proof of concept. A failing test, or a
  finding from `cargo xtask fuzz-smoke` or `cargo xtask crash`, is ideal.
- The impact: what an attacker has to control, and what they gain.
- A suggested fix, if you have one.

Remove tokens, user names, paths and other private data from anything you
attach.

## What to expect

softshellDB has a single maintainer, so responses are best effort. A report
goes through these steps:

1. **Acknowledgement** in the private report.
2. **Assessment:** whether it is a vulnerability, which commits it affects,
   and how severe it is.
3. **Fix:** prepared privately when needed, then landed on `main` and
   released as a patch.
4. **Disclosure:** a GitHub security advisory, crediting you unless you
   would rather not be, and a CVE when warranted.

If a report is declined, for example as out of scope, the maintainer
explains why. Please keep the details private until the advisory is
published, or until you and the maintainer agree on a date.

## Scope

softshellDB treats every file it reads and every byte a client sends as
possibly malformed or hostile. In scope:

- **Database files:** opening or reading a crafted or corrupted database
  directory (WAL, SST, manifest, `CURRENT`, `IDENTITY`, catalog) causes a
  panic, a hang, undefined behavior, memory use past the configured limits,
  a wrong answer reported as success, or a write outside the database
  directory.
- **Data loss:** a commit acknowledged as durable is lost or partially
  applied after a crash or power loss on a supported OS and filesystem.
- **Decoders:** SDOC, JSON and JSONC, keycode, SLZ, series chunks, query
  documents and the server configuration: a panic, hang, stack overflow or
  allocation past `Limits` on any input.
- **The server:** any request processed before authentication; a token
  role bypass; a way to make the server bind a non-loopback address
  without TLS; a client exhausting memory, threads or the writer lock past
  the configured limits; tokens appearing in logs or errors.
- **`unsafe` code** in `softshell-sys` and `softshell-platform`: undefined
  behavior reachable from safe code.
- **Build automation and CI:** `xtask` reaching the network or a secret; a
  workflow that exposes secrets to pull requests.

Out of scope:

- Databases on network filesystems, or on filesystems the docs do not
  support (`docs/03-platforms.md`).
- Reading or changing data by someone who can already read or write the
  database directory: filesystem permissions protect it, and encryption at
  rest is in the backlog. (A crafted directory that makes softshellDB
  misbehave is in scope, above.)
- Denial of service by an authenticated `admin` token.
- Missing TLS, until roadmap task M11.7: the server refuses non-loopback
  addresses by design.
