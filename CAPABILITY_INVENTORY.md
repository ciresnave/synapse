# Synapse — Measured Capability Inventory

**Measured 2026-09-09** against `main` @ `f0f570c` (verified current: `git rev-parse main` ==
`git ls-remote origin main`, 0 commits behind), on `rustc 1.100.0-nightly (cea272fa3, 2026-09-07)`,
`cargo 1.100.0-nightly`, host `x86_64-pc-windows-msvc`.

Every number below is the output of a command that was run. Where I state an absence, I state the
positive control that proves the query could have found something. Nothing here is inferred from a
document, a filename, or a commit message.

**Read this before the README.** The README is marketing copy, not a specification; §6 lists the
specific properties it asserts that the code has never had.

---

## 0. The one-line summary

At `f0f570c`, **no invocation of cargo in this repository succeeded — in any feature
configuration.** Not `build`, not `check`, not `test`, not `clippy`. The repo did not reach the
compiler. Two defects caused this; both are now fixed, and the library builds and 102 tests run.

| | at `f0f570c` | now |
|---|---|---|
| `cargo check --lib` (default features) | **fails at dependency resolution** | **EXIT 0** |
| `cargo test` | **fails at dependency resolution** | **102 tests, 101 pass, 1 fail** |
| feature configurations that resolve | **0 of 10** | 10 of 10 |
| feature configurations that compile | **0 of 10** | **1 of 10** (`native` only — see §3) |

**And the question nothing in the repository answers — does a message actually get from A to B?** No
test sends one over a socket, so I ran the probes myself (§2.2):

| | result |
|---|---|
| **UDP round trip** | ✅ **DELIVERS** — payload intact, ~1 ms, via the factory path with `bind_port` set |
| **TCP round trip** | 🔴 **SILENTLY DROPS** — `send_message` returns `Ok(confirmation: Sent)`; nothing ever arrives |
| **WebSocket** | 🔴 same defect as TCP, read not run |
| **QUIC** | 🔴 **simulation** — binds nothing, fabricates connections with a hardcoded RTT |

⚠️ **So Synapse can carry a message today, over UDP, and the two transports have opposite and
undocumented construction requirements.** That single fact matters more for planning than everything
else in this document.

---

## 1. What was blocking the build

### 1.1 Three dependency features that have never existed

`Cargo.toml` requested five features from `auth-framework`; three of them do not exist:

```
requested: oauth-device-flows  enhanced-device-flow  config-management  enterprise-features  token-to-profile
exists:    yes                 yes                   NO                 NO                   NO
```

Checked against `index.crates.io/au/th/auth-framework` — **every published version**: `0.1.1`,
`0.2.0`, `0.3.0`, `0.4.0`, `0.4.1`, `0.4.2`, `0.5.0-rc1`, `0.5.0-rc18`, `0.5.0-rc19`.
`config-management`, `enterprise-features`, `token-to-profile` are absent from all nine.
**Positive control:** the same query reports `enhanced-device-flow` PRESENT in 0.3.0 onward, and
`oauth-device-flows` resolves as the implicit feature of auth-framework's optional dependency of the
same name. The query discriminates. Independently confirmed absent in auth-framework's unpublished
`main` (rc24) by the auth-framework lane, using its own control set.

**Cargo validates a dependency's requested feature names at RESOLUTION time, whether or not the
optional dependency is enabled.** So this single line broke configurations that do not use auth at
all. Measured, all ten feature sets, identical first error:

```
minimal core crypto networking nat_traversal email database auth wasm native
  -> 10 of 10 EXIT=101, zero lines compiled

error: failed to select a version for `auth-framework`.
package `synapse` depends on `auth-framework` with feature `config-management`
but `auth-framework` does not have that feature.
```

Note cargo names only the *first* missing feature, so the error message undercounts by two.

**Provenance — the breakage is isolated to one commit.** Measured with
`git show <ref>:Cargo.toml` across the release history (bisect contributed by the Claim Auditor lane,
verified here independently):

```
7b80ca4  (v1.1.0 release)   oauth-device-flows  enhanced-device-flow
7775f19                     oauth-device-flows  enhanced-device-flow
c06ad4a                     oauth-device-flows  enhanced-device-flow
f0f570c  (HEAD, checkpoint) oauth-device-flows  enhanced-device-flow
                            + config-management + enterprise-features + token-to-profile
```

**Every tagged/release commit requests only the two features that exist. `f0f570c` — "Checkpoint:
commit all local work before machine wipe" — introduced all three phantom names.** So the repo was
resolvable through v1.1.0 and was broken by the emergency checkpoint, which is also the commit that
renamed the trust-system test to `.disabled` (§4) and published
`AUTH_FRAMEWORK_INTEGRATION_SUMMARY.md` (§6.1). ⚠️ **One commit, made under time pressure, carries
the build break, the disabled test, and the document asserting the work was complete.**

**Fix applied:** removed the three names. This is deleting a statement that was never true, not a
version migration. The `^0.3.0` pin is untouched — crates.io serves `0.4.2` stable and `0.5.0-rc19`,
and for a 0.x crate `^0.3.0` excludes both, but that upgrade is a merge decision and is left open.

### 1.2 A forced linker that is not installed

`.cargo/config.toml` (added in `f0f570c` itself) set `linker = "lld-link"` unconditionally. `lld-link`
ships with LLVM, not with Rust. Without LLVM installed, every build died before compiling:

```
error: linker `lld-link` not found
```

**Fix applied:** commented out, with the reasoning preserved in the file. Measured: the default MSVC
linker links the library and all 11 integration-test binaries with no LNK1318. The problem the
override was added for did not reproduce.

---

## 2. Verified working — measured by execution

`cargo test` on the default (`native`) feature set. **102 tests, 101 passed, 1 failed, 0 ignored.**

| target | tests | result |
|---|---:|---|
| `--lib` (unit tests) | 62 | all pass |
| `basic_integration_test` | 4 | all pass |
| `concurrent_registry_access_test` | 4 | all pass |
| `edge_case_test` | 8 | all pass |
| `high_load_test` | 2 | all pass |
| `integration_test` | 3 | all pass |
| `multi_transport_integration` | 4 | all pass |
| `network_partition_test` | 3 | all pass |
| `registry_integration_test` | 1 | all pass |
| `security_test` | 8 | all pass |
| `transport_error_handling_test` | 3 | **2 pass, 1 FAIL** |

**Nothing is `#[ignore]`d.** Count is 0 across every test file — this is a real zero, not an absent
query: the same scan returns 53 host test attributes plus 7 `#[wasm_bindgen_test]` in `tests/` (§7).

### 2.1 The one real test failure — a genuine behavioural defect

```
thread 'test_transport_error_handling' panicked at tests\transport_error_handling_test.rs:45:21:
Should fail with invalid email
```

The transport **accepted an invalid email address that the test asserts must be rejected**. This is
not an infrastructure problem; it is the test doing its job. It took 8.08s, which suggests a network
timeout path is involved. **Deliberately not fixed** — it is a behaviour question, and the code that
owns it may not survive the merge.

### 2.2 ⚠️ END-TO-END ROUND TRIP: MEASURED, AND IT FAILS WHILE REPORTING SUCCESS

**No test in the suite sends a message between two endpoints over a real socket, so I ran one.** This
is the most consequential measurement in this document.

**Probe A — Synapse to Synapse over TCP, one process, two `TcpTransportImpl` instances.** Receiver
bound to port 47811; sender given the receiver's address:

```
[probe] receiver.start() OK -> status Running
[probe] PORT 47811 IS ACCEPTING (raw TcpStream::connect succeeded)
[probe] send_message OK -> DeliveryReceipt { transport_used: Tcp, delivery_time: 1.19ms,
                             target_reached: "127.0.0.1:47811", confirmation: Sent }
[probe] attempt 1..5: receive_messages -> 0 message(s)
[probe] RESULT: NO MESSAGE ARRIVED
```

⚠️ **`send_message` returns `Ok` with `confirmation: Sent` and a delivery time, for a message that is
never delivered.** A caller has no way to detect the failure from the API.

**Cause — established, not inferred.** `TcpTransportImpl::new` binds `self.listener` on the port.
Then `start_server()` spawns a task that binds a **second** `TcpListener` on **the same address**:

```rust
let local_addr = listener.local_addr()?;         // self.listener already owns this port
tokio::spawn(async move {
    if let Ok(listener) = TcpListener::bind(local_addr).await {   // <-- FAILS, port in use
        loop { match listener.accept().await { ... } }            // <-- never runs
    } else {
        error!("Failed to create TCP listener for server task");  // <-- swallowed
    }
});
```

**Confirmed by turning the swallowed log on** (`RUST_LOG=synapse=debug`), which is the discriminating
test — the error fires *between* `start()` returning `Ok` and the send "succeeding":

```
INFO  TCP transport listening on port 47812        <- self.listener binds
INFO  TCP transport started successfully           <- start() returns Ok, status Running
ERROR Failed to create TCP listener for server task  <- the accept loop never starts
INFO  TCP message sent to 127.0.0.1:47812 in 1.01ms  <- reported success
      receive_messages -> 0 message(s)  x5
```

**So the port accepts connections — `self.listener`'s backlog absorbs them — but nothing ever calls
`accept()` on it.** Bytes are written into a socket no one reads. `start()` returns `Ok(())` and
`status()` reports `Running` regardless.

#### ✅ Independently confirmed, by a different method, and the fix is verified

The Fuel 1 lane replicated this at the portfolio PM's request, deliberately **without** my probe, my
instrument, or my diagnosis — they were given the symptom only, plus a frozen base and a
pre-registered prediction. They reached the **same cause by reading**, and then went further than I
did:

```
their observation:  send Ok / confirmation=Sent / 1.35ms, receive_messages -> 0
                    then -> 1 AFTER THEIR FIX
their cause:        the double-bind, same lines (:57 bind, :101 re-bind, :128 swallowed else)
their fix:          make self.listener an Arc<TcpListener>, clone the Arc into the accept
                    loop, delete the second bind
```

⚠️ **Two independent methods converged on one cause, and the fix is verified rather than plausible.**
I had recorded this as "looks small — someone should confirm that before believing it." It is now
confirmed: **a message arrives after the change.** Filed as
[`ciresnave/synapse#9`](https://github.com/ciresnave/synapse/issues/9) with the patch, marked
UNAPPLIED because `push=false`.

⚠️ **And they resolved a gap I had flagged in my own experiment.** I had noted that I only ever set an
explicit `listen_port` and never tested `listen_port = 0` / absent. Their answer: that is a
**separate state, not this bug** — with port 0 the constructor binds no listener at all,
`start_server` is a no-op, and there is no address to receive on. **So there are two distinct ways to
get a deaf TCP transport: "never a server" (port unset) and "port owned but never served" (port set).
Both report `Running`.** The measured symptom above is the second.

**Probe B — the wire format, captured by a RAW `tokio::net::TcpListener` with no Synapse on the
receiving side.** This direction *works*:

```
[probe] raw listener accepted from 127.0.0.1:52059
[probe] ===== 364 BYTES ON THE WIRE =====
{"message_id":"a3ca0312-...","to_global_id":"bob@synapse.local",
 "from_global_id":"alice@synapse.local","encrypted_content":[82,79,85,78,...],
 "signature":[1,2,3,4],"timestamp":"2026-09-09T14:13:31.517436100Z",
 "security_level":"public","routing_path":[],"metadata":{"probe":"wire-format"}}
[probe] PARSES AS JSON.
```

**The wire format is one self-describing JSON object per TCP connection**, framed by connection
close. Not bincode, not a Rust-specific encoding. `encrypted_content` and `signature` are JSON arrays
of byte integers rather than base64 (verbose, but trivial to implement).

#### ✅ PROBE C — UDP DELIVERS END TO END. THERE IS A WORKING TRANSPORT TODAY.

**This is the most useful result in this document.** Same shape of probe, UDP instead of TCP,
receiver built through `UdpTransportFactory` with `bind_port` set:

```
INFO  UDP transport bound to 0.0.0.0:47901
[udp] sending...
DEBUG Sending UDP message to 127.0.0.1:47901
DEBUG Received 321 bytes via UDP from 127.0.0.1:57249
DEBUG Queued UDP message, total: 1
[udp] attempt 1: 1 message(s)
[udp] ***** DELIVERED ***** from=alice@synapse.local payload=UDP_PROBE_PAYLOAD
[udp] RESULT: UDP ROUND TRIP SUCCEEDED
```

**A `SecureMessage` was serialised, sent over a real loopback socket, received, queued, and returned
by `receive_messages()` with its payload intact, in about 1 ms.**

⚠️ **BUT ONLY VIA THE FACTORY, AND THE LIFECYCLE CONTRACT DIFFERS FROM TCP'S.** This is the part that
would cost someone a day:

| | what starts the receive loop |
|---|---|
| `tcp_unified` | `Transport::start()` calls `self.start_server()` — so `new()` + `start()` is the right path |
| `udp_unified` | ⚠️ **`Transport::start()` does NOT call `start_server()`.** Only `UdpTransportFactory::create_transport` does, and **only if `bind_port` is present and > 0** |

`UdpTransportImpl::new()` leaves `socket: None`, and `start()` merely logs:

```rust
// Note: We need mutable access to self to start the server
// This is a limitation of the current design - we'll work around it
info!("UDP transport ready (server will start on first use)");
```

⚠️ **"server will start on first use" is false — nothing starts it on first use.** Construct a UDP
transport the way you construct a TCP one and you get a permanently deaf transport that reports
`Running`. **The two transports have opposite construction requirements and neither is documented.**

Two further observations, recorded because they mislead:
- `receiver.status()` returned **`Stopped`** immediately after the factory had already bound the
  socket and started the loop. Status does not track the server.
- `start()` then logged *"server will start on first use"* on a transport whose server was **already
  running**. The log describes the code path, not the state.

**Limits: single process, loopback, one message, one direction.** UDP is also inherently lossy and
capped at 65507 bytes (`max_message_size` default). **This is a working link, not a reliable one.**

#### What this means for a non-Rust agent runtime

| direction | status |
|---|---|
| **Synapse → foreign process (TCP)** | ✅ **WORKS — measured.** A Python/Node/Go process opening a TCP listener receives clean parseable JSON. No Rust linkage required. |
| **foreign process → Synapse (TCP)** | 🔴 **BROKEN — measured.** Synapse's TCP listener never accepts. |
| **Synapse ↔ Synapse (UDP)** | ✅ **WORKS — measured, both directions of the round trip.** Via the factory path with `bind_port` set. |
| **foreign process ↔ Synapse (UDP)** | **Not probed**, but the receive loop is real and reads datagrams off a bound socket, so a foreign process sending the same JSON to that port is the most promising untested path. |

**There is no published wire specification, no schema, and no client library in any language** — but
the format is simple enough to implement from the capture above, and `src/types.rs::SecureMessage` is
its de facto schema. **The blocking defect is the receive path, not the protocol.**

#### The other transports — read, not run, and the speculation I first published was wrong

⚠️ **I originally wrote that `udp_unified`, `quic_unified` and `websocket_unified` "follow the same
`bind-then-spawn-and-bind-again` shape." I had not checked. One of the three does; the other two do
not, and one of them does something worse.** Corrected by reading each `start_server`:

| transport | server side | verdict |
|---|---|---|
| `tcp_unified` | binds, then re-binds inside `tokio::spawn` | 🔴 **defect MEASURED end-to-end** (above) |
| `websocket_unified` | **same shape** — binds at :904, stores it, then re-binds the same port at :931 inside the spawn | 🔴 **same defect, READ not run** |
| `udp_unified` | binds once at :72, stores `Arc<UdpSocket>`, receive path uses `self.socket` | ✅ **structurally sound** — my speculation was wrong |
| `quic_unified` | **does not bind anything** | 🔴 **simulation — see below** |
| `tcp_simple`, `http_unified` | no bind call in the file | no server side |

⚠️ **`websocket_unified` is the same bug and is more silent than the TCP one.** It stores the real
listener, then in the spawned task binds a second one on the same port and swallows the failure with
`.ok()` — so unlike `tcp_unified` there is **no error log at all**, and the code comments admit it:

```rust
tokio::spawn(async move {
    let listener = {
        // We need to move the listener out to avoid borrowing issues
        // In a real implementation, we'd keep the listener in the task
        TcpListener::bind(format!("0.0.0.0:{}", actual_port)).await.ok()   // <-- port in use
    };
    if let Some(listener) = listener { ... while let Ok(..) = listener.accept().await ... }
});
```

⚠️ **`quic_unified` performs no networking whatsoever. It is a simulation that reports success.**
`start_server()` binds nothing and returns `Ok` after logging **`"QUIC server started on {addr}"`** at
INFO. `connect_to_server()` sleeps 50 ms to "simulate connection establishment", fabricates a
`QuicConnection` with a hardcoded `rtt: Some(20ms)`, and stores it. **The file contains no reference
to `quinn` or any QUIC library** — control: `grep -n quinn src/transport/quic_unified.rs` returns
nothing, while `quinn` is present in `Cargo.lock`. Its own comments say so:

```
// In real implementation, this would:
//   1. Configure QUIC server with certificates and crypto  2. Bind to local address ...
// For simulation, just log
```

**So an operator enabling QUIC sees `QUIC transport started` in the logs and has no QUIC.** The
orphaned `src/transport/quic.rs` (740 lines, §5.3) *does* contain a real `endpoint.accept()` loop —
**the working-looking implementation is the one excluded from the build.**

**Still unmeasured end-to-end:** UDP, WebSocket, QUIC, HTTP, email. The table above is a reading of
`start_server` in each, not a round trip. **`udp_unified` being structurally sound is not a claim that
it delivers.**

**Reproducing:** the two probes are ~70 lines each and are not committed (adding a build target is
outside this pass's charter). Probe A: construct two `TcpTransportImpl` via
`TcpTransportImpl::new(&HashMap)` with `listen_port` set on one, `start()` both, `send_message` to
`127.0.0.1:<port>`, then poll `receive_messages()`. Probe B: replace the receiver with a raw
`tokio::net::TcpListener` and `read_to_end`. Run with `RUST_LOG=synapse=debug` — **without it the
causal error is invisible.**

### 2.3 What "verified" does and does not mean here

102 passing tests over a 44,562-line crate is thin, and the distribution is skewed: `security_test`
carries 44 assertions across 8 tests, while `multi_transport_integration` has 4 tests and **1**
assertion total, and `registry_integration_test` is 1 test with 1 assertion. Assertion density per file is recorded in §7. **Test count is not coverage** and no coverage run was performed.

---

## 2.4 ⚠️ CI HAS NEVER COMPILED THIS REPOSITORY — NOT ONCE, IN ITS ENTIRE HISTORY

This is the structural explanation for how everything else in this document survived unnoticed.

**The repository has had exactly three GitHub Actions runs, ever:**

```
34368757423  2026-09-09  pull_request  repair/buildable-and-inventory  failure   <- this pass
27432430342  2026-06-12  dynamic       main                            success
27432424608  2026-06-12  push          main                            failure   <- the only real run on main
```

**The only push-triggered run on `main` — at `f0f570c`, the checkpoint commit — failed at step 8 of
16, and everything that matters was skipped:**

```
 6 Install stable toolchain      success
 7 Install wasm-pack             success
 8 Check formatting              FAILURE      <- cargo fmt -- --check
 9 Clippy                        skipped
10 Build                         skipped
11 Run tests                     skipped
12 Build WebAssembly package     skipped
```

⚠️ **`Build`, `Run tests`, `Clippy` and the WASM build have never executed on this repository.** The
run on my own branch (2026-09-09) fails identically, at the same step, with the same four skips —
**two runs, three months apart, same shape.**

**Why the formatting check fails — and ⚠️ MY FIRST ANSWER WAS AN ARTIFACT OF THE INSTRUMENT.**

I originally recorded: *"`cargo fmt -- --check` flags 23 files, all in `examples/` and `tests/`, none
in `src/`."* The set was byte-identical at `f0f570c` and on the repair branch, so the red was
correctly identified as pre-existing. **But "none in `src/`" was false, and so was the portfolio
PM's independently-measured version of the same claim.** When I ran `cargo fmt` to fix it, it did not
format — it **errored**:

```
Error writing files: failed to resolve mod `wasm`: file for module found at both
  "C:\Projects\synapse\src\wasm.rs" and "C:\Projects\synapse\src\wasm\mod.rs"
```

⚠️ **`src/wasm.rs` is 0 bytes and has been since the initial release commit `33f3448` — it has never
held content at any ref** (`git cat-file -s <ref>:src/wasm.rs` → `0` at `33f3448`, `7b80ca4`,
`f0f570c`; it is 0 bytes in the published 1.1.0 crate too). **`src/wasm/mod.rs` is the real module.**
The collision made the module tree unresolvable, **so rustfmt bailed out before it ever reached
`src/`.**

**Two people measured "no `src/` files affected" and both were reading the shape of a crash.** After
deleting the empty file, `cargo fmt` touched **18 `src/` files that have never been formatted in this
repository's history** — including `config.rs`, `router.rs`, `transport/manager.rs`, and the whole of
`synapse/telemetry/`. The true figure is **44 files** (26 under `examples/`+`tests/`, 18 under
`src/`), not 23.

⚠️ **The general shape, which is the transferable part: a tool that fails early reports a SMALLER
population than the truth, and the smaller number looks like good news.** "The formatting problems
are confined to examples and tests" was a more comfortable finding than "the formatter has never
run", and nothing in the output announced the difference. This document previously recorded the
comfortable version. **The `src/wasm.rs` collision was already documented in §5.4 as a predicted
`wasm32` failure — it was in fact breaking the build pipeline on every platform, on every run, and I
did not connect the two until the formatter refused to run.**

⚠️ **The consequence is the important part. `cargo fmt` does not need dependency resolution — it
parses sources and never consults the lockfile — so it runs fine at `f0f570c`, where `cargo build`
cannot resolve at all (§1.1).** The pipeline therefore fails on *formatting* and short-circuits
before it can discover that **the crate does not build**. **The most serious defect in the repository
was invisible to CI because a cosmetic check ran first and failed.**

**This also means every "the build is green" claim in the root status documents (§6) was
uncheckable**: there has never been a CI run that got as far as compiling, and at `f0f570c` a local
build could not resolve either. ⚠️ **Nobody was ignoring a red build signal. There was no build
signal.**

**Remedy is not in this pass's scope** (it changes CI behaviour, and the 23 files are examples and
tests whose fate the merge decides), but the shape is small: the formatting failures are whitespace
and import-order drift, and moving `Check formatting` after `Build`/`Run tests` — or fixing the 23
files — would make the pipeline able to report on the thing it exists to report on.

---

## 3. ⚠️ The feature system is decorative — `native` is the only configuration that can compile

**Of the 17 declared features, exactly ONE gates any compiled code.**

Counted over `*.rs` only (`src/synapse/services/registry.rs.bak2` and `registry.rs.fixed` are
tracked in git but are not modules and compile in no configuration; they were inflating my first
count and are excluded):

| feature | `#[cfg(feature = …)]` sites in compiled code |
|---|---:|
| `mdns` | 9 (`transport/discovery.rs`, `transport/mod.rs`) |
| `wasm-logger` | 1 |
| `console_error_panic_hook` | 1 |
| `cache` `database` `email` `crypto` `auth` `http` `networking` `nat_traversal` `wasm` `telemetry` `minimal` `core` `std` `auto_discovery` | **0 each** |

**No module declaration anywhere in the crate carries a feature gate.** The only `cfg` on a `mod` is
`#[cfg(target_arch = "wasm32")] pub mod wasm;`. So `--features minimal` compiles exactly the same
source as `--features native` — it just removes the dependencies that source needs.

Positive control: `src/synapse/storage/mod.rs:4` is a bare `pub mod database;`, and
`src/synapse/storage/database.rs:6` is an unconditional `use sqlx::{PgPool, Row};`. Consequently
`--features minimal` produced 118 compile errors, 103 of them E0432/E0433 "unresolved import / crate
not linked", concentrated in `storage/database.rs` (42), `synapse/auth/mod.rs` (17),
`transport/llm_discovery.rs` (12), `synapse/auth/utils.rs` (12).

**Implication for the merge:** the published feature matrix is fiction. Anyone who does
`synapse = { version = "1.1", default-features = false, features = ["minimal"] }` gets a crate that
cannot compile. The subsystem boundaries the features imply do not exist in the code.

---

## 4. Compiles but unexercised

Everything in this section builds under `native` and has **no test that executes it**. Judged by
absence of any reference from `tests/` — control: `security_test.rs` and `edge_case_test.rs` do
reference the crate's public API, so the query finds tests when they exist.

- **`src/synapse/blockchain/` and `services/trust_manager.rs`** — 7 modules, 3,772 lines, 64 public
  functions. **Split, not uniform.** See the retraction immediately below before quoting anything
  about this subsystem.

#### ⚠️ RETRACTION: I claimed this subsystem had "zero executing coverage." That is FALSE.

I wrote that the trust/staking path "has no executing test at all," and elsewhere that it is "a
consensus and staking implementation that has never executed — not once, in any test, on any path."
**Both statements are wrong**, and I repeated them to three other lanes and to the project owner
before the FAM lane measured it and pushed back. Corrected measurement:

```
module                       own #[test]   exercised by tests/          status
blockchain/staking.rs             9        (self)                       TESTED -- and they RUN
blockchain/block.rs               0        security_test.rs sign_block  PARTIAL
blockchain/consensus.rs           0        none                         0 of 9 pub fns
blockchain/verification.rs        0        none                         0 of 5 pub fns
blockchain/mod.rs                 0        none                         0 of 7 pub fns
services/trust_manager.rs         0        none                         0 of 15 pub fns
```

`staking.rs` has **nine unit tests that execute and pass** — stake, unstake, slash,
validator-selection-by-weight, minimum/maximum/balance rejection. Verified by name in the runner, not
inferred:

```
$ cargo test --lib -- --list | grep staking
synapse::blockchain::staking::tests::test_slash_stake_security_validation: test
synapse::blockchain::staking::tests::test_validator_selection_by_stake_weight: test
... 9 total
```

⚠️ **How I got it wrong: I looked only at `tests/` and never decomposed the 62 lib unit tests by
module.** I reported "62 passing" in §2 and then reasoned about subsystem coverage from the
integration-test directory alone, as though the two were the same population. **A test count you
report but never break down is a number you cannot reason with.** The 9 staking tests were inside my
own headline figure the whole time.

**What survives, and it is still the finding that matters for the merge:** `consensus.rs`,
`verification.rs`, `blockchain/mod.rs` and `trust_manager.rs` — **36 public functions — have no
executing test.** `TrustManager` is referenced only in `end_to_end_integration_test.rs`, which **does
not compile** (§5.5), and there only via `::new()`. ⚠️ **Constructing a type is not exercising it**;
a search for `new(` matches every type in every test and was what produced FAM's own first, inflated
pass. And `security_test.rs:232` explicitly defers the other half of `block.rs`:
`// Test verification requires proper implementation of verify_block_signature` — **signing is
exercised, verification is not.**

For that 36-function remainder the tier point stands: type-checking constrains shape, not agreement,
liveness or safety, which are the only properties a consensus mechanism exists to provide. It is not
"less verified than the transports" but "unverified in a category where the compiler cannot help."

#### ⚠️ RETIER: consensus and verification are not untested — they are UNWIRED

This is a stronger and more specific finding than "no coverage," raised by the FAM lane and verified
here independently. **A textual caller scan across all of `src/`, `tests/` and `examples/` finds 19
public functions with no call site anywhere in the repository:**

```
blockchain/verification.rs   4 of 4   verify_block, verify_transaction,
                                      quick_validate_transaction, batch_verify_transactions
blockchain/consensus.rs      8 of 8   add_validator, remove_validator, start_consensus_round,
                                      process_vote, get_consensus_state,
                                      update_validator_trust_scores,
                                      get_active_validator_count, has_minimum_validators
services/trust_manager.rs    6 of 14
blockchain/mod.rs            1 of 6
```

Positive control: `send_message` matches in 22 files, `unstake_points` in 2, `sign_block` in 1. The
scan finds callers where they exist.

**And the wiring itself confirms it.** Both engines are constructed, stored on the `Blockchain`
struct, and **never read again** — each name appears exactly three times in the whole crate:

```
consensus_engine     mod.rs:79 field decl · :91 construction · :105 struct literal   NEVER READ
verification_engine  mod.rs:82 field decl · :98 construction · :107 struct literal   NEVER READ
control: staking_manager -> 11 occurrences in the same file (a field that IS used)
```

**What actually runs when `start_consensus()` is called** is a `tokio::spawn` timer loop calling
`process_next_block`, which selects a validator by **round robin on the block number**:

```rust
// Select a validator (in production this would use consensus algorithm)
let validator_idx = previous_block.number as usize % validators.len();
```

⚠️ **The source says so itself.** There is no voting, no quorum, no validator set management — the
`ConsensusEngine` that implements those is built and never consulted.

**Be precise about the verification half, because the obvious overclaim is wrong:** blocks *are*
verified. `process_next_block` calls `new_block.verify(Some(&previous_block))` and errors out on
failure. What is unwired is the **`VerificationEngine`** — its four public functions, including
batch and transaction verification, have no caller. So verification is *present inline* and the
dedicated engine is dead.

**Consequence for this document's own scheme: `consensus.rs` and `verification.rs` do not belong in
§4 "compiles but unexercised" — they belong in §5, aspirational.** "Unexercised" implies a wired
component nobody has pointed a test at. These are unreachable from any entry point, so no test
*could* exercise them without new wiring being written first. That is a different problem with a
different cost, and it is the single most important structural fact about this subsystem.

⚠️ **Caveat, stated because it bounds the claim: the caller scan is TEXTUAL.** Dispatch through a
trait object, a macro, or reflection would not match. It is strong evidence, not proof. The
`consensus_engine` / `verification_engine` field evidence is independent of it and points the same
way.

#### The instrument exists and was switched off

`tests/trust_system_integration_test.rs.disabled` — 174 lines, 3 test functions — exercises **exactly
the uncovered surface**: `register_entity_with_stake`, `submit_trust_report`, `calculate_trust_score`,
`get_stake_info`, `start_decay_process`, `stop_decay_process`. Git history (measured by the FAM lane):
it shipped as an **active** test in `33f3448` (v1.0.0) and `7b80ca4` (v1.1.0), and was renamed to
`.disabled` in `f0f570c`, the "commit all local work before machine wipe" checkpoint.

⚠️ **So this is not a capability gap and not an instrument that was never aimed. It was aimed, it
shipped in two releases, and it was switched off during an emergency dump.** The remedy is
correspondingly small: re-enable one file and fix whatever it reports. **Deliberately not done here**
— re-enabling a test changes what the suite asserts, and I was asked to add nothing — but it is the
highest-value single action available in this subsystem.

⚠️ **One caveat that keeps this honest: on `main` NOTHING in the crate executes, because the crate
does not build there (§1). All coverage statements in this document are measured on the repaired
branch. "Zero executing coverage on main" is true of every module in Synapse and is a BUILD fact, not
a coverage fact** — quoting it about the trust layer specifically would make it look uniquely bad
when it was merely part of a crate that did not compile.

**For the merge:** FAM brings a trust model (vouchers and revocations, sequence-based
order-independent resolution) that is tested and mutation-proven. The two models cannot both survive,
and the choice should be made on their merits — **not settled by which repository supplies the
project's name.**
- **`src/synapse/storage/database.rs` / `cache.rs` / `migrations.rs`** — compiles against `sqlx` and
  `redis`. No test connects to Postgres or Redis; `dev-dependencies` pull in `sqlx` with `sqlite`,
  which no test file uses.
- **`src/email_server/`** — `smtp_server.rs`, `imap_server.rs`, `auth.rs`, `connectivity.rs`.
  Compiles. `email_server_demo` example compiles. No test binds a port or speaks SMTP/IMAP.
- **`src/transport/nat_traversal.rs`** — compiles (STUN/UPnP via `stun_codec`, `igd`). Exercised only
  by `examples/real_nat_traversal_test.rs`, which **does not compile** (see §5).
- **`src/synapse/api/`** — `participant_api.rs`, `trust_api.rs`, `discovery_api.rs`. No test.
- **`src/streaming.rs`, `src/monitoring.rs`, `src/circuit_breaker.rs`** — compile; `circuit_breaker`
  has a demo example but no test target.

---

## 5. Aspirational — does not compile, is unreachable, or is not in the build at all

⚠️ **Also in this tier, documented in §4 where the evidence sits:** `blockchain/consensus.rs` (8 of 8
public functions) and `blockchain/verification.rs` (4 of 4) are **unwired** — no call site anywhere,
and their engines are constructed and never read. They are not "untested"; they are unreachable from
any entry point, so no test could exercise them without new wiring being written first.


### 5.1 `src/auth_integration_enhanced.rs` — written against an API that has never existed

828 lines. Now gated behind `enhanced-auth` (previously compiled unconditionally, which is what made
the *default* build fail). **`--features enhanced-auth` does not compile.** Its `use auth_framework::{…}`
block imports nine items that exist in no published auth-framework:

```
OAuthTokenResponse · TokenToProfile · audit::{AuditEvent, AuditLogger}
compliance::{ComplianceLevel, ComplianceReporter} · config::{AuthFrameworkConfigManager, ConfigManager}
device_flow::{DeviceFlowConfig, DeviceFlowManager} · methods::AuthMethodEnum · storage::PostgreSqlStorage
```

plus four API-shape errors past the imports (`AuthConfig::enable_audit_logging` — no such method;
`&Vec<String>` vs `Vec<String>`; `AuthToken` vs `Box<AuthToken>`; `Credential::enhanced_device_flow` —
no such variant).

⚠️ **The three phantom feature names in §1.1 map one-to-one onto these phantom imports**
(`config-management` → `config::ConfigManager`, `enterprise-features` → `audit`/`compliance`,
`token-to-profile` → `TokenToProfile`). The manifest and the module were written against the same
anticipated auth-framework, and that API was never published. This is the single measurement that
best characterises the project's documentation-to-code gap.

**Not deleted.** Left in-tree, gated, for the merge to dispose of.

#### ⚠️ What the published crate proves about when this happened

I pulled the real `synapse-1.1.0.crate` from `static.crates.io` and extracted it. The published
artifact and this git tree are **different artifacts under one version string**, and the difference
is exactly this module:

| | published `synapse 1.1.0` | git tree @ `f0f570c` |
|---|---|---|
| `src/auth_integration_enhanced.rs` | **not shipped — file absent** | present, 828 lines, live |
| `src/auth_integration.rs` (715 lines) | **live**: `lib.rs:264 pub mod auth_integration;` | **orphaned** — no `mod` declares it |
| `auth-framework` features requested | 2 (`oauth-device-flows`, `enhanced-device-flow`) | 5 (3 of them phantom) |
| `cargo tree --features auth` | **resolves, EXIT 0** | **EXIT 101** |

So the sequence is legible: at publish time the crate used `auth_integration.rs` and asked for two
features that exist. **Afterwards**, `auth_integration.rs` was dropped from `lib.rs` (not deleted —
just unreferenced), `auth_integration_enhanced.rs` was written against an anticipated
auth-framework, and three matching phantom feature names were added to the manifest. The regression
is post-publish, and `cargo publish` never validated the tree that is on `main`.

**Correction to a claim in circulation:** it is *not* the case that the published version also fails
to resolve, nor that `oauth-device-flows` is missing from auth-framework 0.3.0. Measured:
`oauth-device-flows` **is** available in 0.3.0 as the implicit feature of auth-framework's optional
dependency of that name — `cargo` lists it, and the published tree resolves with it, exit 0. It is
also not the case that `auth` is a non-default feature: `default = ["native"]` and `native` includes
`auth`, so the broken path was the default one. (Reading the index's `features` map alone hides
implicit optional-dependency features; that is the trap here.)

**Still unmeasured:** whether the published `auth_integration.rs` *compiles* against auth-framework
0.3.0. The published tree's `--features auth` build fails first in `redis 0.24.0` (§9), a transitive
dependency, before reaching any Synapse code. So "the published auth worked" is **not** established
either — only that it resolved.

### 5.2 The `enhanced-auth` feature gated nothing, while a README told users to enable it

`Cargo.toml` declared `enhanced-auth = []` — empty, referenced by no `cfg` anywhere — while
`src/synapse/auth/README.md:21` instructs `cargo build --features enhanced-auth`. That command was a
no-op. It is now wired to §5.1 and documented as known-broken.

### 5.3 17 source files — 8,062 lines — are in no configuration's module tree

Not orphaned "modules"; **files that no `mod` declaration ever names**, so they compile never.
Control: `mod tcp_unified;`, `mod udp_unified;`, `mod websocket_unified;`, `mod crypto;` each return
exactly 1 declaration; the files below return 0.

| file | lines | note |
|---|---:|---|
| `src/transport/nat_traversal_clean.rs` | 1159 | |
| `src/transport/quic.rs` | 740 | **QUIC transport — no QUIC in any build** |
| `src/auth_integration.rs` | 715 | ⚠️ **see note below — this is the file that shipped** |
| `src/transport/email_unified.rs` | 710 | |
| `src/transport/websocket.rs` | 588 | |
| `src/wasm/storage.rs` | 572 | |
| `src/wasm/websocket.rs` | 450 | |
| `src/wasm/webrtc.rs` | 443 | |
| `src/wasm/browser.rs` | 442 | |
| `src/transport/tcp.rs` | 434 | |
| `src/wasm/crypto.rs` | 396 | |
| `src/wasm/worker.rs` | 389 | |
| `src/transport/tcp_enhanced.rs` | 334 | imported by a demo that therefore fails — §5.5 |
| `src/transport/udp.rs` | 320 | |
| `src/transport/email_enhanced.rs` | 313 | imported by a demo that therefore fails — §5.5 |
| `src/transport/discovery_test.rs` | 57 | |
| `src/types_new.rs` | 0 | empty |

⚠️ **`src/auth_integration.rs` is not just another orphan, and the reason is a selection criterion
worth stating explicitly because it is easy to lose.** It is the module the **published** crate
declares (`lib.rs:264`), so it is **the only Synapse auth code that plausibly ever compiled against a
real auth-framework.** The currently-imported module, `auth_integration_enhanced.rs`, never did (§5.1).
So if auth survives the merge, *this* is the file to map forward to a current auth-framework —
**"the file that actually shipped" is a better selection criterion than "the file that is currently
imported,"** and the two point at different files here. Retrieve with
`git show f0f570c:src/auth_integration.rs`.

### 5.4 WASM is a 26-line stub, and would not compile on `wasm32` if attempted

`src/wasm/mod.rs` is 26 lines: it declares `pub mod simple;` (109 lines) and a non-WASM `stub` module
whose `WasmSynapseNode::new()` returns `Err("WebAssembly support only available in WASM builds")`.
The six real WASM implementation files (2,692 lines — browser, crypto, storage, webrtc, websocket,
worker) are in §5.3: unreachable.

⚠️ Additionally **`src/wasm.rs` (0 bytes) and `src/wasm/mod.rs` both exist.** That is a hard rustc
error (`file for module 'wasm' found at both …`) on any `wasm32` target — **and, discovered later,
it also broke `cargo fmt` on EVERY platform, which is why CI never reached its build step (§2.4).
`src/wasm.rs` is removed on this branch.** I recorded this defect as a *prediction about wasm32*
before realising it was already causing a *measured* failure on the host, in the pipeline, on every
run. **A defect scoped to a platform nobody builds for was the one blocking the platform everybody
builds on.** `Cargo.toml` declares
`crate-type = ["cdylib", "rlib"]`, a `wasm` feature, `Cargo-wasm.toml`, `build-wasm.sh` and
`build-wasm.bat`. **No wasm32 build was attempted in this pass** — recorded as unmeasured, not as
working.

### 5.5 Targets that do not build

- `examples/multi_transport_circuit_breaker_demo.rs` — imports
  `synapse::transport::{EnhancedTcpTransport, EmailEnhancedTransport}`, which live in the
  **unreachable** files `tcp_enhanced.rs` / `email_enhanced.rs`. 6 errors.
- `examples/real_nat_traversal_test.rs` — `TransportTarget` has no field `metadata`. 2 errors.
  (Not declared in `Cargo.toml`; only reached via `--examples`.)
- `tests/comprehensive_feature_test.rs` — **137 errors** (`SimpleMessage`, `MessageType`,
  `SecurityLevel` not in scope). 12 tests, 31 assertions, none of which have ever run.
- `tests/end_to_end_integration_test.rs` — 44 errors (unresolved `synapse::identity::{Identity,
  KeyPair}`, `synapse::transport::{TransportError, TransportResult, MessageDeliveryStatus}`; and
  `get_id`/`send` declared as members of trait `Transport` that the trait does not have).
- `tests/browser_compatibility_test.rs` — 23 errors. It is a **wasm32-only** file (7
  `#[wasm_bindgen_test]`) that is not gated to `cfg(target_arch = "wasm32")`, so the host test run
  tries to compile it. See §7.

### 5.6 Empty and disabled files that read as coverage

Four test files are **0 bytes** — they appear in `ls tests/` and in any file-count, and contain
nothing:
`high_load_test_new.rs`, `multi_transport_integration_new.rs`, `registry_integration_test_new.rs`,
`webrtc_transport_integration_test.rs`.

Two are renamed out of the build: `trust_system_integration_test.rs.disabled` (the only
trust-system test) and `webrtc_transport_integration_test.rs.disabled`.

Also 0 bytes at repo root: `WINDOWS_LINKER_WORKAROUND.md` (which the README links to as a "detailed
guide"), `build-debug.ps1`, `test-build.ps1`.

`examples/` holds 33 `.rs` files; `Cargo.toml` declares 15. The other 18 are built only by
`--examples` and are unmeasured.

---

## 6. The README asserts properties the code does not have

Not a stylistic complaint — these are specific, checkable claims:

| README claim | measured |
|---|---|
| `auth_manager.authenticate_ai_agent_webauthn(...)` (§"AI-Native Authentication") | **no such symbol in the crate.** Control: `EnhancedSynapseRouter` returns 4 hits. |
| `auth_manager.authenticate_saml_enterprise(...)`, "SAML 2.0 Corporate SSO" | **no such symbol.** No SAML implementation exists. |
| "🔐 WebAuthn Passwordless: AI agents authenticate using hardware security keys" | no WebAuthn code, no WebAuthn dependency in `Cargo.toml`. |
| "📋 Advanced Audit Trails: Complete compliance logging for GDPR, HIPAA, SOX" | the only `audit`/`compliance` references are the **phantom imports** of §5.1. |
| "The first and only authentication system designed specifically for AI agents" | the auth integration module does not compile. |
| Windows §: "This **has been resolved** using LLVM's `lld-link`" | that config is what made the repo unbuildable on a stock toolchain (§1.2). Links to `docs/WINDOWS_LINKER_WORKAROUND.md`; the root copy is **0 bytes**. |

Eight root-level `*_COMPLETE.md` / `*_AUDIT_*.md` / `*_FINAL.md` / `*_SUMMARY.md` reports assert
completion of work that, at `f0f570c`, could not be compiled. **No claim in any of them was treated
as evidence for this inventory.**

### 6.1 ⚠️ `AUTH_FRAMEWORK_INTEGRATION_SUMMARY.md` — a success frame around the change that broke the build

This distinction matters for the remedy: a document that was true and drifted deserves a supersession
banner; one that was never true is a different and worse defect. This one is the latter, and it can
be shown rather than argued.

The document is titled **"🚀 Integration Complete"** and states, as accomplished work:

> *"Updated from auth-framework v0.3 to v0.3.0 with latest features. Added new feature flags:
> `config-management`, `enterprise-features`, `token-to-profile`"*

**Those three feature flags have never existed in any published version of auth-framework** (§1.1 —
all nine versions, 0.1.1 through 0.5.0-rc19; and absent from the crate's unpublished `main` at rc24
per the auth-framework lane's own check with controls).

⚠️ **Precision matters here, and my first wording overreached.** I originally wrote "there is no
point in time at which this paragraph was true." That is wrong as stated: the sentence *"Added new
feature flags: config-management, enterprise-features, token-to-profile"* is **literally accurate** —
those three names really were added to `Cargo.toml`. **The document describes the edit correctly.
What is false is the frame: that the edit constitutes an "Integration Complete", an upgrade "to
latest version with latest features", and a working state.** The edit it accurately reports is
precisely the edit that made the crate unresolvable in every configuration (§1.1). Credit to the
Claim Auditor lane for the distinction — *the change is described faithfully and the change is what
breaks the build* is a sharper and more defensible finding than "the document is false."

So the defect is not a false report of an action; it is **a success frame wrapped around an action
that failed**, published in the same commit as the breakage.

Two further internal tells, independent of anything outside the file:

- *"Updated **from auth-framework v0.3 to v0.3.0**"* — those are the same requirement. The document's
  own headline upgrade is a non-event.
- Its "Next Steps" instructs `cargo run --example auth_framework_v3_demo --features auth,core`.
  **That command could never have run**, for the same reason: `--features auth` did not resolve.

It also lists `src/auth_integration_enhanced.rs` under "Files Created" — the module that has never
compiled (§5.1). **The document, the module, and the three phantom feature names are one artifact
written against an imagined API**, and the document asserts all three as finished.

The remaining seven root reports were not individually adjudicated; this one was, because §1.1 gave
it a decisive test. **Do not generalise this verdict to the others** — they may well be the ordinary
historical kind.

---

## 7. Assertion density (for the merge's triage)

Host tests (`#[test]` / `#[tokio::test]`, including attribute forms with arguments), WASM tests
(`#[wasm_bindgen_test]`), and `assert*!` count per file:

```
FILE                                 host  wasm  asserts   status
security_test.rs                        8     0       44   runs, all pass
comprehensive_feature_test.rs          12     0       31   DOES NOT COMPILE
end_to_end_integration_test.rs          1     0       13   DOES NOT COMPILE
edge_case_test.rs                       8     0       10   runs, all pass
basic_integration_test.rs               4     0        7   runs, all pass
browser_compatibility_test.rs           0     7        7   DOES NOT COMPILE (host) -- see below
transport_error_handling_test.rs        3     0        5   runs, 1 FAILING
concurrent_registry_access_test.rs      4     0        4   runs, all pass
high_load_test.rs                       2     0        4   runs, all pass
network_partition_test.rs               3     0        2   runs, all pass
multi_transport_integration.rs          4     0        1   runs, all pass
integration_test.rs                     3     0        1   runs, all pass
registry_integration_test.rs            1     0        1   runs, all pass
                                     ----  ----
                                       53     7
```

`browser_compatibility_test.rs` is a **wasm32-only** test file: its 7 tests are
`#[wasm_bindgen_test]` and it uses `#[wasm_bindgen]`. It is not gated to `cfg(target_arch =
"wasm32")` and carries no `wasm-bindgen-test` import path that resolves on the host, so `cargo test`
on this machine tries to compile it and fails with 23 errors (`cannot find attribute
wasm_bindgen_test in this scope`). **Its 7 tests have never run here and would need a wasm32 test
runner.** That is a target-gating defect, not broken logic.

⚠️ **A correction to my own first pass, recorded because the wrong version is the more alarming
one.** I initially counted test attributes with a regex that required `#[test]`/`#[tokio::test]` to
be followed by `]`, which silently missed the argument form. It reported `high_load_test.rs` as
having *4 assertions and zero test attributes* — i.e. a file that can never fail. **That was false.**
`high_load_test.rs` has two real tests:

```
tests/high_load_test.rs:9   #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
tests/high_load_test.rs:87  #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
```

which is exactly the "running 2 tests" `cargo test` reported for that target. The tell that something
was wrong was the mismatch between my static count (0) and the runtime count (2); a static count with
no runtime number beside it would have shipped the false claim. **Prefer `cargo test`'s own per-target
counts over any grep.**

---

## 8. Changes made in this pass

Four edits, all narrow, none adding a feature or changing behaviour:

1. `Cargo.toml` — removed 3 never-existent `auth-framework` features. **Unblocked all 10 configs.**
2. `.cargo/config.toml` — commented out the forced `lld-link`, reasoning preserved in-file.
3. `src/lib.rs` + `Cargo.toml` — gated `auth_integration_enhanced` behind the pre-existing (and
   previously inert) `enhanced-auth` feature; marked it known-broken in the manifest.
4. `src/auth_integration_enhanced.rs` — header rewritten to state, with the measurement, that it has
   never compiled against a real auth-framework. **No code changed.**

Deliberately **not** done, because the merge decides: the `auth-framework` 0.3 → 0.4/0.5 upgrade; the
failing `test_transport_error_handling`; the 17 unreachable files; the 3 non-compiling test targets;
the WASM double-module; the README's claims; the `.bak2`/`.fixed` files tracked in `src/`.

---

## 9. Known-unmeasured

Stated so nobody reads silence as a pass:

- No `wasm32-unknown-unknown` build attempted (§5.4 predicts a hard failure).
- No coverage instrumentation. No clippy run. No `cargo audit` / advisory scan.
- The 18 undeclared files in `examples/` were not compiled.
- `cargo doc` not run; the crate-level doctests in `src/lib.rs` were exercised only insofar as
  `cargo test --lib` ran them — the large doc examples there are mostly commented out internally.
- No behaviour was verified against a live network, SMTP/IMAP server, Postgres, or Redis.
- Transitive-dependency health: `net2 v0.2.39` is flagged future-incompatible by cargo.

- ⚠️ **`redis 0.24.0` vs `0.24.1` — a one-patch-version difference decides whether the auth path
  builds, and my first explanation of it was wrong.** `auth-framework 0.3.0` has
  `default = ["redis-storage"]` and Synapse does not pass `default-features = false`, so enabling
  `auth` pulls a `redis ^0.24` **in addition to** the `redis 0.32.x` that the `cache` feature
  selects — the ranges are disjoint, so cargo builds *both*. Measured:

  ```
  git tree @ f0f570c, --features native : redis v0.24.1  +  redis v0.32.7   -> COMPILES (lib EXIT 0)
  published synapse 1.1.0, --features auth : redis v0.24.0                  -> FAILS
      redis-0.24.0/src/script.rs:{150,176}  4x error[E0277] `!: FromRedisValue`  (never-type fallback)
  ```

  **`0.24.0` does not compile on rustc 1.100.0-nightly; `0.24.1` does.** The published crate ships a
  `Cargo.lock` pinning `0.24.0`, which is why the published tree fails there and this tree does not.

  I earlier recorded the cause as "`redis 0.32.4` is selected via the `cache` feature." **That was an
  inference and it was wrong** — both versions are in the graph, so the `cache` feature does not
  displace anything. The FAM lane flagged the discrepancy; the actual discriminator is the patch
  version in the lockfile.

  ⚠️ **And the failure is UNREACHABLE FOR DEPENDENTS, which is stronger than "lockfile-aged."**
  Measured by the FAM lane on a scratch crate depending on `synapse = "1.1.0"`: `cargo tree -i redis`
  reports `redis@0.24.1` + `redis@0.32.7` — not `0.24.0`. Control: `cargo tree -i auth-framework`
  resolves the chain `auth-framework 0.3.0 -> synapse 1.1.0 -> probe`. **A crate's bundled
  `Cargo.lock` is authoritative only when it is the top-level package; as a dependency it is
  ignored.** So `0.24.0` is reachable only by extracting the `.crate` and building it standalone —
  which is what I did. **No consumer of synapse 1.1.0 has ever seen this failure**, and it is not
  evidence against auth-framework 0.3.0 at all.

  **Isolated control**, built by the auth-framework lane on a two-file scratch crate — no Synapse
  tree involved, same toolchain, same features auth-framework 0.3.0 requests (`aio`, `tokio-comp`),
  only the version requirement differing:

  ```
  redis = { version = "0.24",   features = ["aio","tokio-comp"] }  -> resolves 0.24.1  EXIT 0
  redis = { version = "=0.24.0", features = ["aio","tokio-comp"] } -> 4x E0277         EXIT 101
  ```

  The control fires under one hypothesis and not the other, and the error signature matches the one
  seen here exactly. auth-framework 0.3.0 declares `redis = "^0.24"`, which admits `0.24.1`.

  **The transferable rule: "does the published artifact build?" is two questions, not one, and the
  answers can disagree.** Built standalone, its bundled lock decides; built as a dependency, the
  top-level resolve decides and its lock is discarded. Testing a published crate by extracting and
  building it measures a configuration that exists only in the tester's directory.
