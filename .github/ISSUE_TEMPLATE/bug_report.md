---
name: Bug report
about: Something is broken in the broker
title: "[bug] "
labels: ["bug", "needs-triage"]
---

**Note:** if this is a security issue, *do not* file it here. See
[SECURITY.md](../../SECURITY.md).

## What happened

A clear, concise description of what went wrong.

## What should have happened

What did you expect to happen instead?

## Reproduction

Minimum steps to reproduce. Include configuration, commands run, and any
relevant env vars (with secrets redacted).

```text
$ docker compose up -d
$ ...
```

## Environment

- Broker version / git SHA:
- Deployment: ( ) local docker-compose ( ) Fly.io ( ) other:
- OS:
- Postgres version:

## Logs

Relevant excerpts from `docker compose logs broker` (or `fly logs`).
Please redact peer pubkeys and signatures.

```text
```
