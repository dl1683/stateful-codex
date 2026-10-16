# Inert console request protocol

`encode_request(&request, &receiving_worker)` produces one version1 request frame;
`decode_request(&frame, &receiving_worker)` validates an exact closed frame.
`ReceiveContext` explicitly selects nonce, worker width
and bootstrap control references. Decoding does not authenticate or own handles.

UTF-16 units, including lone surrogates, round-trip unchanged. Environments above
32767 units are accepted within the65536-unit protocol budget. NUL-bearing path/
command buffers, nonce/role/width mismatches, malformed JSON, unknown/duplicate
fields, oversize or incomplete frames refuse with static errors. Larger existing
environments remain an activation compatibility gate. No launcher uses this crate;
stream reading, responses, lifecycle, OS custody/rights and native repair remain separate.
