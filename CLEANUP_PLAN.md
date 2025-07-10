# Synapse Project Cleanup Plan

## Files to Remove

### 1. Obsolete Root Directory Files

- `standalone_demo.rs` - Standalone demo superseded by proper examples
- `transport_test.rs` - Test file superseded by proper test suite
- `real_time_demo.rs` - Demo file superseded by examples
- `enhanced_demo.rs` - Demo file superseded by examples
- `simple_demo.exe` - Compiled binary no longer needed
- `simple_demo.pdb` - Debug symbols no longer needed

### 2. Obsolete Examples

- `examples/ai_assistant.rs` - Has compilation errors, superseded by ai_assistant_network.rs
- `examples/ai_assistant_stub.rs` - Stub file no longer needed
- `examples/basic_chat.rs` - Has compilation errors, superseded by basic_chat_fixed.rs
- `examples/connectivity_demo.rs` - Has compilation errors, superseded by connectivity_demo_fixed.rs
- `examples/tool_interaction.rs` - Has compilation errors, superseded by tool_interaction_fixed.rs
- `examples/http_transport_demo.rs.backup` - Backup file no longer needed
- `examples/unified_transport_test.rs` - Test file, should be moved to tests/ or removed

### 3. Obsolete Test Files (if any have compilation errors)

- Review tests for compilation errors and remove those that don't compile

### 4. Build Artifacts

- `target/` directory contains build artifacts that should be excluded from git
- Various `.exe` and `.pdb` files in root directory

## Files to Keep

### Working Examples

- `examples/hello_world.rs` ✅
- `examples/working_basic_chat.rs` ✅
- `examples/simple_working_demo.rs` ✅
- `examples/basic_chat_fixed.rs` ✅
- `examples/connectivity_demo_fixed.rs` ✅
- `examples/tool_interaction_fixed.rs` ✅
- `examples/ai_assistant_network.rs` ✅
- `examples/multi_modal_collaboration.rs` ✅
- `examples/enterprise_service_mesh.rs` ✅
- `examples/comprehensive_test.rs` ✅
- `examples/production_readiness_test.rs` ✅

### Working Tests

- `tests/comprehensive_feature_test.rs` ✅
- `tests/edge_case_test.rs` ✅
- `tests/security_test.rs` ✅
- `tests/integration_test.rs` ✅
- Other tests that compile successfully

### Documentation

- Essential documentation files will be kept
- Redundant status files may be consolidated

## Next Steps

1. Remove obsolete files
2. Update .gitignore to exclude build artifacts
3. Run final compilation check
4. Update documentation references
