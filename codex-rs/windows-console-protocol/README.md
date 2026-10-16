# Inert console request protocol

`encode_request(&request, &receiving_worker)` creates a version1 closed frame;
`decode_request(&frame, &receiving_worker)` checks it against explicit nonce,
worker width and bootstrap control references, without authenticating/owning handles.
Lone UTF-16 surrogates and environments above32767 units round-trip within the
65536-unit budget. NUL paths, mismatched nonce/roles/width, malformed/unknown/
duplicate JSON and incomplete/oversize frames refuse. No launcher uses this crate;
stream/response/lifecycle/OS admission/native repair and larger environments stay gated.
