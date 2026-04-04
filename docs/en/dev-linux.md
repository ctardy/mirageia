# Linux Development Environment — MirageIA

Guide to set up a Linux dev machine (native or WSL) for building, testing and running MirageIA locally.

---

## System prerequisites

```bash
# Debian / Ubuntu
sudo apt update && sudo apt install -y build-essential pkg-config libssl-dev git curl

# Fedora
sudo dnf groupinstall -y "Development Tools" && sudo dnf install -y openssl-devel git curl

# Arch
sudo pacman -Syu --noconfirm base-devel openssl git curl
```

---

## 1. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# Verify
rustc --version    # 1.75+ required
cargo --version
```

Add to `~/.bashrc` or `~/.zshrc` if not already present:

```bash
. "$HOME/.cargo/env"
```

---

## 2. Clone the repository

```bash
git clone https://github.com/ctardy/mirageia.git
cd mirageia
```

---

## 3. Build

```bash
# Dev build (fast, unoptimized)
cargo build

# Release build (optimized)
cargo build --release
```

Binary location:
- `target/debug/mirageia` (dev)
- `target/release/mirageia` (release)

---

## 4. Run tests

```bash
# All tests (unit + e2e)
cargo test

# Unit tests only
cargo test --lib

# E2E only
cargo test --test e2e_proxy

# Specific module
cargo test -- detection::regex

# Specific test
cargo test -- test_detect_email

# Verbose output
cargo test -- --nocapture
```

---

## 5. Pre-commit checks (mirrors CI)

The CI runs two jobs: clippy and tests. To reproduce locally:

```bash
# 1. Clippy (strict linter, -D warnings = error on any warning)
rustup component add clippy   # once
cargo clippy -- -D warnings

# 2. Tests
cargo test

# 3. Quick check (no linking, faster than build)
cargo check
```

**Common clippy patterns to avoid:**

| Pattern | Clippy error | Fix |
|---------|-------------|-----|
| `x >= 200 && x < 300` | `manual_range_contains` | `(200..300).contains(&x)` |
| `v.sort_by(\|a,b\| b.len().cmp(&a.len()))` | `unnecessary_sort_by` | `v.sort_by_key(\|b\| Reverse(b.len()))` |
| `let _ = x;` on a Result | `let_underscore_drop` | explicit `drop(x);` |

---

## 6. Run the proxy locally

```bash
# Normal mode
cargo run

# With debug logs
MIRAGEIA_LOG_LEVEL=debug cargo run

# Passthrough mode (no pseudonymization)
cargo run -- proxy --passthrough

# With a command (e.g. test curl)
cargo run -- wrap -- curl -s http://localhost:3100/health
```

---

## 7. Useful daily commands

```bash
# Format code (optional, not in CI yet)
cargo fmt

# Check outdated dependencies
cargo install cargo-outdated   # once
cargo outdated

# Clean build cache (if weird issues)
cargo clean

# Full rebuild
cargo clean && cargo build
```

---

## 8. Project structure

```
mirageia/
├── src/
│   ├── main.rs                    # CLI (clap): proxy, console, setup, stop, update, wrap
│   ├── config/settings.rs         # AppConfig, config.toml + env loading
│   ├── detection/
│   │   ├── regex_detector.rs      # PII detection via regex
│   │   ├── types.rs               # PiiType enum + PiiEntity
│   │   ├── postprocess.rs         # Entity post-processing
│   │   └── mod.rs                 # Re-exports
│   ├── mapping/
│   │   ├── table.rs               # MappingTable (bidirectional, in-memory)
│   │   └── crypto.rs              # AES-256-GCM encryption
│   ├── pseudonymization/
│   │   ├── generator.rs           # Realistic pseudonym generation
│   │   ├── replacer.rs            # PII -> pseudonym replacement
│   │   ├── depseudonymizer.rs     # Pseudonym -> original replacement
│   │   ├── fragment_restorer.rs   # Fragment restoration in responses
│   │   └── dictionaries.rs        # Name/firstname dictionaries
│   ├── proxy/
│   │   ├── server.rs              # Axum handler, ProxyEvent, graceful shutdown
│   │   ├── client.rs              # Upstream HTTP client (reqwest)
│   │   ├── router.rs              # Provider routing (Anthropic, OpenAI)
│   │   ├── extractor.rs           # JSON text field extraction
│   │   └── error.rs               # Proxy error types
│   └── streaming/
│       ├── sse_parser.rs          # SSE parser (Anthropic + OpenAI)
│       └── buffer.rs              # Streaming de-pseudonymization buffer
├── tests/
│   └── e2e_proxy.rs               # Integration tests (mock upstream + proxy)
├── docs/                          # FR/EN documentation
├── .github/workflows/
│   ├── ci.yml                     # CI: clippy + tests (Linux + Windows)
│   └── release.yml                # Release: build + upload binaries
├── Cargo.toml                     # Dependencies and metadata
└── CLAUDE.md                      # Claude Code instructions
```

---

## 9. Recommended dev workflow

```bash
# 1. Create a branch
git checkout -b feat/my-feature

# 2. Code...

# 3. Verify before committing
cargo clippy -- -D warnings && cargo test

# 4. Commit
git add <files>
git commit -m "feat: description"

# 5. Push
git push -u origin feat/my-feature
```

---

## 10. WSL (Windows Subsystem for Linux)

If developing on Windows with WSL:

```powershell
# From PowerShell (admin)
wsl --install -d Ubuntu
```

Then in the WSL terminal, follow this guide from step 1.

Source code can be:
- **Inside WSL** (`/home/user/mirageia`): best compilation performance
- **On mounted Windows** (`/mnt/c/dev/projects/mirageia`): slower to compile but accessible from both sides

Recommendation: clone inside WSL for compilation, use VS Code with the "Remote - WSL" extension for editing.

---

## Troubleshooting

### `linker 'cc' not found`

```bash
sudo apt install build-essential
```

### `failed to run custom build command for openssl-sys`

```bash
sudo apt install pkg-config libssl-dev
```

### E2E tests timeout

E2E tests spawn servers on random ports. If a firewall blocks localhost connections:

```bash
# Verify localhost works
curl http://127.0.0.1:3100 2>&1 | head -1
# Should return "connection refused" (not "timeout")
```

### `cargo test` is slow

```bash
# First build is slow (compiles all deps), subsequent builds are incremental
# To speed up, use the mold linker:
sudo apt install mold
```

Add to `~/.cargo/config.toml`:

```toml
[target.x86_64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
```
