#!/usr/bin/env bash
set -euo pipefail

if ! command -v make &> /dev/null; then
  echo "make not found, installing..."
  if command -v apt-get &> /dev/null; then
    sudo apt-get install -y make
  elif command -v dnf &> /dev/null; then
    sudo dnf install -y make
  elif command -v yum &> /dev/null; then
    sudo yum install -y make
  elif command -v pacman &> /dev/null; then
    sudo pacman -S --noconfirm make
  elif command -v apk &> /dev/null; then
    sudo apk add make
  elif command -v brew &> /dev/null; then
    brew install make
  elif [ "$(uname)" = "Darwin" ]; then
    xcode-select --install
    echo "Xcode command line tools are installing. Re-run this script when done."
    exit 1
  else
    echo "Could not install make. Please install it manually and re-run this script."
    exit 1
  fi
fi

sudo make deps
make configure
make install
