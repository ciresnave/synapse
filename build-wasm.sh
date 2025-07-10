#!/bin/bash

# Synapse WebAssembly Build Script
# This script builds the Synapse library for WebAssembly targets

set -e

echo "🧠 Building Synapse for WebAssembly..."

# Install wasm-pack if not available
if ! command -v wasm-pack &> /dev/null; then
    echo "Installing wasm-pack..."
    curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
fi

# Install required targets
echo "Installing WebAssembly targets..."
rustup target add wasm32-unknown-unknown
rustup target add wasm32-wasi

# Build for web browsers
echo "Building for web browsers..."
wasm-pack build --target web --out-dir pkg-web -- --no-default-features --features wasm

# Build for Node.js
echo "Building for Node.js..."
wasm-pack build --target nodejs --out-dir pkg-node -- --no-default-features --features wasm

# Build for bundlers (webpack, etc.)
echo "Building for bundlers..."
wasm-pack build --target bundler --out-dir pkg-bundler -- --no-default-features --features wasm

# Build for no-modules (direct script include)
echo "Building for no-modules..."
wasm-pack build --target no-modules --out-dir pkg-no-modules -- --no-default-features --features wasm

echo "✅ WebAssembly builds complete!"
echo ""
echo "📦 Output directories:"
echo "  - pkg-web/       - For web browsers with ES modules"
echo "  - pkg-node/      - For Node.js applications"
echo "  - pkg-bundler/   - For webpack/rollup bundlers"
echo "  - pkg-no-modules/ - For direct script inclusion"
echo ""
echo "🔗 JavaScript bindings and TypeScript definitions are included in each package."
echo ""
echo "📘 Usage examples:"
echo "  Web: import init, { BrowserSynapseNode } from './pkg-web/synapse.js';"
echo "  Node: const synapse = require('./pkg-node/synapse.js');"
echo ""
echo "🚀 Ready to deploy Synapse in web browsers!"
