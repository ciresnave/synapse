# Receiver acknowledgement — design (P2 slice b)

**Status:** design for review, written 2026-09-17. It is stacked on `feat/sender-authentication`
(PR #37), which is held until CireSnave answers #33 §11 Q1, so this slice is held too. If that answer
changes slice a, this branch is rebased.

**Already decided by the PM (2026-09-17):**
- Acknowledgement is **explicit only**: the receiving application acknowledges after it has processed
  the message.
- The never-constructed `DeliveryConfirmation::Received` variant is removed.
- The ack goes to a **signed reply-to address** that the sender provides.
- An ack carries its own `sender_proof` (slice a).
- `acknowledge()` **refuses any message whose verdict is not `Verified`** and sends nothing.
- Acknowledging a message twice is idempotent.
- The six structural points in §4–§6 below are accepted.

---

## 1. The requirement

From FAM's handover (`DESIGN-SYNAPSE-HANDOVER.md` §4a, FAM `5111f2b2`), measured there:

> the receiver acknowledges, and the transport must give it something to acknowledge WITH. FAM acked
> on the sender's side because the transport gave it nothing better.

And FAM's own server contract: *"the client must acknowledge … after processing."* FAM lost messages
because it acked when a notification was written to a pipe that nobody read. The claude-peers broker
has the same flaw: `poll-messages` marks a message delivered as it returns it.

**What `main` does today** (inventory §2.2):
- every `DeliveryConfirmation` is built on the sending side, straight after a local write;
- `Received` and `Acknowledged` are never constructed anywhere;
- the QUIC simulation reports `Delivered` without doing any networking.

## 2. Scope

**In:**
- the reply-to and ack metadata keys;
- building and checking acks (a new module, `src/delivery_ack.rs`);
- `TransportManager::acknowledge`;
- sender-side tracking and `delivery_status`;
- removing `DeliveryConfirmation::Received`.

**Out:**
- retransmission and reliability. An ack is evidence of processing, not a delivery guarantee;
- persistence of delivery status across restarts;
- eviction of old tracking entries (a known limit; slice e's seen-window is the natural place for it);
- `SynapseRouter` (email) and the raw per-transport receive paths, which are unchanged.

## 3. How it flows

```
sender (alice)                                   receiver (bob)
  m = SecureMessage::new(to=bob, from=alice)
  m.request_ack("127.0.0.1:47001")   // before signing
  crypto.sign_secure_message(&mut m)
  manager.send_message(target, &m)   -> status(m) = Sent
                                           manager.receive_messages() -> ReceivedMessage{m, Verified}
                                           ...application processes m...
                                           manager.acknowledge(&received, &bob_crypto)
                                             refuses unless Verified and reply_to is present
                                             ack = {to=alice, from=bob, ack.for=m.id,
                                                    ack.digest=H(canonical_input(m))}, signed by bob
                                             sends ack to reply_to
  manager.receive_messages()
    takes the ack out of the results; checks it; status(m) = Acknowledged
```

## 4. Wire format — reserved metadata keys, and no structural change

| key | on | value |
|---|---|---|
| `synapse.reply_to` | an original message that wants an ack | a transport address, in the form `TransportTarget.address` takes (e.g. `127.0.0.1:47001`) |
| `synapse.ack.for` | an ack | the original `message_id`, as its UUID string |
| `synapse.ack.digest` | an ack | lowercase hex SHA-256 of `canonical_input(original)` (slice a, §4) |

Slice a's signature covers all metadata, so a relay cannot change `reply_to` or an ack's fields
without the verdict becoming `Contradicted`. **`request_ack` must be called before signing.**
Calling it afterwards changes a signed field, and the receiver sees `Contradicted(BadSignature)`;
§8 test 10 checks this.

**An ack** is a `SecureMessage` with:
- `to_global_id` set to the original's `from_global_id`, and `from_global_id` set to the original's
  `to_global_id`;
- empty `encrypted_content`;
- `security_level: Authenticated`;
- the two `synapse.ack.*` keys;
- the receiver's `sender_proof`.

An ack never carries `synapse.reply_to`, and acks are never acknowledged.

**The digest binds the ack to exactly what was processed.** A matching `message_id` alone would not
show the receiver processed the same content.

## 5. Receiver side — `acknowledge`

```rust
impl TransportManager {
    pub async fn acknowledge(
        &self,
        received: &ReceivedMessage,
        signer: &CryptoManager,
    ) -> Result<DeliveryReceipt>;
}
```

It refuses, sending nothing and returning `Err`, when any of these holds:

| # | condition | error |
|---|---|---|
| 1 | `received.sender` is not `Verified` | `SynapseError::AuthenticationError` — the PM's anti-reflector rule |
| 2 | the message is itself an ack | `SynapseError::InvalidMessageFormat`. Unreachable through `receive_messages`, which never returns acks, but `ReceivedMessage` can be built by hand |
| 3 | no `synapse.reply_to` | `SynapseError::InvalidMessageFormat` |
| 4 | `signer` has no key pair | the signing error from slice a |

Otherwise it builds the ack, signs it, and sends it to
`TransportTarget::new(original.from_global_id).with_address(reply_to)`. It returns the send receipt.

**Idempotency, both sides.**
- **Receiver:** calling `acknowledge` again for the same message is allowed and sends another ack.
  That is deliberate: on a lossy transport, re-acknowledging is the only recovery, since retransmission
  is out of scope.
- **Sender:** a second valid ack for a message that is already `Acknowledged` changes nothing.
- §8 test 3 checks both sides together.

## 6. Sender side — tracking and `delivery_status`

```rust
impl TransportManager {
    pub async fn delivery_status(&self, message_id: &str) -> Option<DeliveryConfirmation>;
}
```

- **Tracking.** After a successful send, `send_message` records any message that carries
  `synapse.reply_to`, keyed by its `message_id` string. The record holds:
  - the original's `to_global_id`;
  - the original's digest, `H(canonical_input(m))`, computed at send time;
  - the status, `Sent`.

  Messages without `reply_to` are not tracked, and `delivery_status` returns `None` for them.
- **Taking acks out.** `receive_messages` removes every message that carries `synapse.ack.for` from
  its results, whether valid or not. **Acks are control traffic and never reach the application.**
- **Upgrading.** A removed ack upgrades its tracked message to `Acknowledged` only if **all** of these
  hold:
  1. its verdict is `Verified`, which needs the receiver's key pinned in this manager's `TrustStore`;
  2. `synapse.ack.for` names a tracked message;
  3. the ack's `from_global_id` equals that message's `to_global_id`;
  4. `synapse.ack.digest` equals the recorded digest.

  Anything else is dropped with a `debug!` naming the failed condition. **An unverified or
  mismatched ack never changes a status.**
- **Pumping.** Status only changes while the application keeps calling `receive_messages`, because
  the manager has no background receive loop. This is documented on `delivery_status`.

## 7. `DeliveryConfirmation`

- `Received` is **removed**; nothing constructs or matches it.
- `Acknowledged` is documented as: *"the receiving application acknowledged the message with a
  verified, signed ack whose digest matches what was sent (P2 slice b)"*.
- `Sent` and `Delivered` are unchanged. The QUIC simulation's `Delivered` claim stays recorded in the
  inventory and is not touched here.

## 8. Tests

New file `tests/receiver_acknowledgement.rs`; test 1 is a unit test in `src/delivery_ack.rs`.

1. **Building an ack** (unit): the ids are swapped, both ack keys are present, the digest is the
   original's, the content is empty, there is no `reply_to`, and the ack is signed by the signer.
2. **End to end over UDP:** alice pins bob, bob pins alice, and each has a `TransportManager` bound to
   its own port.
   - Alice sends a signed message with `reply_to` set to her address. Its status is `Sent`.
   - Bob receives it as `Verified` and calls `acknowledge`.
   - Alice's `receive_messages` returns **no** messages (the ack was taken out), and the status is now
     `Acknowledged`.
3. **Idempotent:** bob acknowledges twice and both calls return `Ok`. After alice pumps, the status is
   `Acknowledged`, alice's application got no messages, and nothing errors.
4. **No reflection** (PM requirement):
   - Bob receives a message whose verdict is `Unverifiable`, because alice is not pinned at bob.
   - `acknowledge` returns `AuthenticationError`.
   - A raw UDP socket bound to the message's `reply_to` receives **nothing** within 500 ms.
   - Control: the same socket does receive an ack for a `Verified` message in the same run.
5. **A forged ack** (signed by mallory, whom alice has not pinned) is taken out, and the status stays
   `Sent`.
6. **A wrong digest** (a valid bob signature over the digest of different content) leaves the status
   at `Sent`.
7. **A wrong acker:** carol is pinned at alice, but the original was addressed to bob. Carol's
   correctly signed ack leaves the status at `Sent`.
8. **An unknown `ack.for`** is taken out, ignored, and changes nothing.
9. **No `reply_to`:** `acknowledge` returns `InvalidMessageFormat`, and nothing is sent. Same control
   as test 4.
10. **`request_ack` after signing** makes the receiver's verdict `Contradicted(BadSignature)`. This
    shows that `reply_to` is covered by the signature.
11. **Mutation check, run once and reported in the PR:** remove condition 4 (the digest comparison).
    Test 6 must fail, and nothing else may.

The failing-test set must stay `{test_transport_error_handling}`.

## 9. Risks

- **UDP loss and the `try_lock` drop in `udp_unified.rs`.** An ack can be lost. Test 2 polls, and the
  application can re-acknowledge (§5).
- **Unbounded tracking.** Each ack-requesting send adds one entry that is never evicted. Acceptable for
  this slice; eviction belongs with slice e.
- **The sender must pin the receiver's key** to accept acks. That is by design, and the same
  pinned-key model as slice a.
