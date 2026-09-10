#!/usr/bin/env bash
# -----------------------------------------------------------------------------
# git-tui installer for macOS, Linux and Windows (Git Bash).
#
#   Install the latest release:
#     curl -fsSL https://raw.githubusercontent.com/vxtien-qa/git-tui/master/install.sh | bash
#
#   Build from source (installs Rust if it is missing):
#     ./install.sh --build
#
#   Windows PowerShell:
#     irm https://raw.githubusercontent.com/vxtien-qa/git-tui/master/install.ps1 | iex
# -----------------------------------------------------------------------------
set -euo pipefail

REPO="vxtien-qa/git-tui"
BIN_NAME="git-tui"
BUILD_MODE=false

for arg in "$@"; do
  case "$arg" in
    --build|-build) BUILD_MODE=true ;;
    --help|-h)
      sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
  esac
done

# --- Detect platform ---------------------------------------------------------
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux*)
    ASSET="git-tui-linux-x64"
    PLATFORM="Linux x64"
    INSTALL_DIR="/usr/local/bin"
    ;;
  Darwin*)
    case "$ARCH" in
      arm64|aarch64)
        ASSET="git-tui-darwin-arm64"
        PLATFORM="macOS (Apple silicon)"
        ;;
      x86_64)
        ASSET="git-tui-darwin-x64"
        PLATFORM="macOS (Intel)"
        ;;
      *)
        echo "Unsupported macOS architecture: $ARCH" >&2
        exit 1
        ;;
    esac
    INSTALL_DIR="/usr/local/bin"
    ;;
  MINGW*|MSYS*|CYGWIN*|Windows_NT*)
    ASSET="git-tui-windows-x64.exe"
    PLATFORM="Windows x64"
    BIN_NAME="git-tui.exe"
    INSTALL_DIR="${LOCALAPPDATA:-$HOME/AppData/Local}/git-tui"
    ;;
  *)
    echo "Unsupported operating system: $OS" >&2
    echo "Download a binary from https://github.com/$REPO/releases" >&2
    exit 1
    ;;
esac

echo
echo "git-tui installer"
echo "  Platform: $PLATFORM"
echo

# --- Install a binary to its destination -------------------------------------
install_binary() {
  local src="$1"
  mkdir -p "$INSTALL_DIR"

  case "$OS" in
    MINGW*|MSYS*|CYGWIN*|Windows_NT*)
      mv "$src" "$INSTALL_DIR/$BIN_NAME"
      echo "Installed to $INSTALL_DIR/$BIN_NAME"
      if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
        echo
        echo "Add this directory to your PATH:"
        echo "  $INSTALL_DIR"
        echo
        echo "Or run this once in PowerShell:"
        echo "  [Environment]::SetEnvironmentVariable('Path', \$env:Path + ';$INSTALL_DIR', 'User')"
      fi
      ;;
    *)
      chmod +x "$src"
      echo "Installing to $INSTALL_DIR/$BIN_NAME"
      if [ -w "$INSTALL_DIR" ]; then
        mv "$src" "$INSTALL_DIR/$BIN_NAME"
      else
        echo "  (requires sudo)"
        sudo mv "$src" "$INSTALL_DIR/$BIN_NAME"
      fi
      ;;
  esac
}

if [ "$BUILD_MODE" = true ]; then
  # --- Build from source -----------------------------------------------------
  echo "Mode: build from source"
  echo

  case "$OS" in
    MINGW*|MSYS*|CYGWIN*|Windows_NT*)
      echo "On Windows, PowerShell is recommended instead:"
      echo
      echo "  .\\install.ps1 -Build"
      echo
      echo "PowerShell can install the Visual Studio Build Tools and Rust for you;"
      echo "Git Bash cannot install the Build Tools."
      echo
      read -r -p "Continue anyway? (y/N) " confirm
      if [ "$confirm" != "y" ] && [ "$confirm" != "Y" ]; then
        echo "Aborted. Run in PowerShell: .\\install.ps1 -Build"
        exit 0
      fi
      echo
      ;;
  esac

  echo "[1/2] Checking Rust"
  if command -v rustc &>/dev/null; then
    echo "      $(rustc --version)"
    if command -v rustup &>/dev/null; then
      rustup update stable 2>&1 | tail -1 || true
    fi
  else
    echo "      Rust not found, installing via rustup"
    if ! command -v curl &>/dev/null; then
      echo "      curl not found. Install Rust manually: https://rustup.rs" >&2
      exit 1
    fi
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env" 2>/dev/null || export PATH="$HOME/.cargo/bin:$PATH"
    if ! command -v rustc &>/dev/null; then
      echo "      Rust install failed. See https://rustup.rs" >&2
      exit 1
    fi
    echo "      Rust installed: $(rustc --version)"
  fi

  echo "[2/2] Building git-tui in release mode"
  echo "      The first build takes a few minutes."
  echo
  cargo build --release

  case "$OS" in
    MINGW*|MSYS*|CYGWIN*|Windows_NT*) BUILT_BIN="target/release/git-tui.exe" ;;
    *) BUILT_BIN="target/release/git-tui" ;;
  esac

  if [ ! -f "$BUILT_BIN" ]; then
    echo "Build output not found: $BUILT_BIN" >&2
    exit 1
  fi
  install_binary "$BUILT_BIN"
else
  # --- Install the latest release --------------------------------------------
  TMP_FILE="$(mktemp)"
  URL="https://github.com/$REPO/releases/latest/download/$ASSET"

  echo "Downloading $ASSET"
  echo "  $URL"

  # Plain HTTPS first, so installing needs no GitHub credentials. The GitHub
  # CLI is only a fallback, for example while the repository is still private.
  if ! curl -fsSL "$URL" -o "$TMP_FILE"; then
    if command -v gh &>/dev/null && gh auth status &>/dev/null; then
      echo "  Direct download failed, retrying with the GitHub CLI"
      if ! gh release download --repo "$REPO" --pattern "$ASSET" -O "$TMP_FILE" --clobber; then
        echo "Download failed. See https://github.com/$REPO/releases" >&2
        rm -f "$TMP_FILE"
        exit 1
      fi
    else
      echo "Download failed. See https://github.com/$REPO/releases" >&2
      rm -f "$TMP_FILE"
      exit 1
    fi
  fi

  install_binary "$TMP_FILE"
fi

echo
echo "git-tui installed."
echo
echo "Run: git-tui"
echo
echo "git-tui drives the GitHub CLI, so it needs gh installed and authorised:"
echo "  1. Install gh: https://cli.github.com/"
echo "  2. Sign in:    gh auth login -s repo,project,read:org"
echo
