# Cross-engine canonical state inputs

Synthetic fixtures created by CPython SQLite 3.50.4 with foreign keys enabled and DELETE journal mode, closed without sidecars. They contain canonical migrations through 33, 55 or 60 only. No fork owner/binding DDL is applied. SQLx ledger rows use exact source-byte SHA384 and fixed synthetic timestamps; creation is an input operation, not a runtime conversion result.

| Input | SHA256 |
| --- | --- |
| state_canonical33_sqlite3504.sqlite | 09ea2a95555c00e4ba0a96ff31df2ae4dde1f01c66b949e5e173bdea9c04899d |
| state_canonical55_sqlite3504.sqlite | 0f634c7d15d18a1e9095f51265747fd5983d5ffb3060d1f8c2cddf8881ba0990 |
| state_canonical60_sqlite3504.sqlite | 9f7074b32300e5d87e109dcecf56a1d9c53607ff039747f74bda444b9ae81ad1 |

Sources 1-58 match 00b62fe3208d529fa663b6673c5fe6b1fd12d7de; canonical 59/60 match 39f427708738953ac01c40c3d9351236e8ea21c0 and are retained under migration_catalog. Independent checks cover 148 source/ledger identities. Rust qualification separately tests reading all three and extending 33 to 55 with its own SQLite engine. The existing legacy-56 fixture's last-writer header is 3.51.3; this is not complete writer provenance.

Creation artifacts are retained at C:/Users/devan/sc_dogfood/lsr_drivers/reports/stateful_b0_sqlite350_fixture_pack/: builder.py, manifest.json, full logical snapshots, verification.json and independent_verification.json. The sibling steward receipt stateful_b0_sqlite_engine_pin_receipt.json records source dependency pins. The external builder is not invoked by tests. BUILD.bazel's existing testdata glob includes these files.
