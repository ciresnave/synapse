// SPDX-License-Identifier: MIT OR Apache-2.0
//! Value wrappers the wire types carry. Their JSON form is the wire format; nothing here depends on a
//! binary codec. They lived in `synapse::blockchain::serialization`, which still re-exports them,
//! until the core dropped its unmaintained binary codec (RUSTSEC-2025-0141; see the 2026-10-07
//! removal plan under `docs/superpowers/plans/`).

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateTimeWrapper(pub chrono::DateTime<chrono::Utc>);

impl Default for DateTimeWrapper {
    fn default() -> Self {
        DateTimeWrapper(chrono::Utc::now())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UuidWrapper(pub Uuid);

impl fmt::Display for UuidWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl DateTimeWrapper {
    pub fn new(dt: chrono::DateTime<chrono::Utc>) -> Self {
        DateTimeWrapper(dt)
    }

    pub fn into_inner(self) -> chrono::DateTime<chrono::Utc> {
        self.0
    }
}

impl UuidWrapper {
    pub fn new(uuid: Uuid) -> Self {
        UuidWrapper(uuid)
    }

    pub fn into_inner(self) -> Uuid {
        self.0
    }
}
