# Vendored bids2nf oracle fixtures

Golden `*_unified.json` outputs copied from
https://github.com/agahkarakuzu/bids2nf (`tests/expected_outputs/`), MIT-licensed.

Regenerate with `scripts/vendor_bids2nf_fixtures.sh`. The oracle test
(`tests/oracle.rs`) reconstructs an in-memory dataset (`MemFs`) from the file
paths referenced in each golden JSON, runs the qmrust-bids resolver, and asserts
the produced unified JSON equals the golden — proving parity with bids2nf without
committing any imaging binaries.
