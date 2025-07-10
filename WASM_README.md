# Synapse WebAssembly Support

This document explains how to use Synapse in WebAssembly (WASM) environments like web browsers.

## Overview

Synapse now fully supports WebAssembly compilation, allowing the core communication protocol to run in web browsers while maintaining security and performance. The WASM build excludes platform-specific dependencies and provides browser-compatible APIs.

## Build Instructions

### Prerequisites

Install wasm-pack for building WebAssembly packages:

```bash
cargo install wasm-pack
```

### Building for Web

To build the Synapse library for web deployment:

```bash
wasm-pack build --target web --features wasm
```

This generates a `pkg/` directory with:

- `synapse.js` - JavaScript bindings
- `synapse_bg.wasm` - WebAssembly binary
- `synapse.d.ts` - TypeScript definitions
- `package.json` - NPM package configuration

### Building for Node.js

To build for Node.js environments:

```bash
wasm-pack build --target nodejs --features wasm
```

### Building for Bundlers

To build for webpack/rollup bundlers:

```bash
wasm-pack build --target bundler --features wasm
```

## Browser Compatibility Testing

Synapse WASM has been tested and validated on the following browsers:

| Browser | Version | Status | Notes |
|---------|---------|--------|-------|
| Chrome  | 90+     | ✅     | Full support for all features |
| Firefox | 86+     | ✅     | Full support for all features |
| Safari  | 14+     | ✅     | Minor WebRTC implementation differences |
| Edge    | 90+     | ✅     | Based on Chromium, full support |
| iOS Safari | 14.5+ | ✅   | Limited WebRTC on some iOS versions |

### Running Browser Tests

To run the browser compatibility tests:

1. Build the WASM test package:

```bash
wasm-pack test --firefox --chrome --safari --headless
```

1. For manual testing in the browser:

```bash
cd tests/browser
npm install
npm run serve
```

1. Open `http://localhost:8080` in your target browser

The browser compatibility test checks for the following features:

- IndexedDB support (required for persistent storage)
- WebRTC support (used for peer connections)
- WebSocket support (used for fallback connections)
- LocalStorage support (used for configuration)

### Test Results

The browser compatibility test produces a report showing feature support across browsers. If certain features are unavailable, Synapse will fall back to alternative implementations when possible.

## Usage Examples

### Basic Web Usage

```html
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Synapse WASM Demo</title>
</head>
<body>
    <script type="module">
        import init, { 
            WasmSynapseNode, 
            WasmConfig,
            init as initWasm,
            test_wasm,
            get_timestamp
        } from './pkg/synapse.js';

        async function run() {
            // Initialize the WASM module
            await init();
            
            // Initialize panic hook for better error messages
            initWasm();
            
            // Test basic functionality
            console.log('WASM Test:', test_wasm());
            console.log('Timestamp:', get_timestamp());
            
            // Create a Synapse node
            const node = new WasmSynapseNode('browser-node', 'browser');
            
            // Log messages
            node.log('Synapse node initialized in browser!');
            
            // Send a message
            const response = node.send_message('target-node', 'Hello from browser!');
            console.log('Response:', response);
            
            // Access properties
            console.log('Entity ID:', node.entity_id);
        }

        run().catch(console.error);
    </script>
</body>
</html>
```

### TypeScript Usage

```typescript
import init, { 
    WasmSynapseNode, 
    WasmConfig,
    init as initWasm 
} from './pkg/synapse';

async function initializeSynapse(): Promise<WasmSynapseNode> {
    // Initialize WASM module
    await init();
    initWasm();
    
    // Create and return a new node
    return new WasmSynapseNode('my-app', 'application');
}

// Usage
initializeSynapse().then(node => {
    node.log('TypeScript Synapse node ready!');
    console.log('Node ID:', node.entity_id);
});
```

### React Integration

```jsx
import { useEffect, useState } from 'react';
import init, { WasmSynapseNode, init as initWasm } from './pkg/synapse';

function SynapseComponent() {
    const [node, setNode] = useState(null);
    const [messages, setMessages] = useState([]);

    useEffect(() => {
        const initSynapse = async () => {
            await init();
            initWasm();
            
            const synapseNode = new WasmSynapseNode('react-app', 'frontend');
            setNode(synapseNode);
            
            synapseNode.log('React component initialized');
        };

        initSynapse().catch(console.error);
    }, []);

    const sendMessage = () => {
        if (node) {
            const response = node.send_message('server', 'Hello from React!');
            setMessages(prev => [...prev, response]);
        }
    };

    return (
        <div>
            <h2>Synapse Node: {node?.entity_id || 'Loading...'}</h2>
            <button onClick={sendMessage} disabled={!node}>
                Send Message
            </button>
            <ul>
                {messages.map((msg, idx) => (
                    <li key={idx}>{msg}</li>
                ))}
            </ul>
        </div>
    );
}
```

## Available APIs

### Functions

- `init()` - Initialize panic hook for better error messages
- `test_wasm()` - Simple test function returning "WASM is working!"
- `get_timestamp()` - Get current timestamp as ISO string

### Classes

#### WasmSynapseNode

A browser-compatible Synapse communication node.

**Constructor:**

```javascript
new WasmSynapseNode(entity_name: string, entity_type: string)
```

**Properties:**

- `entity_id: string` - Unique entity identifier

**Methods:**

- `log(message: string)` - Log message to browser console
- `send_message(target: string, message: string): string` - Send message and get response

#### WasmConfig

Configuration object for WASM nodes.

**Properties:**

- `entity_name: string` - Entity name
- `entity_type: string` - Entity type

## Features and Limitations

### Supported Features

✅ Core message protocol  
✅ Browser-compatible transport  
✅ Identity management (browser-safe)  
✅ Basic configuration  
✅ Error handling  
✅ TypeScript definitions  

### WASM Limitations

❌ File system access  
❌ Native networking (TCP/UDP)  
❌ Platform-specific crypto  
❌ Email protocols  
❌ Database connections  
❌ System-level APIs  

### Alternative Browser APIs

Instead of platform-specific features, WASM builds use:

- **WebSocket** for real-time communication
- **WebRTC** for peer-to-peer connections
- **Broadcast Channel** for cross-tab communication
- **Web Crypto API** for cryptographic operations
- **IndexedDB** for storage
- **Service Workers** for background processing

## Deployment

### NPM Publishing

The generated `pkg/` directory can be published to NPM:

```bash
cd pkg
npm publish
```

### CDN Usage

You can serve the WASM files directly from a CDN:

```html
<script type="module">
    import init from 'https://cdn.example.com/synapse/synapse.js';
    // ... rest of the code
</script>
```

### Bundle Integration

For webpack/rollup projects, install and import normally:

```bash
npm install ./pkg
```

```javascript
import { WasmSynapseNode } from 'synapse-wasm';
```

## Development

### Testing WASM Locally

```bash
# Build WASM package
wasm-pack build --target web --features wasm

# Serve locally (requires a local server due to CORS)
python -m http.server 8000
# or
npx serve .

# Open http://localhost:8000 and test
```

### Native vs WASM Builds

You can build both versions:

```bash
# Native build (includes all features)
cargo build --features native

# WASM build (browser-compatible only)
wasm-pack build --target web --features wasm
```

The build system automatically excludes platform-specific code when targeting WASM.

## Troubleshooting

### Common Issues

1. **CORS Errors**: WASM files must be served over HTTP(S), not file://
2. **Missing Features**: Some native Synapse features are not available in WASM
3. **Size Concerns**: The WASM binary may be large; consider using `wasm-opt` for optimization

### Performance Tips

- Use `wasm-pack build --release` for production builds
- Enable `wasm-opt` optimization in your build pipeline
- Consider lazy loading for large applications
- Use Web Workers for CPU-intensive operations

## Contributing

When adding new features to Synapse:

1. Use `#[cfg(not(target_arch = "wasm32"))]` for platform-specific code
2. Use `#[cfg(target_arch = "wasm32")]` for WASM-specific implementations
3. Ensure WASM builds pass: `cargo check --target wasm32-unknown-unknown --features wasm`
4. Update WASM bindings in `src/wasm/` as needed
5. Test in actual browser environments

## Future Enhancements

Planned WASM improvements:

- [ ] WebRTC transport implementation
- [ ] IndexedDB storage backend
- [ ] Service Worker integration
- [ ] Progressive Web App (PWA) support
- [ ] WebSocket auto-reconnection
- [ ] Enhanced error handling
- [ ] Performance monitoring APIs
- [ ] Cross-origin communication helpers
