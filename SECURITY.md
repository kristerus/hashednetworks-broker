# Security Policy

## Reporting a vulnerability

Email **security@hashednetworks.network** with:

* A description of the issue and its impact.
* Reproduction steps or a proof-of-concept.
* Affected versions (broker tag / commit SHA).
* Your name and how you'd like to be credited (or "anonymous").

PGP key: **TBD** — we will publish a key at
<https://hashednetworks.network/.well-known/pgp-key.asc> once the inbox
is provisioned. Until then, plain email is acceptable; avoid attaching
proof-of-concept payloads or sensitive material.

**Do not** file a public GitHub issue for vulnerabilities.

## Scope

In scope:

* Protocol-level vulnerabilities — replay, downgrade, signature
  bypass, message-injection, malformed-input panics.
* Cryptographic flaws in the broker's verification path
  (`auth::verify_signed_message` and friends).
* Identity bypass — anything that lets a peer act as another's pubkey.
* Rate-limit bypass that enables abuse beyond the documented limits.
* Information disclosure of peer metadata beyond what the protocol
  intends to publish.

## Out of scope

* Distributed-denial-of-service against a single broker instance —
  HashedNetworks v0.5 is a single coordination broker behind Fly.io; DDoS
  resistance comes from your edge provider, not the broker.
* Social engineering of HashedNetworks operators or users.
* Physical attacks on the deployment host.
* Vulnerabilities in upstream Rust crates that don't materially affect
  the broker's threat surface (please report those upstream).
* "I can run a malicious broker" — by design, peers verify each other's
  Ed25519 signatures end-to-end; the broker is a relay, not a TA.

## Response timeline

* **48 hours** — initial acknowledgement of your report.
* **7 days** — status update with our preliminary assessment.
* **Coordinated** — public disclosure timing agreed jointly with you,
  typically once a fix is shipped and a reasonable upgrade window has
  elapsed.

We follow [RFC 9116](https://www.rfc-editor.org/rfc/rfc9116) for
machine-readable security contact info (see `/.well-known/security.txt`
on the deployed broker once that's wired up).

## Hall of fame

Researchers who have responsibly disclosed will be credited here with
their consent. Currently empty — be the first.
