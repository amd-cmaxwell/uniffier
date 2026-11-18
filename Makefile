SHELL := /bin/bash
.PHONY: rustup deps configure install

rustup:
	@if command -v rustup &> /dev/null; then \
		echo "rustup is already installed."; \
	else \
		curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y; \
	fi

deps:
	@if command -v apt-get &> /dev/null; then \
		apt-get install -y build-essential libssl-dev; \
	elif command -v dnf &> /dev/null; then \
		dnf install -y gcc make openssl-devel; \
	elif command -v yum &> /dev/null; then \
		yum install -y gcc make openssl-devel; \
	elif command -v pacman &> /dev/null; then \
		pacman -S --needed base-devel openssl; \
	elif command -v apk &> /dev/null; then \
		apk add build-base openssl-dev; \
	elif command -v brew &> /dev/null; then \
		brew install openssl; \
	else \
		echo "Unsupported package manager. Please install a C compiler and OpenSSL development headers manually."; \
		exit 1; \
	fi

configure: rustup
	@. "$$HOME/.cargo/env" 2>/dev/null; rustup default stable

install:
	@cargo install --path .
	@if ! echo "$$PATH" | grep "$$HOME/.cargo/bin" &> /dev/null; then \
		if [ -n "$$ZSH_VERSION" ] || [ "$$SHELL" = "/bin/zsh" ]; then \
			rcfile=~/.zshrc; \
		else \
			rcfile=~/.bashrc; \
		fi; \
		printf '\nThe CLI has been installed.\n\nRun the following command to add it to your PATH:\n\n\techo '\''PATH=$$HOME/.cargo/bin:$$PATH'\'' >> %s && . %s\n\n' "$$rcfile" "$$rcfile"; \
	fi