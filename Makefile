# Uses ERBetterLod's toolchain, so there's only one copy of it. Run `make toolchain` there first.
TOOLCHAIN    ?= $(HOME)/workdir/elden/ERBetterLod/.toolchain
export RUSTUP_HOME := $(TOOLCHAIN)/rustup
export CARGO_HOME  := $(TOOLCHAIN)/cargo
export PATH  := $(TOOLCHAIN)/mingw/usr/bin:$(PATH)
CARGO        := $(CARGO_HOME)/bin/cargo
DLL          := target/x86_64-pc-windows-gnu/release/AutoFertilizer.dll
# The original plugin's file, which this one replaces. Two copies must not run together.
ORIGINAL     := MHW Auto Fertilizer Plugin.dll
PLUGINS_DIR  ?= $(HOME)/.local/share/Steam/steamapps/common/Monster Hunter World/nativePC/plugins

.PHONY: all build test install uninstall clean

all: build

build:
	$(CARGO) build --release

test:
	$(CARGO) test --target x86_64-unknown-linux-gnu

# Doesn't overwrite a config you've already edited.
install: build
	@if [ -e "$(PLUGINS_DIR)/$(ORIGINAL)" ]; then mv "$(PLUGINS_DIR)/$(ORIGINAL)" "$(PLUGINS_DIR)/$(ORIGINAL).bak"; echo "Renamed the original plugin to $(ORIGINAL).bak"; fi
	cp $(DLL) "$(PLUGINS_DIR)/"
	cp -n AutoFertilizer-config.json "$(PLUGINS_DIR)/" || true

uninstall:
	rm -f "$(PLUGINS_DIR)/AutoFertilizer.dll" "$(PLUGINS_DIR)/AutoFertilizer-config.json" "$(PLUGINS_DIR)/AutoFertilizer.log"

clean:
	rm -rf target
