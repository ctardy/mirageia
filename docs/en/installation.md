# Installation Guide — MirageIA

## Quick Install

```bash
# 1. Install MirageIA (from source or prebuilt binary)
cargo install --path .

# 2. Run the setup wizard
mirageia setup

# 3. Start the proxy
mirageia
```

The `mirageia setup` wizard guides you step by step: port selection, LLM provider choice, whitelist, automatic shell configuration. See the [Guided Configuration](#guided-configuration) section below.

---

## Prerequisites

| Tool | Version | Required | Notes |
|---|---|---|---|
| **Rust** | 1.75+ | Yes | Installed via [rustup](https://rustup.rs/) |
| **GCC** (Windows) | 15+ | Yes (Windows GNU) | Via MSYS2 (`mingw-w64-x86_64-gcc`) |
| **Git** | 2.x | Yes | To clone the repository |

### Supported Systems

| OS | Toolchain | Status |
|---|---|---|
| Windows 11 | `stable-x86_64-pc-windows-gnu` + MSYS2 | ✅ Tested |
| Windows 11 | `stable-x86_64-pc-windows-msvc` | ⚠️ Requires full VS Build Tools |
| macOS | `stable-aarch64-apple-darwin` | Not tested |
| Linux | `stable-x86_64-unknown-linux-gnu` | Not tested |

---

## Step-by-Step Installation (Windows)

### 1. Install Rust

```bash
# From a terminal (Git Bash, PowerShell, etc.)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

# Verify the installation
rustc --version    # rustc 1.94.1 or newer
cargo --version
```

### 2. Install MSYS2 and GCC

MirageIA uses the GNU toolchain on Windows. You need to install GCC via MSYS2.

```bash
# Install MSYS2 via winget
winget install -e --id MSYS2.MSYS2

# Install GCC in MSYS2
/c/msys64/usr/bin/bash.exe -lc "pacman -S --noconfirm mingw-w64-x86_64-gcc"
```

### 3. Configure the Rust Toolchain

```bash
rustup default stable-x86_64-pc-windows-gnu
```

### 4. Configure the PATH

Add these paths to your PATH (in `.bashrc` or `.bash_profile` for Git Bash):

```bash
export PATH="/c/msys64/mingw64/bin:$HOME/.cargo/bin:$PATH"
```

Verification:

```bash
gcc --version     # gcc.exe (Rev8, Built by MSYS2 project) 15.x
cargo --version   # cargo 1.94.x
```

### 5. Clone and Build

```bash
git clone <repo-url>
cd mirageia

# Development build
cargo build

# Release build (optimized)
cargo build --release
```

The binary is located at `target/release/mirageia.exe` (or `target/debug/mirageia.exe`).

### 6. Verify Everything Works

```bash
# Run the tests
cargo test

# Expected result: 144 tests pass, 0 failures
```

---

## Guided Configuration

### The `mirageia setup` Wizard

Instead of configuring manually, run the interactive wizard:

```bash
mirageia setup
```

The wizard guides you through 6 steps:

| Step | Question | What Happens |
|---|---|---|
| 1 | — (automatic) | Detects the OS (Windows/macOS/Linux) and shell (bash/zsh/PowerShell) |
| 2 | Listening port? | Default `3100`, configurable |
| 3 | Which LLM providers? | Multi-select: Anthropic, OpenAI, Gemini, Mistral. Auto-detects already configured API keys |
| 4 | Whitelist? | Terms to never pseudonymize (optional) |
| 5 | — (automatic) | Generates `~/.mirageia/config.toml` |
| 6 | Configure shell? | Offers to add `export` statements to `.bashrc` / `.zshrc` |

Example session:

```
  Detected system: Windows (Git Bash)

? Proxy listening port [3100]: 3100

? Which LLM providers do you use?
  >[x] Anthropic (Claude) ✓ API key detected
   [ ] OpenAI (GPT)
   [ ] Google Gemini
   [ ] Mistral AI

? Add terms to never pseudonymize? [y/N]: y
  Whitelist: Thomas Edison, Martin Fowler

  ✓ Configuration written to ~/.mirageia/config.toml

? Automatically add to ~/.bashrc? [Y/n]: Y
  ✓ ~/.bashrc updated

  Setup complete!
    Proxy     : http://127.0.0.1:3100
    Providers : Anthropic (Claude)
    Shell     : ✓ configured

  To start: mirageia
```

---

## First Launch

```bash
mirageia
```

What happens:
1. Loads `~/.mirageia/config.toml` (created by `setup` or manually)
2. Starts the proxy on the configured port
3. Displays in the terminal:

```
INFO  MirageIA v0.1.0
INFO  MirageIA proxy listening on 127.0.0.1:3100
```

If this is the first use without having run `setup`:
```
First time? Run `mirageia setup` for guided configuration.
```

The proxy starts anyway with defaults — setup is not mandatory.

### Per-Session Activation (Recommended)

Rather than configuring the proxy globally in your shell, use `mirageia wrap` to activate the proxy only for a given session:

```bash
# Terminal 1 — Start the proxy
mirageia

# Terminal 2 — Launch Claude Code through the proxy
mirageia wrap -- claude
```

`wrap` checks that the proxy is running, then launches the command with `ANTHROPIC_BASE_URL` and `OPENAI_BASE_URL` pointing to the proxy. When the command exits, the environment variables are gone.

Advantage: if the proxy is stopped, apps launched normally (`claude`) still work directly against the API.

### Real-Time Monitoring

```bash
# In a separate terminal
mirageia console
```

Displays each request passing through the proxy, with the number of detected PII:
```
  [14:32:01] → PII  Anthropic  /v1/messages (3 PII detected)
  [14:32:02] ← PII  Anthropic  /v1/messages
```

### Passthrough Mode (Temporary Disable)

To relay requests **without pseudonymization** (debugging, performance testing):

```bash
# Via CLI flag
mirageia proxy --passthrough

# Via environment variable
MIRAGEIA_PASSTHROUGH=1 mirageia

# Via config.toml
# [proxy]
# passthrough = true
```

The health check indicates the active mode:
```bash
curl http://localhost:3100/health
# → {"status":"ok","passthrough":true,"pii_mappings":0}
```

### Verification

```bash
# Health check
curl http://localhost:3100/health
# → {"status":"ok","passthrough":false,"pii_mappings":0}

# Test with PII (requires an API key)
curl -X POST http://localhost:3100/v1/messages \
  -H "x-api-key: $ANTHROPIC_API_KEY" \
  -H "content-type: application/json" \
  -H "anthropic-version: 2023-06-01" \
  -d '{
    "model": "claude-sonnet-4-20250514",
    "max_tokens": 100,
    "messages": [{"role":"user","content":"Mon email est jean@acme.fr et l IP est 192.168.1.50"}]
  }'
```

In the MirageIA logs:
```
INFO  PII detected in request pii_count=2
INFO  Request pseudonymized provider=Anthropic mappings=2
```

---

## Manual Configuration (Alternative to Setup)

If you prefer to configure manually:

```bash
mkdir -p ~/.mirageia
cp config.example.toml ~/.mirageia/config.toml
# Edit the file according to your needs
```

Then configure your shell:

```bash
# For Anthropic (Claude Code, SDK)
export ANTHROPIC_BASE_URL=http://localhost:3100

# For OpenAI
export OPENAI_BASE_URL=http://localhost:3100
```

MirageIA automatically routes:
- `/v1/messages` → Anthropic
- `/v1/chat/completions` → OpenAI

See the [main README](../../README.md) for the full list of options.

---

## Troubleshooting

### `cargo: command not found`

The Rust PATH is not configured. Add:
```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

### `gcc.exe: program not found` (Windows)

MSYS2/GCC is not in the PATH:
```bash
export PATH="/c/msys64/mingw64/bin:$PATH"
```

### `ort does not provide prebuilt binaries for x86_64-pc-windows-gnu`

The ONNX feature is not supported with the GNU toolchain. Two solutions:
1. Use the regex detector (default, no ONNX needed)
2. Switch to the MSVC toolchain (`rustup default stable-x86_64-pc-windows-msvc`) with full Visual Studio Build Tools

### `LINK : fatal error LNK1104: cannot open file 'msvcrt.lib'`

Visual Studio does not have the C++ desktop components installed. Install via the VS Installer:
- Component: "MSVC v143 - VS 2022 C++ x64/x86 build tools"
- Component: "Windows 11 SDK"

### Tests Are Failing

```bash
# Clean the build and retry
cargo clean
cargo test
```

---

## Uninstallation

```bash
# Remove the binary
cargo clean

# Remove the configuration
rm -rf ~/.mirageia

# Remove Rust (if desired)
rustup self uninstall
```
