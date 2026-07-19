# Oracle fixtures

Golden `*_unified.json` outputs, sourced from the bids2nf project
(https://github.com/agahkarakuzu/bids2nf, `tests/expected_outputs/`), MIT-licensed.

Regenerate with `scripts/vendor_reference_fixtures.sh`. The oracle test
(`tests/oracle.rs`) reconstructs an in-memory dataset (`MemFs`) from the file
paths referenced in each golden JSON, runs the rust-bids resolver, and asserts
the produced unified JSON equals the golden — validating grouping, ordering, and
suffix-filtering against the reference golden outputs for the vendored
collections, without committing any imaging binaries. Because the input
dataset is reconstructed from the golden's own referenced paths, this does not
prove full parity with the reference implementation (e.g. it can't test
over-inclusion or under-inclusion of files relative to a real dataset
directory).
