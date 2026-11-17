# rusty_dock — cross-platform build
#
# Slint compiles natively per target, so each target needs that Rust target
# installed (`rustup target add <triple>`) plus a linker for it. In practice:
#   - Run `make linux` / `make darwin` natively on that OS
#     (Linux: gcc + libfontconfig1-dev libxkbcommon-dev; macOS: Xcode command line tools).
#   - `make windows` works natively (MSVC) or from Linux/macOS via the
#     MinGW-w64 cross linker (see WINDOWS_TARGET below).
#   - Or use `make cross-linux`, which builds inside Docker via cross
#     (cargo install cross --git https://github.com/cross-rs/cross).
#
# Every target copies the release binary to the project root as RustyDock.exe
# (Windows) or RustyDock (Linux/macOS); Cargo itself builds rusty_dock, since
# rustc warns about crate names that aren't snake_case. Linux and macOS builds
# share the name `RustyDock`, so only the most recent one is kept.

APP     := rusty_dock
# Name of the copied binary.
BIN     := RustyDock
CARGO   ?= cargo

# Target architecture for linux/darwin/cross-linux builds; defaults to the
# host's. Accepts Rust names (x86_64, aarch64) or Go-style (amd64, arm64),
# e.g. `make darwin ARCH=x86_64` for an Intel Mac binary on Apple Silicon.
ifeq ($(OS),Windows_NT)
HOST_ARCH := $(if $(filter ARM64,$(PROCESSOR_ARCHITECTURE)),aarch64,x86_64)
else
HOST_ARCH := $(shell uname -m)
endif
ARCH      ?= $(HOST_ARCH)
RUST_ARCH := $(patsubst amd64,x86_64,$(patsubst arm64,aarch64,$(ARCH)))

# Strip symbols from release binaries (Go's -s -w).
export CARGO_PROFILE_RELEASE_STRIP := true

# Cross-compiling Windows from Linux/macOS uses the GNU target and MinGW-w64
# (Debian/Ubuntu: apt install gcc-mingw-w64-x86-64; Homebrew: brew install mingw-w64).
ifeq ($(OS),Windows_NT)
WINDOWS_TARGET ?= x86_64-pc-windows-msvc
EXE  := .exe
COPY = powershell -NoProfile -Command "Copy-Item -Force $(1) $(2)"
else
WINDOWS_TARGET ?= x86_64-pc-windows-gnu
EXE  :=
COPY = cp -f $(1) $(2)
endif

LINUX_TARGET  := $(RUST_ARCH)-unknown-linux-gnu
DARWIN_TARGET := $(RUST_ARCH)-apple-darwin

# Install the Rust std for a target if missing (no-op when already installed).
ADD_TARGET = rustup target add $(1)

.PHONY: all build run check test fmt clippy windows linux darwin cross-linux clean help

all: help

## build: release build for the host OS/arch -> RustyDock.exe or RustyDock
build:
	$(CARGO) build --release
	$(call COPY,target/release/$(APP)$(EXE),$(BIN)$(EXE))

## run: debug build and run
run:
	$(CARGO) run

## check / test / fmt / clippy: usual cargo checks
check:
	$(CARGO) check
test:
	$(CARGO) test
fmt:
	$(CARGO) fmt
clippy:
	$(CARGO) clippy -- -D warnings

## windows: Windows x86_64 GUI build (no console window in release) -> RustyDock.exe
windows:
	$(call ADD_TARGET,$(WINDOWS_TARGET))
	$(CARGO) build --release --target $(WINDOWS_TARGET)
	$(call COPY,target/$(WINDOWS_TARGET)/release/$(APP).exe,$(BIN).exe)

## linux: Linux build for $(ARCH) -> RustyDock
linux:
	$(call ADD_TARGET,$(LINUX_TARGET))
	$(CARGO) build --release --target $(LINUX_TARGET)
	$(call COPY,target/$(LINUX_TARGET)/release/$(APP),$(BIN))

## darwin: macOS build for $(ARCH) -> RustyDock
darwin:
	$(call ADD_TARGET,$(DARWIN_TARGET))
	$(CARGO) build --release --target $(DARWIN_TARGET)
	$(call COPY,target/$(DARWIN_TARGET)/release/$(APP),$(BIN))

## cross-linux: Linux $(ARCH) via cross (Docker) -> RustyDock
cross-linux:
	cross build --release --target $(LINUX_TARGET)
	$(call COPY,target/$(LINUX_TARGET)/release/$(APP),$(BIN))

## clean: remove build outputs
clean:
	$(CARGO) clean
ifeq ($(OS),Windows_NT)
	powershell -NoProfile -Command "Remove-Item -Force -ErrorAction SilentlyContinue $(BIN).exe, $(BIN)"
else
	rm -f $(BIN).exe $(BIN)
endif

## help: list targets
help:
	@echo Targets: build run check test fmt clippy windows linux darwin cross-linux clean  (optional ARCH=x86_64 or ARCH=aarch64)
