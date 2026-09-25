# Router Merge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Merge `SynapseRouter` (`src/router.rs`) and `EnhancedSynapseRouter` (`src/router_enhanced.rs`) into one type with the union of both APIs, fix the email wire-format defect and the unsigned-fast-path defect this merge exposes, and delete the unverified receive path rather than carry it forward under a new name.

**Architecture:** One merged router type holds identity/crypto state (from `SynapseRouter`), an optional `MultiTransportRouter` (from `EnhancedSynapseRouter`, kept as a separate internal layer per the design note's §1), and a real `EmailTransportImpl` (from PR #55) obtained via the same `TransportProvider::create_email_transport` bridge `MultiTransportRouter` already uses. Every send path funnels through one shared signing helper before reaching any transport.

**Tech Stack:** Existing crate types only — no new dependencies. `EmailTransportImpl`/`TransportProvider` (PR #55, already merged to `main`), `CryptoManager::sign_secure_message`, `abstraction::{Transport, TransportReceive, RawInbox}`.

**Spec:** `docs/superpowers/specs/2026-09-25-router-merge-design.md` (commit `f0b5b34`, branch `design/router-merge`) — executors read both documents; this plan argues from that design note and does not restate its reasoning, only its binding decisions.

## Global Constraints

Copied verbatim from the design note and the PM's relay of CireSnave's ruling. Every task's requirements implicitly include this section.

- CireSnave, verbatim (relayed by the PM): *"They were likely separate to prevent breaking changes and, in my opinion, that was the wrong choice."* This is 2.0.0 breaking work with the project owner's explicit blessing. **No compatibility shims, no deprecated type aliases preserving the old names `SynapseRouter`/`EnhancedSynapseRouter`.**
- PM, verbatim, requirement 1: *"ONE signing call, reached by every send path... the merged type must make that structurally impossible, not separately fixed twice."*
- PM, verbatim, requirement 2: *"A born-red for Defect 1 specifically: a test that FAILS if any send path emits an unsigned `SecureMessage`. Assert on `sender_proof` being signed, per path, and sabotage it — revert one path to `unsigned()` and confirm red. Every other test in that crate passes today with the fast path unsigned, so nothing existing would catch a regression."*
- PM, verbatim, requirement 3: *"Name what breaks and do not soften it — the 2 shipped CLI binaries, ~9 examples, ~13 integration tests, and `connectivity.rs`/`streaming.rs`/`mcp_server.rs`."*
- Design note §1 (verbatim): *"`MultiTransportRouter` is a layer the merged router composes, not a peer to merge with it."* Do not touch `src/transport/router.rs`'s internals in this plan beyond what Task 5 requires (the export-path demotion).
- Design note §3 (verbatim): *"The merged router's email path must be `EmailTransportImpl`... not `src/email.rs`'s `EmailTransport`, and not through `SimpleMessage`'s round-trip-through-JSON-string indirection."*
- Design note §4 (verbatim): *"Do not let it survive the merge by simply relocating it onto the merged type's namespace — that would be 'keep it' wearing a new struct name."* (On the unverified receive path.)

**Deviations from the design note found while planning, stated here per this project's standing rule (a plan must state where it deviates from its spec, not silently diverge):**

1. The design note's §4 recommendation ("the merged router's receive path becomes `EmailTransportImpl::receive_raw` plus whatever `TransportManager`-equivalent signature verification the rest of this crate already uses") undersold the actual gap. `TransportManager::receive_messages` (`src/transport/manager.rs:671`) verifies via a `TrustStore` (`store.verify_at(message, now)`) — a certificate-chain-based trust model. Neither `SynapseRouter` nor `EnhancedSynapseRouter` currently holds a `TrustStore`; their identity model is `IdentityRegistry::get_public_key(global_id) -> Option<String>` (`src/identity.rs:504`), a flat PEM-lookup model with no certificate chains. These are two different, not-yet-reconciled identity mechanisms in this codebase. Task 4 below is scoped as a research-first task because of this — resolving which model the merged router's verification should use is real design work this plan's own research did not complete, and the task says so explicitly rather than picking one silently.

## File Structure

- **Create** `src/router_merged.rs` (working name — Task 1 Step 1 finalizes it; see that task's naming note) — the merged router type, its constructor, and the shared signing helper.
- **Delete** `src/router.rs` (Task 6, once nothing references it) and `src/router_enhanced.rs` (Task 6).
- **Modify** `src/lib.rs` — remove `pub use router::SynapseRouter;`/`pub use router_enhanced::EnhancedSynapseRouter;`, add the merged type's export; remove `MultiTransportRouter` from the crate-root re-export (Task 5).
- **Modify** every named consumer (Task 6): `src/bin/router.rs`, `src/bin/client.rs`, the ~9 examples, ~13 integration tests, `src/connectivity.rs`, `src/streaming.rs`, `src/mcp_server.rs`.

## Task 1: Merged struct, constructor, and the three named collisions

**Files:**
- Create: `src/router_merged.rs`
- Test: inline `#[cfg(test)]` in the same file

**Interfaces:**
- Produces: `pub struct SynapseRouter` (the design note's open question on naming is resolved here: keep the name `SynapseRouter` — it is the more fundamental of the two old names, and per the "no compatibility shim" constraint there is no old `SynapseRouter` left to collide with once Task 6 deletes it) with `pub async fn new(config: Config, our_global_id: String) -> Result<Self>`, `pub async fn start(&self) -> Result<()>`, `pub async fn status(&self) -> EnhancedRouterStatus` (moved into this file from `router_enhanced.rs`, see Step 4).

- [ ] **Step 1: Write the failing test for the merged constructor**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[tokio::test]
    async fn new_builds_a_router_with_both_apis_available() {
        let config = Config::default_for_entity("Test", "tool");
        let router = SynapseRouter::new(config, "test@synapse.local".to_string())
            .await
            .expect("construct");
        // From the old SynapseRouter side:
        assert_eq!(router.get_our_global_id(), "test@synapse.local");
        // From the old EnhancedSynapseRouter side -- proves both halves are actually present,
        // not just one renamed:
        let status = router.status().await;
        assert!(!status.multi_transport_enabled || status.available_transports.is_empty());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib router_merged::tests::new_builds_a_router_with_both_apis_available -- --exact`
Expected: FAIL to compile — `src/router_merged.rs` does not exist yet.

- [ ] **Step 3: Read both source files' full current state before writing the merge**

Run: `cat src/router.rs src/router_enhanced.rs` (or read them with your file tool) — both files have been read in full during this plan's own research, but re-read them yourself before transcribing, since a subagent implementing this task did not do that research itself. Confirm the exact field lists: `SynapseRouter` holds `crypto: Arc<RwLock<CryptoManager>>`, `identity: Arc<RwLock<IdentityRegistry>>`, `email: Arc<RwLock<EmailTransport>>` (the OLD `src/email.rs` type — this field's type changes in Task 2, do not carry it forward as-is), `our_global_id: String`. `EnhancedSynapseRouter` holds `synapse_router: SynapseRouter`, `multi_transport: Option<Arc<MultiTransportRouter>>`, plus email-server-related fields for `email_server()`/`is_running_email_server()`/`email_server_connectivity()` — read these three methods' bodies to find the exact field(s) backing them.

- [ ] **Step 4: Write the merged struct**

```rust
// SPDX-License-Identifier: MIT OR Apache-2.0
//! The merged Synapse router: `SynapseRouter` + `EnhancedSynapseRouter`'s combined API, per
//! docs/superpowers/specs/2026-09-25-router-merge-design.md. No compatibility shim for the two
//! old names -- CireSnave's ruling was that keeping them separate was itself the wrong call.

use crate::{
    CryptoManager,
    config::Config,
    error::Result,
    identity::IdentityRegistry,
    transport::router::MultiTransportRouter,
};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct SynapseRouter {
    crypto: Arc<RwLock<CryptoManager>>,
    identity: Arc<RwLock<IdentityRegistry>>,
    /// `None` until Task 2 wires a real `EmailTransportImpl` in; kept as a distinct step so
    /// Task 1's own test can pass without depending on Task 2's transport-construction code.
    email: Arc<RwLock<Option<Arc<dyn crate::transport::abstraction::Transport>>>>,
    multi_transport: Option<Arc<MultiTransportRouter>>,
    our_global_id: String,
    // Task 5's `email_server()`/`is_running_email_server()`/`email_server_connectivity()` fields
    // are copied here verbatim from whatever Step 3's research found backing them in
    // `router_enhanced.rs` -- this plan does not repeat that struct literal since it was not
    // re-verified at the time of writing; copy it exactly, field names and types unchanged.
}

impl SynapseRouter {
    pub async fn new(config: Config, our_global_id: String) -> Result<Self> {
        let crypto = Arc::new(RwLock::new(CryptoManager::new()));
        let identity = Arc::new(RwLock::new(IdentityRegistry::new()));
        Ok(Self {
            crypto,
            identity,
            email: Arc::new(RwLock::new(None)),
            multi_transport: None,
            our_global_id,
        })
    }

    /// Old `SynapseRouter::start()`'s real body (Step 3), with `EnhancedSynapseRouter::start()`'s
    /// previously-commented-out inner call now live -- Step 3 found it dead
    /// (`// self.synapse_router.start().await?;`, `router_enhanced.rs:619`); un-comment it here as
    /// part of whatever this merged `start()` does.
    pub async fn start(&self) -> Result<()> {
        // Implementer: transcribe the real body found in Step 3, not a stub. If the old
        // `EnhancedSynapseRouter::start()` did other setup around the commented-out call (read its
        // full body before writing this), that setup is preserved too.
        Ok(())
    }

    pub fn get_our_global_id(&self) -> &str {
        &self.our_global_id
    }
}
```

**Implementer note:** the `email` field's type (`Arc<RwLock<Option<Arc<dyn Transport>>>>`) and the missing `status()`/email-server fields are deliberately incomplete here — Step 3 requires you to read the actual source before finalizing the struct literal, and this plan's own code sample is not a substitute for that read. Do not skip Step 3 because this step already has code in it.

- [ ] **Step 5: Add `status()`, run the test**

Add `pub async fn status(&self) -> EnhancedRouterStatus` using the struct copied from `router_enhanced.rs:801-806` (`RouterHealth`/`multi_transport_enabled`/`email_server_enabled`/`available_transports`) — construct a `RouterHealth` from this router's own crypto/identity state (mirroring old `SynapseRouter::get_health()`'s body, found in Step 3) as `synapse_status`, and the other three fields from whatever state Step 4's struct ended up holding.

Run: `cargo test --lib router_merged::tests::new_builds_a_router_with_both_apis_available -- --exact`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/router_merged.rs
git commit -m "feat: merged SynapseRouter struct, constructor, start/status collision resolution"
```

## Task 2: One shared signing helper; email-only `send_message` through `EmailTransportImpl`

**Files:**
- Modify: `src/router_merged.rs`
- Test: inline `#[cfg(test)]`

**Interfaces:**
- Consumes: `SynapseRouter` (Task 1); `transport::providers::{TransportProvider, ProductionTransportProvider}` (existing, PR #55); `CryptoManager::sign_secure_message(&self, &mut SecureMessage) -> Result<()>` (existing, `src/crypto.rs:182`).
- Produces: `async fn sign_new_message(&self, to: &str, content: &[u8], security_level: SecurityLevel) -> Result<SecureMessage>` (private helper — Task 3 also calls this, do not duplicate its logic); `pub async fn send_message(&self, simple_msg: SimpleMessage, destination_global_id: String) -> Result<()>` (the explicit email-only send, old `SynapseRouter`'s name and shape kept per the design note's §2).

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn send_message_signs_before_handing_to_the_email_transport() {
    let config = Config::default_for_entity("Test", "tool");
    let mut router = SynapseRouter::new(config, "alice@synapse.local".to_string())
        .await
        .expect("construct");
    router.crypto.write().await.generate_keypair().expect("keypair");
    router.ensure_email_transport().await.expect("email transport");

    let simple_msg = SimpleMessage {
        to: "bob@synapse.local".to_string(),
        from_entity: "alice@synapse.local".to_string(),
        content: "hello".to_string(),
        message_type: MessageType::Direct,
        metadata: Default::default(),
    };
    // This will fail to actually deliver (no real SMTP server at bob@synapse.local in a unit
    // test), but the point of this test is what happens BEFORE delivery is attempted: the
    // signing. Capture the SecureMessage sign_new_message produces directly instead of only
    // asserting on send_message's overall Ok/Err, since a network failure and a signing failure
    // would otherwise look the same from outside.
    let signed = router
        .sign_new_message("bob@synapse.local", b"hello", SecurityLevel::Authenticated)
        .await
        .expect("sign");
    assert!(
        !matches!(signed.sender_proof.alg, crate::sender_auth::ProofAlg::None),
        "a message built for sending must be signed, not left as alg \"none\""
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib send_message_signs_before_handing_to_the_email_transport -- --exact`
Expected: FAIL to compile — `sign_new_message`/`ensure_email_transport` don't exist yet.

- [ ] **Step 3: Implement the shared signing helper**

```rust
impl SynapseRouter {
    /// The ONE place a `SecureMessage` is built and signed for sending. Every send path in this
    /// router — `send_message`, `send_message_smart`'s fast branch (Task 3),
    /// `send_message_with_transport` — calls this, so no path can reach a transport with an
    /// unsigned message by forgetting to call `sign_secure_message` itself. This is the
    /// structural fix for board item 65 (the old `EnhancedSynapseRouter::create_secure_message`
    /// hardcoded `SenderProof::unsigned()` and never signed at all).
    async fn sign_new_message(
        &self,
        to_global_id: &str,
        content: &[u8],
        security_level: SecurityLevel,
    ) -> Result<SecureMessage> {
        let mut message = SecureMessage {
            message_id: crate::synapse::blockchain::serialization::UuidWrapper::new(
                uuid::Uuid::new_v4(),
            ),
            to_global_id: to_global_id.to_string(),
            from_global_id: self.our_global_id.clone(),
            encrypted_content: content.to_vec(),
            sender_proof: crate::sender_auth::SenderProof::unsigned(),
            timestamp: crate::synapse::blockchain::serialization::DateTimeWrapper::new(
                chrono::Utc::now(),
            ),
            security_level,
            routing_path: Vec::new(),
            metadata: Default::default(),
            protocol_version: crate::types::PROTOCOL_VERSION,
        };
        let crypto = self.crypto.read().await;
        crypto.sign_secure_message(&mut message)?;
        Ok(message)
    }

    /// Constructs this router's `EmailTransportImpl` on first use, via the same
    /// `TransportProvider::create_email_transport` bridge `MultiTransportRouter` already uses
    /// (`src/transport/providers.rs:114`) -- not a second, separate construction path.
    async fn ensure_email_transport(&self) -> Result<Arc<dyn crate::transport::abstraction::Transport>> {
        {
            let existing = self.email.read().await;
            if let Some(t) = existing.as_ref() {
                return Ok(Arc::clone(t));
            }
        }
        let provider = crate::transport::providers::ProductionTransportProvider;
        // Implementer: `create_email_transport` needs the router's own `Config`, which this
        // struct does not currently store as a field (Task 1's struct literal didn't keep it --
        // check Step 3 of Task 1 again and add a `config: Config` field to the struct now if it
        // is missing, since this method needs it and `new()` already receives one).
        let transport = provider
            .create_email_transport(&self.config)
            .await?
            .ok_or_else(|| crate::error::SynapseError::TransportError(
                "email transport construction returned None".to_string(),
            ))?;
        transport.start().await?;
        let mut slot = self.email.write().await;
        *slot = Some(Arc::clone(&transport));
        Ok(transport)
    }

    /// Email-only send, no transport selection -- the explicit low-level path old `SynapseRouter`
    /// callers reached for. Routes through `EmailTransportImpl` (PR #55), never `src/email.rs`'s
    /// `EmailTransport` and never a `SimpleMessage`-round-tripped-through-JSON-string.
    pub async fn send_message(
        &self,
        simple_msg: SimpleMessage,
        destination_global_id: String,
    ) -> Result<()> {
        let message = self
            .sign_new_message(
                &destination_global_id,
                simple_msg.content.as_bytes(),
                SecurityLevel::Authenticated,
            )
            .await?;
        let transport = self.ensure_email_transport().await?;
        let target = crate::transport::abstraction::TransportTarget::new(destination_global_id);
        transport.send_message(&target, &message).await?;
        Ok(())
    }
}
```

- [ ] **Step 4: Run the test**

Run: `cargo test --lib send_message_signs_before_handing_to_the_email_transport -- --exact`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/router_merged.rs
git commit -m "feat: one shared signing helper, email-only send_message via EmailTransportImpl"
```

## Task 3: Fix the fast path (board item 65) and its born-red test

**Files:**
- Modify: `src/router_merged.rs`
- Test: inline `#[cfg(test)]`

**Interfaces:**
- Consumes: `sign_new_message` (Task 2); `MultiTransportRouter::send_message(&self, target: &str, message: &SecureMessage, urgency: MessageUrgency) -> Result<DeliveryReceipt>` (existing, unchanged).
- Produces: `pub async fn send_message_smart(&self, to_entity: &str, content: &str, message_type: MessageType, security_level: SecurityLevel, urgency: MessageUrgency) -> Result<String>` (old `EnhancedSynapseRouter`'s name and signature kept).

- [ ] **Step 1: Write the required born-red test FIRST, and confirm it is currently meaningful**

This is the PM's explicit requirement 2. Write it before the fix, so its "currently red for the right reason" state is provable, not assumed:

```rust
#[tokio::test]
async fn every_send_path_signs_before_it_reaches_a_transport() {
    let config = Config::default_for_entity("Test", "tool");
    let mut router = SynapseRouter::new(config, "alice@synapse.local".to_string())
        .await
        .expect("construct");
    router.crypto.write().await.generate_keypair().expect("keypair");

    // The property under test: every message this router hands to ANY transport must be signed.
    // send_message (Task 2) already provably signs -- assert it again here for completeness --
    // and send_message_smart's fast (MultiTransportRouter) branch must too, which it does not
    // yet at the start of this task.
    let via_send_message = router
        .sign_new_message("bob@synapse.local", b"one", SecurityLevel::Authenticated)
        .await
        .expect("sign");
    assert!(!matches!(via_send_message.sender_proof.alg, crate::sender_auth::ProofAlg::None));

    // This calls send_message_smart with RealTime urgency (the fast/MultiTransportRouter branch)
    // and inspects the SecureMessage it actually builds before handing to MultiTransportRouter --
    // requires send_message_smart's fast-path message construction to be observable, which Step
    // 2's fix makes true by routing it through the same sign_new_message helper. Implementer: if
    // MultiTransportRouter::send_message itself cannot be easily mocked/observed in a unit test
    // (it dials real transports), test this at the level of "send_message_smart's fast branch
    // calls self.sign_new_message before calling multi_transport.send_message" via a smaller,
    // more targeted assertion -- e.g., refactor send_message_smart's fast branch to build the
    // message via sign_new_message and assert on the RETURNED value of a testable inner function,
    // rather than asserting on network-dependent end-to-end behavior. Do not weaken this test to
    // "send_message_smart doesn't panic" -- it must assert on the signature specifically.
}
```

- [ ] **Step 2: Run test to verify it fails for the right reason**

Run: `cargo test --lib every_send_path_signs_before_it_reaches_a_transport -- --exact`
Expected: FAIL — because `send_message_smart` doesn't exist yet at the start of this task (it moves from `router_enhanced.rs` in this step). Once it exists but before its fast branch is fixed, re-run and confirm the failure mode changes to "the fast branch's message has `ProofAlg::None`" specifically, not a compile error — this is the actual regression state the born-red test must catch.

- [ ] **Step 3: Port `send_message_smart`, fixing the fast branch to use `sign_new_message`**

```rust
impl SynapseRouter {
    pub async fn send_message_smart(
        &self,
        to_entity: &str,
        content: &str,
        message_type: MessageType,
        security_level: SecurityLevel,
        urgency: MessageUrgency,
    ) -> Result<String> {
        if let Some(ref mt_router) = self.multi_transport
            && matches!(urgency, MessageUrgency::RealTime | MessageUrgency::Interactive)
        {
            // Fixed: was EnhancedSynapseRouter::create_secure_message, which hardcoded
            // SenderProof::unsigned() and never called the crypto manager (board item 65). Now
            // the same sign_new_message every other path uses.
            let secure_msg = self
                .sign_new_message(to_entity, content.as_bytes(), security_level)
                .await?;
            match mt_router.send_message(to_entity, &secure_msg, urgency).await {
                Ok(receipt) => return Ok(receipt.message_id),
                Err(e) => {
                    tracing::warn!("Multi-transport failed: {e}, falling back to email");
                }
            }
        }
        let simple_msg = SimpleMessage {
            to: to_entity.to_string(),
            from_entity: self.our_global_id.clone(),
            content: content.to_string(),
            message_type,
            metadata: Default::default(),
        };
        self.send_message(simple_msg, to_entity.to_string())
            .await
            .map(|_| "email_fallback".to_string())
    }
}
```

Note the old `create_secure_message` private method (`router_enhanced.rs:661-682`) is not ported at all — it is deleted by this port, not kept alongside the fix. Grep for any other caller of it before deleting (Step 3 of Task 1's research should already have found none, but confirm).

- [ ] **Step 4: Run the born-red test, confirm PASS**

Run: `cargo test --lib every_send_path_signs_before_it_reaches_a_transport -- --exact`
Expected: PASS.

- [ ] **Step 5: Required sabotage — confirm the test actually catches the regression**

Per the PM's explicit requirement: temporarily revert the fast branch's `self.sign_new_message(...)` call back to constructing a `SecureMessage` with `SenderProof::unsigned()` directly (the old `create_secure_message` shape). Run the test again — it must fail. Revert the sabotage. Run again — it must pass. Report both outcomes in this task's own report; do not skip this step because Step 4 already passed once.

- [ ] **Step 6: Commit**

```bash
git add src/router_merged.rs
git commit -m "fix: board item 65 -- send_message_smart's fast path now signs via the shared helper"
```

## Task 4: Receive path — research first, then delete-and-replace

**Files:**
- Modify: `src/router_merged.rs`
- Test: inline `#[cfg(test)]`

**This task starts with research this plan's own writing did not finish (stated in Global Constraints' Deviations section above) — do not skip straight to code.**

- [ ] **Step 1: Resolve the identity-model question**

Read `src/identity.rs`'s `IdentityRegistry` in full (not just `get_public_key`) and `src/sender_auth.rs`'s `TrustStore`/`verify_at` in full. Decide, and record your reasoning in this task's report: does the merged router's receive-side verification use `IdentityRegistry::get_public_key` directly (a flat, simpler check — the router already has an `IdentityRegistry` field from Task 1) with a raw signature-verification call against that key, or does it adopt a `TrustStore` (heavier, but reuses the crate's own certificate-chain trust model that every other transport's receive path already relies on via `TransportManager`)? **Recommendation, not a mandate:** prefer `IdentityRegistry`, since introducing `TrustStore` as new router state is a larger scope addition this plan did not budget for, and `IdentityRegistry`'s flat model is sufficient to close the specific gap here (some real check, replacing zero checks) without taking on certificate-chain machinery the router has never had. If you choose `TrustStore` instead, say why in the report — it is a legitimate choice, just a bigger one.

- [ ] **Step 2: Write the failing test**

```rust
#[tokio::test]
async fn receive_messages_rejects_an_unverifiable_sender() {
    let config = Config::default_for_entity("Test", "tool");
    let router = SynapseRouter::new(config, "bob@synapse.local".to_string())
        .await
        .expect("construct");
    // Implementer: construct a SecureMessage with a signature that does not match any key
    // IdentityRegistry (or TrustStore, per Step 1's choice) knows about, deliver it into the
    // router's email transport's receive path directly (via receive_raw + the router's own inbox,
    // mirroring how tests/transport_repairs.rs's Pair harness delivers messages between two real
    // transport instances on loopback -- read that harness before writing this test's delivery
    // mechanism), and assert receive_messages() does NOT return it as a delivered message.
}
```

- [ ] **Step 3: Delete the unverified path, implement the replacement**

Delete `process_email_message`'s equivalent entirely (it does not get ported to `router_merged.rs` at all — this is a deletion, not a fix-in-place, per the design note's §4 and the Global Constraints quote above). Implement `pub async fn receive_messages(&self) -> Result<Vec<SimpleMessage>>` using `ensure_email_transport()` (Task 2) to get the `Arc<dyn Transport>`, call its `receive_raw(&mut inbox)` (construct `RawInbox::new()` — accessible since this file is in-crate), drain it, and verify each message against whatever Step 1 chose before converting to `SimpleMessage` and including it in the result. A message that fails verification is dropped, not returned — mirroring `TransportManager::receive_messages`'s own `verdict.is_verified()` gate.

- [ ] **Step 4: Run the test, confirm PASS. Commit.**

```bash
git add src/router_merged.rs
git commit -m "feat: real receive-path verification, replacing the deleted unverified process_email_message"
```

## Task 5: Demote `MultiTransportRouter`'s export; finalize `lib.rs`

**Files:**
- Modify: `src/lib.rs`

**Interfaces:** none new — this task only changes what's re-exported, not what exists.

- [ ] **Step 1:** In `src/lib.rs`, remove `pub use router::SynapseRouter;` and `pub use router_enhanced::EnhancedSynapseRouter;` (or whatever the exact current lines are — re-check, since Task 1-4 haven't touched `lib.rs` yet), replace with `pub use router_merged::SynapseRouter;`. Remove `MultiTransportRouter` from the top-level re-export list entirely (it remains `pub` inside `src/transport/`, reachable as `crate::transport::router::MultiTransportRouter` for anything that genuinely needs direct access).

- [ ] **Step 2:** `cargo build --lib` and read every error — this will surface every remaining reference to the old names, which is Task 6's job to fix, but confirm here that the error list matches this plan's expected consumer list (Global Constraints' requirement 3) before moving on, so a missed consumer is caught now rather than discovered mid–Task 6.

- [ ] **Step 3: Commit**

```bash
git add src/lib.rs
git commit -m "refactor: export the merged SynapseRouter; demote MultiTransportRouter from the crate root"
```

## Task 6: Migrate every named consumer; delete the old files

**Files:**
- Modify: `src/bin/router.rs`, `src/bin/client.rs`, `src/connectivity.rs`, `src/streaming.rs`, `src/mcp_server.rs`
- Modify: `examples/ai_assistant_network.rs`, `ai_assistant_stub.rs`, `basic_chat.rs`, `email_integration_test.rs`, `enhanced_router_demo.rs`, `enterprise_service_mesh.rs`, `multi_modal_collaboration.rs`, `simple_unknown_name_resolution.rs`, `tool_interaction.rs`, `unified_transport_demo.rs`
- Modify: the ~13 integration tests Task 5's build-error list surfaces
- Delete: `src/router.rs`, `src/router_enhanced.rs`

- [ ] **Step 1:** Work through Task 5 Step 2's build-error list one file at a time. For each: update the constructor call to `SynapseRouter::new(config, id)` (unchanged signature), and update any method call that changed (`status()`'s return type is now always `EnhancedRouterStatus` — update any code that pattern-matched the old plain `String`; `send_message`/`send_message_smart` names are unchanged from what each old type already had, so most call sites should need only the type name fixed, not the method name).

- [ ] **Step 2:** For `src/bin/client.rs`'s `listen_for_messages` specifically: this previously called `receive_messages()` against a path that always returned empty (the permanent stub, deleted in Task 4). It now calls a real, verifying receive path. This is a behavior change, not just a signature fix — note it explicitly in this task's report, since a CLI command going from "does nothing" to "does something" is worth flagging even though no code review would catch it as a type error.

- [ ] **Step 3:** Once every consumer builds, delete `src/router.rs` and `src/router_enhanced.rs`. Run `cargo build --lib --tests --examples` clean.

- [ ] **Step 4:** `cargo fmt --check`, `cargo clippy --lib --tests --all-targets --keep-going -- -D warnings` (confirm the known pre-existing baseline is unchanged — re-derive the current baseline list at implementation time via a clean run on `main` first, since this plan was written before checking whether PR #55's merge changed it). `cargo test --lib --tests` full suite.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: migrate all consumers to the merged SynapseRouter, delete router.rs/router_enhanced.rs"
```

## Task 7: Verification

- [ ] **Step 1: Mutation check.** Revert Task 3's fast-path fix (this is the same sabotage as Task 3 Step 5, but run here as part of the formal verification pass, under a hard timeout) and confirm `every_send_path_signs_before_it_reaches_a_transport` fails; revert, confirm green.
- [ ] **Step 2: Full suite**, `cargo build --tests && cargo test --lib --tests` in this worktree.
- [ ] **Step 3: fmt/clippy**, both invocations (`cargo clippy -- -D warnings` and `--lib --tests --all-targets --keep-going -- -D warnings`).
- [ ] **Step 4: Breaking-changes list for the PR body** — compiled from Task 6's per-file notes: every consumer that changed, `status()`'s return type, `listen`'s behavior change, `MultiTransportRouter` no longer at the crate root, the three old public type names gone with no shim.
- [ ] **Step 5: Update `CAPABILITY_INVENTORY.md`** to reflect the merge and mark the now-resolved findings (the unverified receive path, the unsigned fast path, the two-siblings-with-incoherent-capability-split naming confusion) as fixed, with the specific commits.

No version bump — this crate gets none until 2.0.0 publishes, which needs the project owner's explicit approval.

## Self-Review

**Spec coverage:** design note §1 (MultiTransportRouter stays separate) → untouched by this plan except Task 5's export change, as required. §2 (merged API, collisions) → Task 1. §3 (one email path) → Task 2. §4 (delete unverified receive) → Task 4. §5 (demote MultiTransportRouter export) → Task 5. §6 (migration) → Task 6. The independently-found third defect (unsigned fast path) → Task 3, with the PM's required born-red test.

**Placeholder scan:** Task 1 Step 4's struct literal is intentionally incomplete pending Step 3's research (flagged explicitly, not silently). Task 2's `ensure_email_transport` flags a missing `config` field on the struct that Task 1 may need to add — flagged, not assumed. Task 4 is explicitly research-first per the Global Constraints' stated deviation. All three are stated limitations with a named next step, not vague instructions.

**Type consistency:** `sign_new_message` (Task 2) is the one signing helper both Task 2's `send_message` and Task 3's `send_message_smart` call — same name, same signature, used consistently. `ensure_email_transport` (Task 2) is reused by Task 4's receive path. `EnhancedRouterStatus` (Task 1) is the return type kept consistent through every later task that touches `status()`.
