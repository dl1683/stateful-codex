# Standalone project-store bootstrap

```text
stateful-memory --project <directory> capabilities --json
stateful-memory --project <directory> root --json
stateful-memory --project <directory> init --storage local-unsynchronized --json
```

The root operation inspects immutable kernel identity only. Explicit init creates
bootstrap metadata. It does not adopt unknown files, migrate legacy memory, expose
knowledge entries or attach native writers. Responses disclose bootstrapOnly coverage, unavailable semantic revisions
and no native attachment. It is not the complete S1c/company-brain milestone.

No Codex login, home or network service is required. Existing unknown/partial stores
refuse. Network, known synchronized and unsupported filesystems refuse. Metadata
initialization requires the operator's explicit local-unsynchronized assertion.
That assertion is a host precondition, not comprehensive sync-agent detection.
Reads create no stores. Init retries inspect the same published identity;
post-publication errors can leave an uncertain result. Inactive abandoned stages
are ignored. Actual killed-process and power-loss recovery remain unqualified.

Responses are lossless ASCII-escaped JSON, a UTF-8 subset, capped at 4096 bytes with
the envelope and trailing LF. --json selects compact output; default is indented
JSON. Supplementary Unicode uses JSON surrogate pairs. This interface encoding
does not repair HAL's separate exact-source-byte failure. Oversized replies return
a bounded error. Recall, source reads, foreign writes, retirement, stdio MCP and
native migration require their separately reviewed implementation stages.
