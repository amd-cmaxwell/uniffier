SHELL := /bin/bash
.PHONY: deps rustup configure install


deps:
	@if command -v apt-get &> /dev/null; then \
		apt-get install -y build-essential libssl-dev curl; \
	elif command -v dnf &> /dev/null; then \
		dnf install -y gcc make openssl-devel curl; \
	elif command -v yum &> /dev/null; then \
		yum install -y gcc make openssl-devel curl; \
	elif command -v pacman &> /dev/null; then \
		pacman -S --needed base-devel openssl curl; \
	elif command -v apk &> /dev/null; then \
		apk add build-base openssl-dev curl; \
	elif command -v brew &> /dev/null; then \
		brew install openssl curl; \
	else \
		echo "Unsupported package manager. Please install a C compiler and OpenSSL development headers manually."; \
		exit 1; \
	fi

rustup:
	@if command -v rustup &> /dev/null; then \
		echo "rustup is already installed."; \
	elif ! command -v curl &> /dev/null; then \
		echo "Error: curl is required to install rustup but was not found."; \
		echo "Run 'sudo make deps' to install it."; \
		exit 1; \
	else \
		curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y; \
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