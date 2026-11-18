//! foldz's [`ReplicatedState`]: one replica and no runtime state. foldz
//! takes no commands, so the state stays at revision 0 and every submit is
//! refused.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;

use axum::body::Bytes;
use futures_util::future::BoxFuture;
use samplez_service::replica::{Ack, NotApplied, Reason, Resolution};
use samplez_service::ws::{Delta, Snapshot};
use samplez_service::{Owner, Position, ReadIndex, ReplicaInfo, ReplicatedState};

pub struct Standalone {
    id: String,
    epoch: u64,
}

impl Standalone {
    /// A replica named `id`, with an epoch drawn at random: the state
    /// starts empty on every start.
    pub fn new(id: String) -> Standalone {
        Standalone {
            id,
            epoch: RandomState::new().hash_one(0u8),
        }
    }
}

impl ReplicatedState for Standalone {
    fn replica(&self) -> ReplicaInfo {
        ReplicaInfo {
            id: self.id.clone(),
            role: "standalone".into(),
            leader: None,
            in_sync: true,
            lag: Some(0),
            ready: true,
            commitable: true,
        }
    }

    fn epoch(&self) -> u64 {
        self.epoch
    }

    fn revision(&self) -> u64 {
        0
    }

    fn read_index(&self) -> BoxFuture<'_, ReadIndex> {
        Box::pin(async { ReadIndex::Confirmed(0) })
    }

    fn applied(&self, _revision: u64) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }

    fn snapshot(&self) -> BoxFuture<'_, Snapshot> {
        let snapshot = Snapshot {
            epoch: self.epoch,
            revision: 0,
            text: "{}".into(),
        };
        Box::pin(async move { snapshot })
    }

    fn deltas_since(&self, since: Position) -> BoxFuture<'_, Option<Vec<Delta>>> {
        let deltas = (since.epoch == self.epoch).then(Vec::new);
        Box::pin(async move { deltas })
    }

    fn command_resolution(&self, _owner: &Owner, _id: &str) -> Option<(u64, Resolution)> {
        None
    }

    fn submit(
        &self,
        _owner: &Owner,
        _id: &str,
        _command: Bytes,
    ) -> BoxFuture<'_, Result<Ack, NotApplied>> {
        Box::pin(async {
            Err(NotApplied {
                reason: Reason::Invalid,
                detail: "foldz takes no commands".into(),
            })
        })
    }

    fn renew_sessions(&self, _live: &[Owner]) {}
}
