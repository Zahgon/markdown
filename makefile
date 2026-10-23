# Python-Markdown makefile

.PHONY : help
help:
	@echo 'Usage: make <subcommand>'
	@echo ''
	@echo 'Subcommands:'
	@echo '    install       Install the markdown command locally'
	@echo '    build         Build the release binary'
	@echo '    test          Run all tests'
	@echo '    lint          Check formatting and run clippy'
	@echo '    clean         Clean up the build directory'

.PHONY : install
install:
	cargo install --path .

.PHONY : build
build:
	cargo build --release

.PHONY : test
test:
	cargo test --release

.PHONY : lint
lint:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

.PHONY : clean
clean:
	cargo clean
	rm -f test-output.html
	rm -rf tmp
