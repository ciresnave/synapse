@echo off
REM Synapse WebAssembly Build Script for Windows
REM This script builds the Synapse library for WebAssembly targets

echo 🧠 Building Synapse for WebAssembly...

REM Check if wasm-pack is installed
where wasm-pack >nul 2>nul
if %ERRORLEVEL% NEQ 0 (
    echo Installing wasm-pack...
    powershell -Command "Invoke-WebRequest -Uri https://rustwasm.github.io/wasm-pack/installer/init.ps1 -OutFile init.ps1; .\init.ps1"
    del init.ps1
)

REM Install required targets
echo Installing WebAssembly targets...
rustup target add wasm32-unknown-unknown
rustup target add wasm32-wasi

REM Build for web browsers
echo Building for web browsers...
wasm-pack build --target web --out-dir pkg-web -- --no-default-features --features wasm

REM Build for Node.js
echo Building for Node.js...
wasm-pack build --target nodejs --out-dir pkg-node -- --no-default-features --features wasm

REM Build for bundlers (webpack, etc.)
echo Building for bundlers...
wasm-pack build --target bundler --out-dir pkg-bundler -- --no-default-features --features wasm

REM Build for no-modules (direct script include)
echo Building for no-modules...
wasm-pack build --target no-modules --out-dir pkg-no-modules -- --no-default-features --features wasm

echo ✅ WebAssembly builds complete!
echo.
echo 📦 Output directories:
echo   - pkg-web\       - For web browsers with ES modules
echo   - pkg-node\      - For Node.js applications
echo   - pkg-bundler\   - For webpack/rollup bundlers
echo   - pkg-no-modules\ - For direct script inclusion
echo.
echo 🔗 JavaScript bindings and TypeScript definitions are included in each package.
echo.
echo 📘 Usage examples:
echo   Web: import init, { BrowserSynapseNode } from './pkg-web/synapse.js';
echo   Node: const synapse = require('./pkg-node/synapse.js');
echo.
echo 🚀 Ready to deploy Synapse in web browsers!

pause
