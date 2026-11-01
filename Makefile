.PHONY: setup dev build test check smoke
setup:
	cd web && npm ci

dev:
	sh scripts/dev.sh

build: setup
	cd web && npm run build
	SMARTTHERM_REQUIRE_WEB=1 cargo build --release --locked

test:
	cargo test --locked
	cd web && npm test

check:
	cargo fmt --check
	cargo clippy --locked --all-targets -- -D warnings
	cd web && npm run check && npm run format:check

smoke: build
	python3 scripts/smoke.py
