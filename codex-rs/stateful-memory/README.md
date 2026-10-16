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

## Bounded attributed content

After both compiled project-intelligence and stateful-runtime domains have been
installed by the admitted project-store installer, a foreign engine can read:

```sh
stateful-memory --project ./project --json read --project-uuid <project-uuid> --store-uuid <store-uuid> --entry <entry-id>
```

Read is auth-free and readonly. Both UUIDs must be canonical; IDs fit512 UTF-8
bytes and content fits2048. Responses explicitly identify attributed historical
content, verification notEvaluated and omitted support/applicability. Missing is
uniform for absent/ineligible entries; incomplete coverage refuses separately.
Clocks are decimal strings. Escaped ASCII-safe read envelopes include their
newline within8192 bytes; overflow emits one responseTooLarge error. Metadata
operations retain4096. Argument preflight permits32 OS arguments including the
program name, charging UTF-8 bytes plus64 per argument against16KiB, with invalid
Unicode rejected before clap. init still installs bootstrap metadata only.
Native routing, home migration, source/root/MCP/writes and customer qualification
remain separate gates. Direct SQL writes are unsupported.
