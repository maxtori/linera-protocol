// Copyright (c) Zefchain Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use async_graphql::{InputObject, Request, Response, SimpleObject};
use linera_sdk::base::{ChainId, ContractAbi, ServiceAbi, Timestamp};
use linera_views::{common::CustomSerialize, views};
use serde::{Deserialize, Serialize};

// TODO(#768): Remove the derive macros.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub struct SocialAbi;

impl ContractAbi for SocialAbi {
    type InitializationArgument = ();
    type Parameters = ();
    type Operation = Operation;
    type ApplicationCall = ();
    type Effect = Effect;
    type SessionCall = ();
    type Response = ();
    type SessionState = ();
}

impl ServiceAbi for SocialAbi {
    type Query = Request;
    type QueryResponse = Response;
    type Parameters = ();
}

/// An operation that can be executed by the application.
#[derive(Debug, Serialize, Deserialize)]
pub enum Operation {
    /// Request to be subscribed to another chain.
    RequestSubscribe(ChainId),
    /// Request to be unsubscribed from another chain.
    RequestUnsubscribe(ChainId),
    /// Send a new post to everyone who subscribed to us.
    Post(String),
}

/// An effect of the application on one chain, to be handled on another chain.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    /// The origin chain wants to subscribe to the target chain.
    RequestSubscribe,
    /// The origin chain wants to unsubscribe from the target chain.
    RequestUnsubscribe,
    /// The origin chain made a post, and the target chain is subscribed.
    /// This includes the most recent posts in reverse order, and the total count of posts by the
    /// sender. I.e. the indices of the posts in the `Vec` are `count - 1, count - 2, ...`.
    Posts { count: u64, posts: Vec<OwnPost> },
}

/// A post's text and timestamp, to use in contexts where author and index are known.
#[derive(PartialEq, Debug, Clone, Serialize, Deserialize, SimpleObject)]
pub struct OwnPost {
    /// The timestamp of the block in which the post operation was included.
    pub timestamp: Timestamp,
    /// The posted text.
    pub text: String,
}

/// A post on the social app.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Post {
    /// The key identifying the post, including the timestamp, author and index.
    pub key: Key,
    /// The post's text content.
    pub text: String,
}

/// A key by which a post is indexed.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize, SimpleObject, InputObject)]
#[graphql(input_name = "KeyInput")]
pub struct Key {
    /// The timestamp of the block in which the post was included on the author's chain.
    pub timestamp: Timestamp,
    /// The owner of the chain on which the `Post` operation was included.
    pub author: ChainId,
    /// The number of posts by that author before this one.
    pub index: u64,
}

// Serialize keys so that the lexicographic order of the serialized keys corresponds to reverse
// chronological order, then sorted by author, then by descending index.
impl CustomSerialize for Key {
    fn to_custom_bytes(&self) -> Result<Vec<u8>, views::ViewError> {
        let data = (
            (!self.timestamp.micros()).to_be_bytes(),
            &self.author,
            (!self.index).to_be_bytes(),
        );
        Ok(bcs::to_bytes(&data)?)
    }

    fn from_custom_bytes(short_key: &[u8]) -> Result<Self, views::ViewError> {
        let (time_bytes, author, idx_bytes) = (bcs::from_bytes(short_key))?;
        Ok(Self {
            timestamp: Timestamp::from(!u64::from_be_bytes(time_bytes)),
            author,
            index: !u64::from_be_bytes(idx_bytes),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Key;
    use linera_sdk::base::{ChainId, Timestamp};
    use linera_views::common::CustomSerialize;
    use webassembly_test::webassembly_test;

    #[webassembly_test]
    fn test_key_custom_serialize() {
        let key = Key {
            timestamp: Timestamp::from(0x123456789ABCDEF),
            author: ChainId([0x12345, 0x6789A, 0xBCDEF, 0x0248A].into()),
            index: 0xFEDCBA9876543210,
        };
        let ser_key = key
            .to_custom_bytes()
            .expect("serialization of Key should succeeed");
        let deser_key =
            Key::from_custom_bytes(&ser_key).expect("deserialization of Key should succeed");
        assert_eq!(key, deser_key);
    }
}
