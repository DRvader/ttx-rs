test:
	cargo test --workspace -- --test-threads 1

test_log:
	RUST_LOG=trace cargo test --workspace -- --test-threads 1
